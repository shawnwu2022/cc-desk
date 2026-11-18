use crate::{P256Spki, SecurityError};
use rustls::pki_types::SubjectPublicKeyInfoDer;
use rustls::sign::{Signer, SigningKey};
use rustls::{Error, SignatureAlgorithm, SignatureScheme};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

#[link(wasm_import_module = "cc_desk_browser_tls_v1")]
unsafe extern "C" {
    fn init(spki: *const u8, length: u32, deadline: f64) -> i32;
    fn now() -> f64;
    fn sign(
        sequence: u32,
        deadline: f64,
        message: *const u8,
        length: u32,
        output: *mut u8,
        capacity: u32,
    ) -> i32;
    fn close();
}
static CLAIMED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
struct State {
    public: P256Spki,
    deadline: f64,
    closed: AtomicBool,
    active: AtomicBool,
    sequence: AtomicU32,
}
impl State {
    fn live(&self) -> bool {
        let current = unsafe { now() };
        !self.closed.load(Ordering::SeqCst) && current.is_finite() && current < self.deadline
    }
    fn stop(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            // import 只由可信 DedicatedWorker adapter 提供；不持有锁或借出 Rust 引用。
            unsafe { close() };
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 每个 WASM 实例只允许一个浏览器 TLS key owner；公开元数据须来自可信登记。
#[derive(Clone, Debug)]
pub struct BrowserWasmSigningKey {
    state: Arc<State>,
}
impl BrowserWasmSigningKey {
    pub fn new(public: P256Spki, setup_deadline: f64) -> Result<Self, SecurityError> {
        let current = unsafe { now() };
        if !setup_deadline.is_finite()
            || !current.is_finite()
            || setup_deadline <= current
            || setup_deadline - current > 10_000.0
            || CLAIMED.swap(true, Ordering::SeqCst)
        {
            return Err(SecurityError::Deadline);
        }
        if unsafe { init(public.0.as_ptr(), public.0.len() as u32, setup_deadline) } != 1 {
            unsafe { close() };
            return Err(SecurityError::Identity);
        }
        Ok(Self {
            state: Arc::new(State {
                public,
                deadline: setup_deadline,
                closed: AtomicBool::new(false),
                active: AtomicBool::new(false),
                sequence: AtomicU32::new(0),
            }),
        })
    }
    pub fn close(&self) {
        self.state.stop();
    }
}
impl SigningKey for BrowserWasmSigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        (!self.state.closed.load(Ordering::SeqCst)
            && offered.contains(&SignatureScheme::ECDSA_NISTP256_SHA256))
        .then(|| Box::new(self.clone()) as Box<dyn Signer>)
    }
    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ECDSA
    }
    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(SubjectPublicKeyInfoDer::from(
            self.state.public.0.as_slice(),
        ))
    }
}
impl Signer for BrowserWasmSigningKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        let error = || Error::General("browser TLS signing failed".into());
        if self.state.active.swap(true, Ordering::SeqCst) {
            self.state.stop();
            return Err(error());
        }
        let result = (|| {
            if !self.state.live()
                || message.len() != 130
                || !message[..64].iter().all(|v| *v == 32)
                || &message[64..98] != b"TLS 1.3, client CertificateVerify\0"
            {
                return Err(error());
            }
            let previous = self
                .state
                .sequence
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    (value < 0x7fff_ffff).then_some(value + 1)
                })
                .map_err(|_| error())?;
            let mut output = [0u8; 72];
            // 唯一外部写入区是本栈上72字节；输入已严格定长，adapter 不得写入输入区。
            let length = unsafe {
                sign(
                    previous + 1,
                    self.state.deadline,
                    message.as_ptr(),
                    message.len() as u32,
                    output.as_mut_ptr(),
                    output.len() as u32,
                )
            };
            if !(8..=72).contains(&length) || !self.state.live() {
                return Err(error());
            }
            let signature = &output[..length as usize];
            // 即使主线程使用了错误 key，也不能以声称的公开元数据伪造 LocalIdentity 成功签名。
            ring::signature::UnparsedPublicKey::new(
                &ring::signature::ECDSA_P256_SHA256_ASN1,
                &self.state.public.0[26..],
            )
            .verify(message, signature)
            .map_err(|_| error())?;
            if !self.state.live() {
                return Err(error());
            }
            Ok(signature.to_vec())
        })();
        self.state.active.store(false, Ordering::SeqCst);
        if result.is_err() {
            self.state.stop();
        }
        result
    }
    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ECDSA_NISTP256_SHA256
    }
}
