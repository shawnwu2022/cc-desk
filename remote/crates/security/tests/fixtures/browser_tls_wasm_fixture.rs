#![no_main]
#[cfg(target_arch = "wasm32")]
mod fixture {
    use cc_desk_remote_security::{BrowserWasmSigningKey, LocalIdentity, P256Spki, Role};
    use rustls::{SignatureScheme, sign::SigningKey};
    use std::sync::{Arc, Mutex};
    static KEY: Mutex<Option<Arc<BrowserWasmSigningKey>>> = Mutex::new(None);
    static SIGNATURE: Mutex<Vec<u8>> = Mutex::new(Vec::new());
    fn public() -> Vec<u8> {
        // PUBLIC TEST_ONLY scalar1 的公开点；无私钥或登记行为。
        let hex = "3059301306072a8648ce3d020106082a8648ce3d030107034200046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn fixture_init(deadline: f64, wrong_identity: u32) -> i32 {
        let mut declared = public();
        if wrong_identity == 2 {
            // 同一个公开点的相反点：有效 P256 公钥，但不属于主线程 scalar1 key。
            let other_y = "b01cbd1c01e58065711814b583f061e9d431cca994cea1313449bf97c840ae0a";
            for i in 0..32 {
                declared[59 + i] = u8::from_str_radix(&other_y[i * 2..i * 2 + 2], 16).unwrap();
            }
        }
        let Ok(key) = BrowserWasmSigningKey::new(P256Spki::new(&declared).unwrap(), deadline)
        else {
            return -1;
        };
        let key = Arc::new(key);
        let mut expected = declared;
        if wrong_identity == 1 {
            expected[40] ^= 1;
        }
        if LocalIdentity::new(Role::Client, P256Spki::new(&expected).unwrap(), key.clone()).is_err()
        {
            key.close();
            return -1;
        }
        *KEY.lock().unwrap() = Some(key);
        1
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn fixture_sign(kind: u32) -> i32 {
        // clone 后立即释放 fixture 锁，绝不持锁跨入 JS。
        let Some(key) = KEY.lock().unwrap().clone() else {
            return -1;
        };
        let Some(signer) = key.choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256]) else {
            return -1;
        };
        let mut message = vec![32; 64];
        message.extend_from_slice(b"TLS 1.3, client CertificateVerify\0");
        message.extend_from_slice(&[7; 32]);
        match kind {
            1 => message[64] ^= 1,
            2 => {
                message.pop();
            }
            3 => message.resize(1024, 0),
            _ => {}
        }
        match signer.sign(&message) {
            Ok(signature) => {
                let length = signature.len() as i32;
                *SIGNATURE.lock().unwrap() = signature;
                length
            }
            Err(_) => -1,
        }
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn fixture_signature_byte(index: u32) -> u32 {
        SIGNATURE
            .lock()
            .unwrap()
            .get(index as usize)
            .copied()
            .map_or(256, u32::from)
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn fixture_close() {
        let key = KEY.lock().unwrap().take();
        drop(key);
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn fixture_grow() -> u32 {
        core::arch::wasm32::memory_grow::<0>(1) as u32
    }
}
