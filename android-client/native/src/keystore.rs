//! Application Keystore adapter. The Tokio connection owner never invokes a
//! Java method. Rustls's synchronous signing interface is bridged by one
//! bounded wait on the retained worker; timeout stops admitting new jobs.
use cc_desk_remote_security::{
    GrantError, LocalIdentity, NativeGrantSigner, P256Spki, Role, VerifiedCanonicalGrant,
};
use cc_desk_remote_transport::wss::{RelayRole, RelaySigner, SignError, SigningRequest};
use coset::CborSerializable;
use jni::{
    JNIEnv, JavaVM,
    objects::{GlobalRef, JObject, JValue},
};
use rustls::{
    Error as TlsError, SignatureAlgorithm, SignatureScheme,
    pki_types::SubjectPublicKeyInfoDer,
    sign::{Signer, SigningKey},
};
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::sync::oneshot;

const MAX_SIGN_INPUT: usize = 16 * 1024;
const MAX_TLS_WAIT: Duration = Duration::from_millis(150);

enum Work {
    Endpoint(Vec<u8>, mpsc::SyncSender<Option<Vec<u8>>>),
    Relay(Vec<u8>, oneshot::Sender<Option<Vec<u8>>>),
    Enrollment(Vec<u8>, oneshot::Sender<Option<Vec<u8>>>),
    Prompt {
        id: i64,
        tbs: Vec<u8>,
        scope: [String; 3],
        controller: bool,
    },
    Cancel(i64),
}

struct Queue {
    tx: mpsc::Sender<Work>,
    retired: Arc<AtomicBool>,
}

impl Queue {
    fn post(&self, request: Work) -> bool {
        if self.retired.load(Ordering::Acquire) {
            return false;
        }
        self.tx.send(request).is_ok()
    }
}

pub(crate) struct KeystoreOwner {
    worker: JoinHandle<bool>,
    retired: Arc<AtomicBool>,
}

impl KeystoreOwner {
    /// Called only from the retained native Observer owner after all profile
    /// providers have dropped. If Binder has stopped returning, the attempt
    /// intentionally remains occupied and cannot accept a replacement.
    pub(crate) fn retire(self) -> bool {
        self.retired.store(true, Ordering::Release);
        matches!(self.worker.join(), Ok(true))
    }
}

pub(crate) struct Roles {
    keys: crate::enrollment::NativeKeystoreBindings,
    queue: Arc<Queue>,
    owner: KeystoreOwner,
}

impl Roles {
    /// Proof for one locally constructed Phone enrollment transcript.
    pub(crate) async fn sign_enrollment(
        &self,
        payload: Vec<u8>,
    ) -> Result<[u8; 64], crate::phone_enrollment::PairingError> {
        use crate::phone_enrollment::PairingError;
        if !payload.starts_with(b"cc-desk.native-pairing.phone.v1\0")
            || payload.len() > MAX_SIGN_INPUT
        {
            return Err(PairingError::Invalid);
        }
        let (tx, rx) = oneshot::channel();
        if !self.queue.post(Work::Enrollment(payload, tx)) {
            return Err(PairingError::Unavailable);
        }
        rx.await
            .ok()
            .flatten()
            .and_then(|v| v.try_into().ok())
            .ok_or(PairingError::Unavailable)
    }
    pub(crate) fn relay_for_enrollment(
        &self,
        id: [u8; 16],
    ) -> cc_desk_remote_transport::wss::RelayAdmissionIdentity {
        cc_desk_remote_transport::wss::RelayAdmissionIdentity::new(
            id,
            RelayRole::Phone,
            self.keys.relay.clone(),
        )
    }

    pub(crate) fn into_parts(self) -> (crate::enrollment::NativeKeystoreBindings, KeystoreOwner) {
        (self.keys, self.owner)
    }
}

/// Native platform orchestration only: `bridge` is the app's private role
/// facade holding the three verified HardwareIdentity entries. No JNI export
/// accepts a caller-supplied object as a trust token.
pub(crate) fn open_roles(
    vm: JavaVM,
    bridge: GlobalRef,
    endpoint_spki: [u8; 91],
    origin: cc_desk_remote_record::appbind::CanonicalOrigin,
    relay_id: [u8; 16],
) -> Result<Roles, ()> {
    let spki = P256Spki::new(&endpoint_spki).map_err(|_| ())?;
    let (tx, rx) = mpsc::channel();
    let retired = Arc::new(AtomicBool::new(false));
    let queue = Arc::new(Queue {
        tx,
        retired: retired.clone(),
    });
    let endpoint = LocalIdentity::new(
        Role::Client,
        spki,
        Arc::new(Endpoint {
            queue: queue.clone(),
            spki: SubjectPublicKeyInfoDer::from(endpoint_spki.to_vec()),
        }),
    )
    .map_err(|_| ())?;
    let worker_retired = retired.clone();
    let worker = thread::Builder::new()
        .name("ccdesk-keystore".into())
        .spawn(move || signer_thread(vm, bridge, rx, worker_retired))
        .map_err(|_| ())?;
    Ok(Roles {
        keys: crate::enrollment::NativeKeystoreBindings {
            endpoint,
            relay: Arc::new(Relay {
                queue: queue.clone(),
                origin,
                relay_id,
            }),
            approval: Box::new(Approval {
                queue: queue.clone(),
            }),
        },
        queue,
        owner: KeystoreOwner { worker, retired },
    })
}

fn signer_thread(
    vm: JavaVM,
    target: GlobalRef,
    rx: mpsc::Receiver<Work>,
    retired: Arc<AtomicBool>,
) -> bool {
    let Ok(mut env) = vm.attach_current_thread() else {
        return false;
    };
    for work in rx {
        match work {
            Work::Endpoint(bytes, reply) => {
                let signature = (!retired.load(Ordering::Acquire))
                    .then(|| sign(&mut env, &target, "signEndpointTls", &bytes, 80))
                    .flatten();
                let _ = reply.send(signature);
            }
            Work::Relay(bytes, reply) => {
                if !reply.is_closed() {
                    let signature = (!retired.load(Ordering::Acquire))
                        .then(|| sign(&mut env, &target, "signRelay", &bytes, 64))
                        .flatten();
                    let _ = reply.send(signature);
                }
            }

            Work::Enrollment(bytes, reply) => {
                if !reply.is_closed() && !retired.load(Ordering::Acquire) {
                    let _ = reply.send(sign(&mut env, &target, "signPhoneEnrollment", &bytes, 64));
                }
            }

            Work::Prompt {
                id,
                tbs,
                scope,
                controller,
            } => {
                if retired.load(Ordering::Acquire) || !is_pending(id) {
                    continue;
                }
                let result = env.with_local_frame(16, |env| {
                    let a = JObject::from(env.byte_array_from_slice(&tbs)?);
                    let s = JObject::from(env.new_string(&scope[0])?);
                    let r = JObject::from(env.new_string(&scope[1])?);
                    let g = JObject::from(env.new_string(&scope[2])?);
                    env.call_method(
                        target.as_obj(),
                        "requestApproval",
                        "(J[BLjava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)V",
                        &[
                            JValue::Long(id),
                            JValue::Object(&a),
                            JValue::Object(&s),
                            JValue::Object(&r),
                            JValue::Object(&g),
                            JValue::Bool(u8::from(controller)),
                        ],
                    )?;
                    Ok::<(), jni::errors::Error>(())
                });
                if result.is_err() {
                    clear_exception(&mut env);
                    complete(id, None);
                }
            }
            Work::Cancel(id) => {
                if env
                    .call_method(
                        target.as_obj(),
                        "cancelApproval",
                        "(J)V",
                        &[JValue::Long(id)],
                    )
                    .is_err()
                {
                    clear_exception(&mut env);
                }
            }
        }
    }
    true
}

fn clear_exception(env: &mut JNIEnv<'_>) {
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
}

fn sign(
    env: &mut JNIEnv<'_>,
    target: &GlobalRef,
    method: &str,
    bytes: &[u8],
    max: usize,
) -> Option<Vec<u8>> {
    let result = env.with_local_frame(8, |env| {
        let arg = JObject::from(env.byte_array_from_slice(bytes)?);
        let output = env
            .call_method(target.as_obj(), method, "([B)[B", &[JValue::Object(&arg)])?
            .l()?;
        if output.is_null() {
            return Ok(None);
        }
        let output = jni::objects::JByteArray::from(output);
        let length = env.get_array_length(&output)? as usize;
        if length == 0 || length > max {
            return Ok(None);
        }
        Ok::<Option<Vec<u8>>, jni::errors::Error>(Some(env.convert_byte_array(output)?))
    });
    if result.is_err() {
        clear_exception(env);
    }
    result.ok().flatten()
}

struct Endpoint {
    queue: Arc<Queue>,
    spki: SubjectPublicKeyInfoDer<'static>,
}

struct TlsSigner(Arc<Queue>);

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AndroidEndpointSigner(<hardware>)")
    }
}
impl fmt::Debug for TlsSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AndroidTlsSigner(<hardware>)")
    }
}
impl SigningKey for Endpoint {
    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ECDSA
    }
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered
            .contains(&SignatureScheme::ECDSA_NISTP256_SHA256)
            .then(|| Box::new(TlsSigner(self.queue.clone())) as Box<dyn Signer>)
    }
    fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
        Some(self.spki.as_ref().into())
    }
}
impl Signer for TlsSigner {
    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ECDSA_NISTP256_SHA256
    }
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, TlsError> {
        let failure = || TlsError::General("Android hardware signature unavailable".into());
        if message.is_empty() || message.len() > MAX_SIGN_INPUT {
            return Err(failure());
        }
        let (tx, rx) = mpsc::sync_channel(1);
        if !self.0.post(Work::Endpoint(message.to_vec(), tx)) {
            return Err(failure());
        }
        let response = rx.recv_timeout(MAX_TLS_WAIT).ok().flatten();
        if response.is_none() {
            self.0.retired.store(true, Ordering::Release);
        }
        response.ok_or_else(failure)
    }
}

struct Relay {
    queue: Arc<Queue>,
    origin: cc_desk_remote_record::appbind::CanonicalOrigin,
    relay_id: [u8; 16],
}
impl RelaySigner for Relay {
    fn sign<'a>(
        &'a self,
        request: &'a SigningRequest,
    ) -> Pin<Box<dyn Future<Output = Result<[u8; 64], SignError>> + Send + 'a>> {
        Box::pin(async move {
            let bytes = request.transcript();
            if request.origin() != self.origin.as_str()
                || request.key_id() != self.relay_id
                || request.role() != RelayRole::Phone
                || bytes.is_empty()
                || bytes.len() > MAX_SIGN_INPUT
            {
                return Err(SignError);
            }
            let (tx, rx) = oneshot::channel();
            if !self.queue.post(Work::Relay(bytes.to_vec(), tx)) {
                return Err(SignError);
            }
            rx.await
                .ok()
                .flatten()
                .and_then(|x| x.try_into().ok())
                .ok_or(SignError)
        })
    }
}

struct Approval {
    queue: Arc<Queue>,
}
struct ApprovalSlot {
    sequence: i64,
    match_reply: Option<(i64, oneshot::Sender<Option<[u8; 64]>>)>,
}
static APPROVAL: Mutex<ApprovalSlot> = Mutex::new(ApprovalSlot {
    sequence: 0,
    match_reply: None,
});

fn is_pending(id: i64) -> bool {
    APPROVAL
        .lock()
        .is_ok_and(|s| s.match_reply.as_ref().is_some_and(|(n, _)| *n == id))
}
pub(crate) fn complete(id: i64, signature: Option<[u8; 64]>) {
    if let Ok(mut slot) = APPROVAL.lock() {
        if slot.match_reply.as_ref().is_some_and(|(n, _)| *n == id) {
            if let Some((_, reply)) = slot.match_reply.take() {
                let _ = reply.send(signature);
            }
        }
    }
}
struct CancelPrompt {
    id: i64,
    queue: Arc<Queue>,
}
impl Drop for CancelPrompt {
    fn drop(&mut self) {
        if is_pending(self.id) {
            complete(self.id, None);
            let _ = self.queue.tx.send(Work::Cancel(self.id));
        }
    }
}

impl NativeGrantSigner for Approval {
    fn confirm_and_sign<'a>(
        &'a mut self,
        grant: &'a VerifiedCanonicalGrant,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, GrantError>> + Send + 'a>> {
        Box::pin(async move {
            let fail = || GrantError::Policy;
            let (mut candidate, tbs) = grant.signing_parts()?;
            if tbs.is_empty() || tbs.len() > MAX_SIGN_INPUT {
                return Err(fail());
            }
            let (id, rx) = {
                let mut slot = APPROVAL.lock().map_err(|_| fail())?;
                if slot.match_reply.is_some() {
                    return Err(fail());
                }
                let id = slot.sequence.checked_add(1).ok_or_else(fail)?;
                let (tx, rx) = oneshot::channel();
                slot.sequence = id;
                slot.match_reply = Some((id, tx));
                (id, rx)
            };
            let _guard = CancelPrompt {
                id,
                queue: self.queue.clone(),
            };
            let scope = &grant.payload().scope;
            if !self.queue.post(Work::Prompt {
                id,
                tbs,
                controller: grant.payload().role == cc_desk_remote_client::ClientRole::Controller,
                scope: [
                    scope.session.clone(),
                    scope.run.clone(),
                    scope.generation.clone(),
                ],
            }) {
                return Err(fail());
            }
            let signed = rx.await.ok().flatten().ok_or_else(fail)?;
            candidate.signature = signed.to_vec();
            candidate.to_vec().map_err(|_| fail())
        })
    }
}

/// Synchronous disk commit for the dedicated native enrollment worker, not
/// called from an Android Activity/UI thread or a transport poll. The facade
/// commits the opaque receipt under its hardware-only no-backup store.
pub struct AndroidDurableStore<'a, 'local> {
    env: &'a mut JNIEnv<'local>,
    facade: &'a JObject<'local>,
}

impl<'a, 'local> AndroidDurableStore<'a, 'local> {
    pub(crate) fn load(&mut self, local_id: [u8; 16]) -> Option<zeroize::Zeroizing<Vec<u8>>> {
        if local_id == [0; 16] {
            return None;
        }
        let id: String = local_id.iter().map(|x| format!("{x:02x}")).collect();
        let result = self.env.with_local_frame(8, |env| {
            let arg = JObject::from(env.new_string(id)?);
            let raw = env
                .call_method(
                    self.facade,
                    "readPairingRecord",
                    "(Ljava/lang/String;)[B",
                    &[JValue::Object(&arg)],
                )?
                .l()?;
            if raw.is_null() {
                return Ok(None);
            }
            let raw = jni::objects::JByteArray::from(raw);
            if !(1..=9216).contains(&env.get_array_length(&raw)?) {
                return Ok(None);
            }
            Ok::<_, jni::errors::Error>(Some(zeroize::Zeroizing::new(env.convert_byte_array(raw)?)))
        });
        if result.is_err() {
            clear_exception(self.env);
        }
        result.ok().flatten()
    }

    pub fn new(env: &'a mut JNIEnv<'local>, facade: &'a JObject<'local>) -> Self {
        Self { env, facade }
    }
}

impl crate::enrollment::DurableNativePairingStore for AndroidDurableStore<'_, '_> {
    type Error = ();
    fn insert_once(&mut self, local_id: [u8; 16], record: &[u8]) -> Result<(), ()> {
        let id: String = local_id.iter().map(|n| format!("{n:02x}")).collect();
        let result = self.env.with_local_frame(8, |env| {
            let id = JObject::from(env.new_string(id)?);
            let arg = JObject::from(env.byte_array_from_slice(record)?);
            env.call_method(
                self.facade,
                "sealPairingRecord",
                "(Ljava/lang/String;[B)Z",
                &[JValue::Object(&id), JValue::Object(&arg)],
            )?
            .z()
        });
        if result.is_err() {
            clear_exception(self.env);
        }
        if result.unwrap_or(false) {
            Ok(())
        } else {
            Err(())
        }
    }
}
