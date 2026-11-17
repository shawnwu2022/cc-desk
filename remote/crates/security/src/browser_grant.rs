use crate::{
    BrowserProfile, GrantClock, GrantError, NativeGrantSigner, P256Spki, VerifiedCanonicalGrant,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use coset::TaggedCborSerializable;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::Duration,
};
#[link(wasm_import_module = "cc_desk_browser_grant_v1")]
unsafe extern "C" {
    fn begin(pointer: *const u8, length: u32) -> i32;
    fn read(kind: u32, pointer: *mut u8, capacity: u32) -> i32;
    fn cose(pointer: *const u8, length: u32) -> i32;
    fn cancel();
}
static WAITER: Mutex<Option<Waker>> = Mutex::new(None);
pub(super) fn notify() {
    let wake = WAITER.lock().unwrap().take();
    if let Some(w) = wake {
        w.wake();
    }
}
pub(super) struct Configuration {
    pub trust: crate::browser_grant_config::Trust,
    pub signer: Signer,
    pub budget: Duration,
    pub peer_budget: Option<Duration>,
    pub control_lease: Option<crate::browser_grant_config::ControlLeaseMode>,
    pub snapshot_pull: Option<crate::browser_grant_config::SnapshotPullMode>,
}
impl Configuration {
    pub fn parse(bytes: &[u8]) -> Result<Self, GrantError> {
        let parsed = crate::browser_grant_config::Configuration::parse(bytes)?;
        Ok(Self {
            trust: parsed.trust,
            signer: Signer {
                profile: parsed.profile,
                issuer: parsed.issuer,
            },
            budget: parsed.budget,
            peer_budget: parsed.peer_budget,
            control_lease: parsed.control_lease,
            snapshot_pull: parsed.snapshot_pull,
        })
    }
}
pub(super) struct Signer {
    profile: BrowserProfile,
    issuer: P256Spki,
}
struct Pending {
    done: bool,
}
impl Drop for Pending {
    fn drop(&mut self) {
        WAITER.lock().unwrap().take();
        if !self.done {
            unsafe { cancel() };
        }
    }
}
async fn response(kind: u32, capacity: usize) -> Result<Vec<u8>, GrantError> {
    let mut bytes = vec![0u8; capacity];
    std::future::poll_fn(|cx| {
        *WAITER.lock().unwrap() = Some(cx.waker().clone());
        // 不持锁进入JS；事件注册和读取都发生在同一受控Worker。
        let n = unsafe { read(kind, bytes.as_mut_ptr(), capacity as u32) };
        if n == -1 {
            Poll::Pending
        } else {
            WAITER.lock().unwrap().take();
            if n < 1 || n as usize > capacity {
                Poll::Ready(Err(GrantError::Closed))
            } else {
                bytes.truncate(n as usize);
                Poll::Ready(Ok(std::mem::take(&mut bytes)))
            }
        }
    })
    .await
}
impl NativeGrantSigner for Signer {
    fn browser_profile(&self) -> Option<BrowserProfile> {
        Some(self.profile.clone())
    }
    fn confirm_and_sign<'a>(
        &'a mut self,
        grant: &'a VerifiedCanonicalGrant,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>> {
        Box::pin(async move {
            let mut pending = Pending { done: false };
            let (mut candidate, tbs) = grant.signing_parts()?;
            if tbs.len() > 10000 {
                return Err(GrantError::Bounds);
            }
            let challenge = grant.browser_challenge(&self.profile)?;
            let presentation = cc_desk_remote_record::grant::encode(grant.payload())
                .map_err(|_| GrantError::Encoding)?;
            let request=serde_json::to_vec(&serde_json::json!({"challenge":B64.encode(challenge),"tbs":B64.encode(tbs),"presentation":B64.encode(presentation)})).map_err(|_|GrantError::Encoding)?;
            if request.len() > 32768
                || unsafe { begin(request.as_ptr(), request.len() as u32) } != 1
            {
                return Err(GrantError::Closed);
            }
            let signature = response(1, 64).await?;
            if signature.len() != 64 {
                return Err(GrantError::Bounds);
            }
            candidate.signature = signature;
            let signed = candidate
                .to_tagged_vec()
                .map_err(|_| GrantError::Encoding)?;
            // 错grantKey在WebAuthn前拒绝；最终profile/Host验证仍照常执行。
            crate::grant::signature(&signed, grant.payload(), &self.issuer, grant.issuer_kid())?;
            if signed.len() > 8192 || unsafe { cose(signed.as_ptr(), signed.len() as u32) } != 1 {
                return Err(GrantError::Closed);
            }
            let result = response(2, 16384).await?;
            pending.done = true;
            Ok(result)
        })
    }
}
pub(super) fn clock() -> Arc<dyn GrantClock> {
    Arc::new(|| {
        let now = unsafe { super::now() };
        if !now.is_finite() || !(0.0..=9007199254740991.0).contains(&now) {
            return Err(GrantError::Clock);
        }
        Ok(now.floor() as u64)
    })
}
