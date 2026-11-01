//! Default-off original-document IPC. No JS connection/grant factory exists.
#[cfg(test)]
#[path = "remote_client_bootstrap_tests.rs"]
mod bootstrap_tests;
use crate::cli::{
    document::{DocumentBinding, DocumentRevocation},
    launch_service::NativeRun,
    native_runtime::NativeRuntime,
    profiles::error,
    snapshot::CallerIdentity,
    types::SafeError,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use cc_desk_remote_client::NativeClient;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU8, Ordering},
    Arc, Weak,
};
use std::time::Duration;
use tauri::ipc::{InvokeBody, Request};
use tauri::{State, Webview};
use tokio::sync::{watch, Mutex as AsyncMutex};
/// Shared main-window / explicit-exit call point for the client-only feature.
#[cfg(not(feature = "remote-host"))]
pub(crate) fn request_app_exit(app: &tauri::AppHandle, code: i32) -> bool {
    use tauri::Manager;
    let service = app.state::<Arc<RemoteClientService>>().inner().clone();
    match service.request_exit(code) {
        ExitDecision::Proceed => false,
        ExitDecision::Prevent => true,
        ExitDecision::Wait(original) => {
            let app = app.clone();
            service.start_exit_waiter(original, move |code| app.exit(code));
            true
        }
    }
}
#[cfg(windows)]
use crate::native_crypto::{
    owner::{KeyCustodian, KeyRequestHandle, KeyRetirement, KeyStart},
    signers::{
        NativeGrantPresence, NativeKeyProfile, NativeKeySlot, NativeSignerAuthority,
        NativeSignerBinding, NativeSignerSet,
    },
    SigningPurpose,
};
const MAX_REQUEST: usize = 32768;
const MAX_PAYLOAD: usize = 16270;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireRequest {
    resource_id: u32,
    budget_ms: u16,
    payload: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CloseRequest {
    resource_id: u32,
}
#[derive(Serialize)]
pub(crate) struct ByteReply {
    payload: String,
}
#[derive(Serialize)]
pub(crate) struct SnapshotReply {
    stamp: serde_json::Value,
    parts: Vec<String>,
}
fn decode<T: serde::de::DeserializeOwned>(body: &InvokeBody, limit: usize) -> Result<T, SafeError> {
    let InvokeBody::Raw(bytes) = body else {
        return Err(error("RAW_BODY_REQUIRED"));
    };
    if bytes.len() > limit {
        return Err(error("REQUEST_TOO_LARGE"));
    }
    serde_json::from_slice(bytes).map_err(|_| error("INVALID_REQUEST"))
}
fn request(body: &InvokeBody) -> Result<(u32, Vec<u8>, Duration), SafeError> {
    let wire: WireRequest = decode(body, MAX_REQUEST)?;
    if wire.resource_id == 0
        || wire.budget_ms == 0
        || wire.budget_ms > 1000
        || wire.payload.len() > 21696
    {
        return Err(error("INVALID_REQUEST"));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(&wire.payload)
        .map_err(|_| error("INVALID_REQUEST"))?;
    if bytes.len() > MAX_PAYLOAD || URL_SAFE_NO_PAD.encode(&bytes) != wire.payload {
        return Err(error("INVALID_REQUEST"));
    }
    Ok((
        wire.resource_id,
        bytes,
        Duration::from_millis(u64::from(wire.budget_ms)),
    ))
}
struct OwnedClient {
    id: u32,
    caller: CallerIdentity,
    binding: Weak<DocumentBinding<NativeRun>>,
    client: Arc<NativeClient>,
    published: AtomicBool,
    closed: AtomicBool,
    close_tx: watch::Sender<bool>,
    join: AsyncMutex<Option<tauri::async_runtime::JoinHandle<Result<(), ()>>>>,
    joined: AtomicU8,
    ticket: Mutex<Option<DocumentRevocation<NativeRun>>>,
}
impl OwnedClient {
    fn new(
        id: u32,
        caller: CallerIdentity,
        binding: &Arc<DocumentBinding<NativeRun>>,
        client: NativeClient,
    ) -> Arc<Self> {
        let (tx, mut rx) = watch::channel(false);
        let resource = Arc::new(Self {
            id,
            caller,
            binding: Arc::downgrade(binding),
            client: Arc::new(client),
            published: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            close_tx: tx,
            join: AsyncMutex::new(None),
            joined: AtomicU8::new(0),
            ticket: Mutex::new(None),
        });
        let owned = resource.clone();
        let worker = tauri::async_runtime::spawn(async move {
            if !*rx.borrow() {
                let _ = rx.changed().await;
            }
            owned.client.request_close();
            // Completion proves the original owner task was awaited. The
            // current channel API folds inner JoinError into Closed, so this
            // does not attest normal protocol exit or absence of an inner panic.
            owned.client.retire().await.map(|_| ()).map_err(|_| ())
        });
        // Construction is synchronous; no command can observe the resource before
        // its join handle is retained. The worker also retains this exact resource.
        *resource
            .join
            .try_lock()
            .expect("unpublished retirement slot") = Some(worker);
        resource
    }
    fn retire(&self) {
        self.closed.store(true, Ordering::Release);
        self.client.request_close();
        self.close_tx.send_replace(true);
    }
    fn check(&self) -> Result<(), SafeError> {
        if !self.published.load(Ordering::Acquire) {
            return Err(error("REMOTE_CLIENT_UNAVAILABLE"));
        }
        self.check_private()
    }
    fn check_private(&self) -> Result<(), SafeError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        self.binding
            .upgrade()
            .ok_or_else(|| error("FORBIDDEN"))?
            .check_admitted(&self.caller)
    }
    async fn close_join(&self) -> Result<(), SafeError> {
        self.retire();
        let mut join = self.join.lock().await;
        if let Some(worker) = join.as_mut() {
            let result = worker.await;
            *join = None;
            self.joined.store(
                if matches!(result, Ok(Ok(()))) { 1 } else { 2 },
                Ordering::Release,
            );
        }
        if self.joined.load(Ordering::Acquire) != 1 {
            return Err(error("REMOTE_CLIENT_RETIRE_UNKNOWN"));
        }
        // Releasing the registration after actual join permits a new native owner;
        // an older ticket can never remove a newer document registration.
        self.ticket.lock().take();
        Ok(())
    }
}
#[derive(Clone, Copy)]
#[cfg(any(test, not(feature = "remote-host")))]
enum ExitIntent {
    Idle,
    Waiting(i32),
    Ready(i32),
    Unknown,
}
#[derive(Debug, PartialEq, Eq)]
#[cfg(any(test, not(feature = "remote-host")))]
pub(crate) enum ExitDecision {
    Proceed,
    Wait(i32),
    Prevent,
}
struct ServiceState {
    #[cfg(windows)]
    signing: Option<Arc<NativeSignerAuthority>>,
    #[cfg(any(test, not(feature = "remote-host")))]
    exit_waiter: Option<tauri::async_runtime::JoinHandle<()>>,
    #[cfg(windows)]
    keys: Vec<Arc<NativeKeySlot>>,
    next_id: u32,
    creating: bool,
    closing: bool,
    creation_unknown: bool,
    async_creating: bool,
    creation_stop: Option<watch::Sender<bool>>,
    creation_id: Option<u32>,
    creation_result: Option<Result<(), SafeError>>,
    #[cfg(any(test, not(feature = "remote-host")))]
    exit: ExitIntent,
    resource: Option<Arc<OwnedClient>>,
}
impl ServiceState {
    fn has_keys(&self) -> bool {
        #[cfg(windows)]
        {
            !self.keys.is_empty()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}
pub(crate) struct RemoteClientService {
    state: Mutex<ServiceState>,
    creation_finished: watch::Sender<()>,
    creation_join: AsyncMutex<Option<tauri::async_runtime::JoinHandle<Result<(), SafeError>>>>,
}
impl Default for RemoteClientService {
    fn default() -> Self {
        Self {
            state: Mutex::new(ServiceState {
                #[cfg(windows)]
                signing: None,
                #[cfg(any(test, not(feature = "remote-host")))]
                exit_waiter: None,
                #[cfg(windows)]
                keys: Vec::new(),
                next_id: 0,
                creating: false,
                closing: false,
                creation_unknown: false,
                async_creating: false,
                creation_stop: None,
                creation_id: None,
                creation_result: None,
                #[cfg(any(test, not(feature = "remote-host")))]
                exit: ExitIntent::Idle,
                resource: None,
            }),
            creation_finished: watch::channel(()).0,
            creation_join: AsyncMutex::new(None),
        }
    }
}
enum CreationAuthority {
    #[cfg(windows)]
    Native(crate::native_crypto::owner::NativeSigningAttempt),
    #[cfg(test)]
    TestOnly,
}
struct CreationGuard<'a> {
    service: &'a RemoteClientService,
    id: u32,
    published: bool,
}
impl Drop for CreationGuard<'_> {
    fn drop(&mut self) {
        if !self.published {
            let mut state = self.service.state.lock();
            state.creating = false;
            if std::thread::panicking() {
                state.closing = true;
                state.creation_unknown = true;
            }
            drop(state);
            self.service.creation_finished.send_replace(());
        }
    }
}
impl RemoteClientService {
    /// Only a backend-approved profile can reach this native entry. No IPC
    /// command or default profile source enables it.
    #[cfg(windows)]
    #[allow(dead_code)]
    pub(crate) fn install_profile(
        self: &Arc<Self>,
        binding: Arc<DocumentBinding<NativeRun>>,
        webview: &Webview,
        headers: &tauri::http::HeaderMap,
        mut profile: cc_desk_remote_client::bootstrap::NativeProfile,
    ) -> Result<u32, SafeError> {
        let caller = binding.admit_native(webview, headers)?;
        let authority = self
            .state
            .lock()
            .signing
            .clone()
            .ok_or_else(|| error("NATIVE_PROFILE_UNAVAILABLE"))?;
        if !authority.matches_origin(&profile.binding.origin) {
            return Err(error("NATIVE_BINDING_INVALID"));
        }
        let signers =
            self.fresh_native_signers(std::time::Instant::now() + Duration::from_secs(5))?;
        let public = signers
            .endpoint
            .public_key()
            .ok_or_else(|| error("NATIVE_PROFILE_INVALID"))?;
        profile.local = cc_desk_remote_security::LocalIdentity::new(
            cc_desk_remote_security::Role::Client,
            cc_desk_remote_security::P256Spki::new(public.as_ref())
                .map_err(|_| error("NATIVE_PROFILE_INVALID"))?,
            signers.endpoint,
        )
        .map_err(|_| error("NATIVE_PROFILE_INVALID"))?;
        profile.relay = signers.relay;
        profile.signer = signers.grant;
        self.start_owned_creation_checked(
            binding,
            caller,
            CreationAuthority::Native(signers.attempt),
            move |stop| async move {
                cc_desk_remote_client::bootstrap::connect_native(profile, stop)
                    .await
                    .map_err(|e| {
                        error(
                        if e == cc_desk_remote_client::bootstrap::BootstrapError::RetirementUnknown
                        {
                            "REMOTE_CLIENT_RETIRE_UNKNOWN"
                        } else {
                            "REMOTE_CLIENT_BOOTSTRAP_FAILED"
                        },
                    )
                    })
            },
        )
    }
    fn start_owned_creation_checked<F, U>(
        self: &Arc<Self>,
        binding: Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        authority: CreationAuthority,
        create: F,
    ) -> Result<u32, SafeError>
    where
        F: FnOnce(watch::Receiver<bool>) -> U + Send + 'static,
        U: std::future::Future<Output = Result<NativeClient, SafeError>> + Send + 'static,
    {
        binding.check_admitted(&caller)?;
        let mut slot = self
            .creation_join
            .try_lock()
            .map_err(|_| error("REMOTE_CLIENT_BUSY"))?;
        if slot.is_some() {
            return Err(error("REMOTE_CLIENT_BUSY"));
        }
        let mut reservation = self.begin_create()?;
        let id = reservation.id;
        let (stop, rx) = watch::channel(false);
        let revoked = stop.clone();
        let original_service = Arc::downgrade(self);
        let ticket = binding.on_revoke(
            &caller,
            Arc::new(move || {
                revoked.send_replace(true);
                if let Some(service) = original_service.upgrade() {
                    service.begin_shutdown();
                }
            }),
        )?;
        {
            let mut state = self.state.lock();
            state.async_creating = true;
            if state.closing {
                stop.send_replace(true);
            }
            state.creation_stop = Some(stop);
            state.creation_id = Some(id);
            state.creation_result = None;
        }
        let (ready, start) = tokio::sync::oneshot::channel();
        let service = self.clone();
        let task = tauri::async_runtime::spawn(async move {
            let outcome = if start.await.is_err() {
                Err(error("REMOTE_CLIENT_CLOSED"))
            } else {
                let client = if service.state.lock().closing {
                    Err(error("REMOTE_CLIENT_CLOSED"))
                } else {
                    create(rx).await
                };
                drop(ticket);
                match client {
                    Err(failure) => Err(failure),
                    Ok(client) => {
                        // Retain the exact result in the service before any check can fail.
                        let resource = OwnedClient::new(id, caller.clone(), &binding, client);
                        service.state.lock().resource = Some(resource.clone());
                        let published =
                            service.publish_owned(resource.clone(), &binding, authority);
                        if published.is_err() || service.state.lock().closing {
                            match service.close_resource(resource).await {
                                Ok(()) => {
                                    published.and_then(|_| Err(error("REMOTE_CLIENT_CLOSED")))
                                }
                                Err(failure) => Err(failure),
                            }
                        } else {
                            Ok(())
                        }
                    }
                }
            };
            {
                let mut state = service.state.lock();
                state.creating = false;
                if outcome
                    .as_ref()
                    .is_err_and(|e| e.code == "REMOTE_CLIENT_RETIRE_UNKNOWN")
                {
                    state.creation_unknown = true;
                    state.closing = true;
                }
            }
            service.creation_finished.send_replace(());
            outcome
        });
        *slot = Some(task);
        reservation.published = true;
        let _ = ready.send(());
        Ok(id)
    }
    #[cfg(test)]
    fn start_owned_creation<F, U>(
        self: &Arc<Self>,
        binding: Arc<DocumentBinding<NativeRun>>,
        caller: CallerIdentity,
        create: F,
    ) -> Result<u32, SafeError>
    where
        F: FnOnce(watch::Receiver<bool>) -> U + Send + 'static,
        U: std::future::Future<Output = Result<NativeClient, SafeError>> + Send + 'static,
    {
        self.start_owned_creation_checked(binding, caller, CreationAuthority::TestOnly, create)
    }
    fn publish_owned(
        &self,
        resource: Arc<OwnedClient>,
        binding: &Arc<DocumentBinding<NativeRun>>,
        authority: CreationAuthority,
    ) -> Result<u32, SafeError> {
        let weak = Arc::downgrade(&resource);
        let ticket = binding.on_revoke(
            &resource.caller,
            Arc::new(move || {
                if let Some(r) = weak.upgrade() {
                    r.retire();
                }
            }),
        )?;
        *resource.ticket.lock() = Some(ticket);
        let publish = |current: &dyn Fn() -> bool| {
            let state = self.state.lock();
            if state.closing {
                return Err(error("REMOTE_CLIENT_CLOSED"));
            }
            resource.check_private()?;
            if !current() {
                return Err(error("NATIVE_SIGNING_ATTEMPT_INVALID"));
            }
            resource.published.store(true, Ordering::Release);
            Ok(resource.id)
        };
        match authority {
            #[cfg(windows)]
            CreationAuthority::Native(attempt) => attempt
                .publish_if_current(publish)
                .map_err(|_| error("NATIVE_SIGNING_ATTEMPT_INVALID"))?,
            #[cfg(test)]
            CreationAuthority::TestOnly => publish(&|| true),
        }
    }
    /// Observers borrow the original slot; dropping a wait never drops its task.
    #[allow(dead_code)]
    pub(crate) async fn wait_creation(&self, id: u32) -> Result<(), SafeError> {
        let mut slot = self.creation_join.lock().await;
        if self.state.lock().creation_id != Some(id) {
            return Err(error("FORBIDDEN"));
        }
        if let Some(task) = slot.as_mut() {
            let outcome = match task.await {
                Ok(result) => result,
                Err(_) => {
                    let mut state = self.state.lock();
                    state.creation_unknown = true;
                    state.closing = true;
                    Err(error("REMOTE_CLIENT_RETIRE_UNKNOWN"))
                }
            };
            *slot = None;
            let mut state = self.state.lock();
            if outcome
                .as_ref()
                .is_err_and(|e| e.code == "REMOTE_CLIENT_RETIRE_UNKNOWN")
            {
                state.creation_unknown = true;
                state.closing = true;
            }
            state.async_creating = false;
            state.creating = false;
            state.creation_stop = None;
            state.creation_result = Some(outcome);
            self.creation_finished.send_replace(());
        }
        let state = self.state.lock();
        if state.creation_unknown {
            return Err(error("REMOTE_CLIENT_RETIRE_UNKNOWN"));
        }
        state.creation_result.clone().unwrap_or(Ok(()))
    }

    #[cfg(any(test, not(feature = "remote-host")))]
    pub(crate) fn start_exit_waiter(
        self: &Arc<Self>,
        code: i32,
        exit: impl FnOnce(i32) + Send + 'static,
    ) {
        let mut state = self.state.lock();
        if !matches!(state.exit,ExitIntent::Waiting(original) if original==code)
            || state.exit_waiter.is_some()
        {
            return;
        }
        let original = self.clone();
        state.exit_waiter = Some(tauri::async_runtime::spawn(async move {
            let _ = original.shutdown_and_exit(code, exit).await;
        }));
    }
    #[cfg(all(windows, test))]
    pub(crate) fn install_key_profile(
        &self,
        profile: Option<Arc<dyn NativeKeyProfile>>,
        purpose: SigningPurpose,
    ) -> Result<KeyRequestHandle, SafeError> {
        let profile =
            profile.unwrap_or_else(|| Arc::new(crate::native_crypto::signers::UnavailableProfile));
        self.install_key_profile_inner(profile, purpose, || {})
    }
    #[cfg(all(windows, test))]
    pub(crate) fn install_key_profile_for_test(
        &self,
        profile: Arc<dyn NativeKeyProfile>,
        purpose: SigningPurpose,
        before_activate: impl FnOnce(),
    ) -> Result<KeyRequestHandle, SafeError> {
        self.install_key_profile_inner(profile, purpose, before_activate)
    }
    #[cfg(all(windows, test))]
    fn install_key_profile_inner(
        &self,
        profile: Arc<dyn NativeKeyProfile>,
        purpose: SigningPurpose,
        before_activate: impl FnOnce(),
    ) -> Result<KeyRequestHandle, SafeError> {
        // Reuse the existing native creation reservation. Exit cannot pass an
        // unpublished key worker, and its original handle remains local until
        // registration or known no-key cleanup has completed.
        let mut reservation = self.begin_create()?;
        let spki = profile
            .binding(purpose)
            .map_err(|_| error("NATIVE_PROFILE_UNAVAILABLE"))?;
        let (handle, start) = self.prepare_key(&reservation, profile, purpose, spki)?;
        {
            let mut state = self.state.lock();
            state.creating = false;
            reservation.published = true;
        }
        self.creation_finished.send_replace(());
        before_activate();
        start.release();
        if self.state.lock().closing {
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        Ok(handle)
    }
    #[cfg(windows)]
    fn prepare_key(
        &self,
        reservation: &CreationGuard<'_>,
        profile: Arc<dyn NativeKeyProfile>,
        purpose: SigningPurpose,
        spki: Vec<u8>,
    ) -> Result<(KeyRequestHandle, KeyStart), SafeError> {
        if !std::ptr::eq(reservation.service, self) || reservation.published {
            return Err(error("FORBIDDEN"));
        }
        {
            let state = self.state.lock();
            if state.keys.len() >= 3
                || state.keys.iter().any(|key| {
                    let (old_purpose, old_spki) = key.signal.binding();
                    old_purpose == purpose || old_spki == spki
                })
            {
                return Err(error("NATIVE_KEY_ROLE_CONFLICT"));
            }
        }
        let (owner, handle, start) =
            KeyCustodian::prepare(move || profile.open(purpose), purpose, spki)
                .map_err(|_| error("NATIVE_KEY_UNAVAILABLE"))?;
        let slot = Arc::new(NativeKeySlot::new(owner, handle.clone()));
        let accepted = {
            let mut state = self.state.lock();
            if state.closing {
                false
            } else {
                state.keys.push(slot.clone());
                true
            }
        };
        if !accepted {
            // Unpublished cleanup remains covered by the original reservation.
            // No service/global lock is held while waking or joining.
            drop(start);
            handle.shutdown();
            drop(slot);
            drop(handle);
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        Ok((handle, start))
    }
    #[cfg(windows)]
    #[allow(
        dead_code,
        reason = "native provider requires explicitly approved profile and presence; no production credentials configured"
    )]
    pub(crate) fn native_signers(
        &self,
        profile: Option<Arc<dyn NativeKeyProfile>>,
        binding: NativeSignerBinding,
        presence: Option<Arc<dyn NativeGrantPresence>>,
    ) -> Result<NativeSignerSet, SafeError> {
        let profile = profile.ok_or_else(|| error("NATIVE_PROFILE_UNAVAILABLE"))?;
        let presence = presence.ok_or_else(|| error("NATIVE_PRESENCE_UNAVAILABLE"))?;
        let approval = binding
            .approved_until
            .ok_or_else(|| error("NATIVE_SIGNING_APPROVAL_MISSING"))?;
        if approval <= std::time::Instant::now() {
            return Err(error("NATIVE_SIGNING_APPROVAL_EXPIRED"));
        }
        if binding.relay_id == [0; 16]
            || binding.origin.as_str().len() > 512
            || binding
                .tls_end
                .checked_duration_since(std::time::Instant::now())
                .is_none_or(|d| d > Duration::from_secs(5))
        {
            return Err(error("NATIVE_BINDING_INVALID"));
        }
        let mut reservation = self.begin_create()?;
        if !self.state.lock().keys.is_empty() {
            return Err(error("NATIVE_SIGNERS_ALREADY_OWNED"));
        }
        let mut metadata = Vec::new();
        for purpose in [
            SigningPurpose::Endpoint,
            SigningPurpose::Relay,
            SigningPurpose::Issuer,
        ] {
            let spki = profile
                .binding(purpose)
                .map_err(|_| error("NATIVE_PROFILE_UNAVAILABLE"))?;
            cc_desk_remote_security::P256Spki::new(&spki)
                .map_err(|_| error("NATIVE_PROFILE_INVALID"))?;
            if metadata.iter().any(|(_, old)| old == &spki) {
                return Err(error("NATIVE_KEY_ROLE_CONFLICT"));
            }
            metadata.push((purpose, spki));
        }
        let mut handles = Vec::new();
        let mut starts = Vec::new();
        for (purpose, spki) in metadata {
            match self.prepare_key(&reservation, profile.clone(), purpose, spki) {
                Ok((handle, start)) => {
                    handles.push(handle);
                    starts.push(start);
                }
                Err(error) => {
                    for handle in &handles {
                        handle.shutdown();
                    }
                    drop(starts);
                    return Err(error);
                }
            }
        }
        let authority = NativeSignerAuthority::new(handles.clone(), &binding, presence, profile)
            .map_err(|_| error("NATIVE_SIGNING_APPROVAL_INVALID"))?;
        let result = authority
            .assemble(binding.tls_end)
            .map_err(|_| error("NATIVE_SIGNING_ATTEMPT_INVALID"));
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                for handle in &handles {
                    handle.shutdown();
                }
                drop(starts);
                return Err(error);
            }
        };
        let published = {
            let mut state = self.state.lock();
            if state.closing {
                false
            } else {
                state.signing = Some(authority);
                state.creating = false;
                reservation.published = true;
                true
            }
        };
        if !published {
            for handle in &handles {
                handle.shutdown();
            }
            drop(starts);
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        self.creation_finished.send_replace(());
        for start in starts {
            start.release();
        }
        if self.state.lock().closing {
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        Ok(result)
    }
    #[cfg(windows)]
    #[allow(
        dead_code,
        reason = "backend profile source and native presence remain unavailable"
    )]
    pub(crate) fn fresh_native_signers(
        &self,
        end: std::time::Instant,
    ) -> Result<NativeSignerSet, SafeError> {
        let mut reservation = self.begin_create()?;
        let (authority, keys) = {
            let state = self.state.lock();
            (
                state
                    .signing
                    .clone()
                    .ok_or_else(|| error("NATIVE_PROFILE_UNAVAILABLE"))?,
                state.keys.clone(),
            )
        };
        if keys
            .iter()
            .any(|key| key.observe() != KeyRetirement::Pending)
        {
            return Err(error("NATIVE_SIGNER_UNHEALTHY"));
        }
        let result = authority
            .assemble(end)
            .map_err(|_| error("NATIVE_SIGNER_BUSY_EXPIRED_OR_UNKNOWN"))?;
        {
            let mut state = self.state.lock();
            if state.closing {
                return Err(error("REMOTE_CLIENT_CLOSED"));
            }
            state.creating = false;
            reservation.published = true;
        }
        self.creation_finished.send_replace(());
        Ok(result)
    }
    fn signal_keys(&self) {
        #[cfg(windows)]
        {
            let keys = self.state.lock().keys.clone();
            for key in keys {
                key.signal.shutdown();
            }
        }
    }
    async fn join_keys(&self) -> Result<(), SafeError> {
        #[cfg(windows)]
        {
            let keys = self.state.lock().keys.clone();
            loop {
                let mut pending = false;
                for key in &keys {
                    match key.observe() {
                        KeyRetirement::Pending => pending = true,
                        KeyRetirement::Unknown => return Err(error("NATIVE_KEY_RETIRE_UNKNOWN")),
                        KeyRetirement::Retired | KeyRetirement::NoKeyCreated => (),
                    }
                }
                if !pending {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        Ok(())
    }
    #[cfg(all(windows, test))]
    pub(crate) fn test_key_observations(&self) -> Vec<KeyRetirement> {
        let keys = self.state.lock().keys.clone();
        keys.iter().map(|key| key.observe()).collect()
    }
    fn begin_create(&self) -> Result<CreationGuard<'_>, SafeError> {
        let mut state = self.state.lock();
        if state.closing {
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        if state.creating || state.async_creating || state.resource.is_some() {
            return Err(error("REMOTE_CLIENT_BUSY"));
        }
        let id = state
            .next_id
            .checked_add(1)
            .ok_or_else(|| error("REMOTE_CLIENT_FULL"))?;
        state.next_id = id;
        state.creating = true;
        Ok(CreationGuard {
            service: self,
            id,
            published: false,
        })
    }
    /// Native-only reservation winner. The callback must construct the client from
    /// its original granted channel; no IPC command exposes this factory.
    #[allow(dead_code)]
    pub(crate) fn install_native<F: FnOnce() -> Result<NativeClient, SafeError>>(
        &self,
        binding: Arc<DocumentBinding<NativeRun>>,
        webview: &Webview,
        headers: &tauri::http::HeaderMap,
        create: F,
    ) -> Result<u32, SafeError> {
        let caller = binding.admit_native(webview, headers)?;
        let mut reservation = self.begin_create()?;
        let id = reservation.id;
        let client = create()?;
        let result = self.publish_reserved(id, caller, &binding, client);
        self.state.lock().creating = false;
        reservation.published = true;
        self.creation_finished.send_replace(());
        result
    }
    fn publish_reserved(
        &self,
        id: u32,
        caller: CallerIdentity,
        binding: &Arc<DocumentBinding<NativeRun>>,
        client: NativeClient,
    ) -> Result<u32, SafeError> {
        let resource = OwnedClient::new(id, caller.clone(), binding, client);
        let closing = {
            let mut state = self.state.lock();
            state.resource = Some(resource.clone());
            state.closing
        };
        if closing {
            resource.retire();
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        let weak = Arc::downgrade(&resource);
        let registered = binding.on_revoke(
            &caller,
            Arc::new(move || {
                if let Some(resource) = weak.upgrade() {
                    resource.retire();
                }
            }),
        );
        match registered {
            Ok(ticket) => *resource.ticket.lock() = Some(ticket),
            Err(failure) => {
                resource.retire();
                return Err(failure);
            }
        }
        if self.state.lock().closing {
            resource.retire();
            return Err(error("REMOTE_CLIENT_CLOSED"));
        }
        if let Err(failure) = resource.check_private() {
            resource.retire();
            return Err(failure);
        }
        resource.published.store(true, Ordering::Release);
        Ok(id)
    }
    fn get(
        &self,
        id: u32,
        caller: &CallerIdentity,
        binding: &Arc<DocumentBinding<NativeRun>>,
    ) -> Result<Arc<OwnedClient>, SafeError> {
        let resource = {
            let state = self.state.lock();
            if state.closing {
                return Err(error("REMOTE_CLIENT_CLOSED"));
            }
            state
                .resource
                .clone()
                .ok_or_else(|| error("REMOTE_CLIENT_UNAVAILABLE"))?
        };
        if resource.id != id
            || resource.caller != *caller
            || !resource
                .binding
                .upgrade()
                .is_some_and(|original| Arc::ptr_eq(&original, binding))
        {
            return Err(error("FORBIDDEN"));
        }
        binding.check_admitted(caller)?;
        resource.check()?;
        Ok(resource)
    }
    /// Seal native admission atomically with observing any original owner.
    /// Retirement notification runs outside the state lock.
    pub(crate) fn begin_shutdown(&self) -> bool {
        let (waiting, resource, stop) = {
            let mut state = self.state.lock();
            state.closing = true;
            (
                state.creating
                    || state.async_creating
                    || state.creation_unknown
                    || state.resource.is_some()
                    || state.has_keys(),
                state.resource.clone(),
                state.creation_stop.clone(),
            )
        };
        self.signal_keys();
        if let Some(stop) = stop {
            stop.send_replace(true);
        }
        if let Some(resource) = resource {
            resource.retire();
        }
        waiting
    }
    pub(crate) fn request_close_all(&self) {
        self.begin_shutdown();
    }
    pub(crate) async fn shutdown(&self) -> Result<(), SafeError> {
        // Subscribe before checking state so publication/drop cannot be lost.
        let mut finished = self.creation_finished.subscribe();
        self.begin_shutdown();
        let creation = self.state.lock().creation_id;
        if let Some(id) = creation {
            let _ = self.wait_creation(id).await;
        }
        let resource = loop {
            {
                let state = self.state.lock();
                if state.creation_unknown {
                    return Err(error("REMOTE_CLIENT_RETIRE_UNKNOWN"));
                }
                if !state.creating {
                    break state.resource.clone();
                }
            }
            // A synchronous native provider cannot be preempted. This single
            // tracked exit waiter reports no retirement until its original
            // reservation reaches a known terminal state.
            finished
                .changed()
                .await
                .map_err(|_| error("REMOTE_CLIENT_RETIRE_UNKNOWN"))?;
        };
        if let Some(resource) = resource {
            resource.close_join().await?;
            let mut state = self.state.lock();
            if state
                .resource
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &resource))
            {
                state.resource = None;
            }
        }
        self.join_keys().await?;
        Ok(())
    }
    /// Preserve the first exit intent even between creation completion and
    /// scheduling its sole waiter. Ready permits only that original code.
    #[cfg(any(test, not(feature = "remote-host")))]
    pub(crate) fn request_exit(&self, code: i32) -> ExitDecision {
        let (decision, resource) = {
            let mut state = self.state.lock();
            state.closing = true;
            let decision = match state.exit {
                ExitIntent::Idle if state.creation_unknown => {
                    state.exit = ExitIntent::Unknown;
                    ExitDecision::Prevent
                }
                ExitIntent::Idle
                    if state.creating
                        || state.async_creating
                        || state.resource.is_some()
                        || state.has_keys() =>
                {
                    state.exit = ExitIntent::Waiting(code);
                    ExitDecision::Wait(code)
                }
                ExitIntent::Idle => {
                    state.exit = ExitIntent::Ready(code);
                    ExitDecision::Proceed
                }
                ExitIntent::Ready(original) if code == original => ExitDecision::Proceed,
                ExitIntent::Waiting(_) | ExitIntent::Ready(_) | ExitIntent::Unknown => {
                    ExitDecision::Prevent
                }
            };
            (decision, state.resource.clone())
        };
        self.signal_keys();
        if let Some(resource) = resource {
            resource.retire();
        }
        decision
    }
    #[cfg(any(test, not(feature = "remote-host")))]
    pub(crate) async fn shutdown_and_exit(
        &self,
        code: i32,
        exit: impl FnOnce(i32),
    ) -> Result<(), SafeError> {
        if !matches!(self.state.lock().exit, ExitIntent::Waiting(original) if code == original) {
            return Err(error("FORBIDDEN"));
        }
        if let Err(failure) = self.shutdown().await {
            self.state.lock().exit = ExitIntent::Unknown;
            return Err(failure);
        }
        {
            let mut state = self.state.lock();
            if !matches!(state.exit, ExitIntent::Waiting(original) if code == original) {
                return Err(error("FORBIDDEN"));
            }
            state.exit = ExitIntent::Ready(code);
        }
        exit(code);
        Ok(())
    }
    async fn close_resource(&self, resource: Arc<OwnedClient>) -> Result<(), SafeError> {
        resource.close_join().await?;
        let mut state = self.state.lock();
        if state
            .resource
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &resource))
        {
            state.resource = None;
        }
        Ok(())
    }
}
impl Drop for RemoteClientService {
    fn drop(&mut self) {
        self.request_close_all();
    }
}
fn admission(
    runtime: &NativeRuntime,
    webview: &Webview,
    request: &Request<'_>,
) -> Result<(Arc<DocumentBinding<NativeRun>>, CallerIdentity), SafeError> {
    let binding = runtime.binding()?;
    let caller = binding.admit_native(webview, request.headers())?;
    Ok((binding, caller))
}
#[tauri::command]
pub(crate) async fn remote_client_exchange(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    service: State<'_, Arc<RemoteClientService>>,
) -> Result<ByteReply, SafeError> {
    let (binding, caller) = admission(&runtime, &webview, &request)?;
    let (id, payload, budget) = request_body(&request)?;
    let resource = service.get(id, &caller, &binding)?;
    resource.check()?;
    let bytes = resource
        .client
        .exchange(&payload, budget)
        .await
        .map_err(|_| error("REMOTE_CLIENT_OUTCOME_UNKNOWN"))?;
    resource.check()?;
    binding.check_admitted(&caller)?;
    if bytes.len() > MAX_PAYLOAD {
        return Err(error("REMOTE_CLIENT_BOUNDS"));
    }
    Ok(ByteReply {
        payload: URL_SAFE_NO_PAD.encode(bytes),
    })
}
fn request_body(request: &Request<'_>) -> Result<(u32, Vec<u8>, Duration), SafeError> {
    self::request(request.body())
}
#[tauri::command]
pub(crate) async fn remote_client_snapshot(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    service: State<'_, Arc<RemoteClientService>>,
) -> Result<SnapshotReply, SafeError> {
    let (binding, caller) = admission(&runtime, &webview, &request)?;
    let (id, payload, budget) = request_body(&request)?;
    let resource = service.get(id, &caller, &binding)?;
    resource.check()?;
    let batch = resource
        .client
        .snapshot(&payload, budget)
        .await
        .map_err(|_| error("REMOTE_CLIENT_OUTCOME_UNKNOWN"))?;
    resource.check()?;
    binding.check_admitted(&caller)?;
    if batch.parts.is_empty() || batch.parts.len() > 24000 {
        return Err(error("REMOTE_CLIENT_BOUNDS"));
    }
    let mut total = 0usize;
    for part in &batch.parts {
        total = total
            .checked_add(part.len())
            .ok_or_else(|| error("REMOTE_CLIENT_BOUNDS"))?;
        if part.len() > 32768 || total > 4 * 1024 * 1024 {
            return Err(error("REMOTE_CLIENT_BOUNDS"));
        }
    }
    let reply = SnapshotReply {
        stamp: serde_json::to_value(batch.stamp).map_err(|_| error("REMOTE_CLIENT_BOUNDS"))?,
        parts: batch
            .parts
            .into_iter()
            .map(|part| URL_SAFE_NO_PAD.encode(part))
            .collect(),
    };
    resource.check()?;
    binding.check_admitted(&caller)?;
    Ok(reply)
}
#[tauri::command]
pub(crate) async fn remote_client_close(
    webview: Webview,
    request: Request<'_>,
    runtime: State<'_, Arc<NativeRuntime>>,
    service: State<'_, Arc<RemoteClientService>>,
) -> Result<(), SafeError> {
    let (binding, caller) = admission(&runtime, &webview, &request)?;
    let close: CloseRequest = decode(request.body(), 128)?;
    let resource = service.get(close.resource_id, &caller, &binding)?;
    service.close_resource(resource).await?;
    binding.check_admitted(&caller)
}
#[cfg(test)]
use crate::tests::remote_channels as current_test_channels;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipc_requests_are_strict_and_bounded() {
        for body in [
            br#"{"resourceId":1,"budgetMs":0,"payload":"AQ"}"#.as_slice(),
            br#"{"resourceId":1,"budgetMs":1,"payload":"AQ=="}"#,
            br#"{"resourceId":1,"resourceId":2,"budgetMs":1,"payload":"AQ"}"#,
            br#"{"resourceId":1,"budgetMs":1,"payload":"AQ","extra":0}"#,
        ] {
            assert!(request(&InvokeBody::Raw(body.to_vec())).is_err());
        }
        assert!(request(&InvokeBody::Raw(vec![b' '; MAX_REQUEST + 1])).is_err());
        let (id, b, budget) = request(&InvokeBody::Raw(
            br#"{"resourceId":1,"budgetMs":1000,"payload":"AQID"}"#.to_vec(),
        ))
        .unwrap();
        assert_eq!(id, 1);
        assert_eq!(b, [1, 2, 3]);
        assert_eq!(budget, Duration::from_secs(1));
    }
}

#[cfg(test)]
mod creation_tests {
    use super::*;
    #[tokio::test]
    async fn shutdown_cannot_report_retired_during_native_creation() {
        let service = RemoteClientService::default();
        service.state.lock().creating = true;
        assert!(
            tokio::time::timeout(Duration::from_millis(30), service.shutdown())
                .await
                .is_err()
        );
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    #[tokio::test]
    async fn shutdown_closes_admission_while_original_native_creation_is_in_flight() {
        let service = Arc::new(RemoteClientService::default());
        let worker = service.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _reservation = worker.begin_create().unwrap();
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let shutdown = service.shutdown();
        tokio::pin!(shutdown);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), &mut shutdown)
                .await
                .is_err()
        );
        assert!(service.begin_create().is_err());
        release_tx.send(()).unwrap();
        thread.join().unwrap();
        tokio::time::timeout(Duration::from_secs(1), shutdown)
            .await
            .unwrap()
            .unwrap();
        assert!(!service.state.lock().creating);
        assert!(service.begin_create().is_err());
    }
    #[test]
    fn native_factory_panic_clears_reservation_but_never_reopens_unknown_service() {
        let service = RemoteClientService::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _reservation = service.begin_create().unwrap();
            panic!("TEST_ONLY factory panic");
        }));
        assert!(result.is_err());
        assert!(!service.state.lock().creating);
        assert!(service.begin_create().is_err());
    }
    #[test]
    fn aborted_native_reservations_never_reuse_resource_ids() {
        let service = RemoteClientService::default();
        let first = service.begin_create().unwrap();
        let old = first.id;
        drop(first);
        let next = service.begin_create().unwrap();
        assert!(next.id > old);
    }
}

#[cfg(test)]
mod exit_regression_tests {
    use super::*;
    #[test]
    fn idle_exit_observation_must_seal_native_admission() {
        let service = RemoteClientService::default();
        assert!(!service.begin_shutdown());
        assert!(service.begin_create().is_err());
    }
    #[tokio::test]
    async fn factory_panic_cannot_be_reported_as_joined_shutdown() {
        let service = RemoteClientService::default();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _reservation = service.begin_create().unwrap();
            panic!("TEST_ONLY unknown factory result");
        }));
        assert!(service.shutdown().await.is_err());
    }
    #[tokio::test]
    async fn original_exit_waiter_keeps_its_code_until_reserved_creation_finishes() {
        let service = Arc::new(RemoteClientService::default());
        let worker = service.clone();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _reservation = worker.begin_create().unwrap();
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(service.request_exit(17), ExitDecision::Wait(17));
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let actual_code = std::sync::atomic::AtomicI32::new(0);
        let wait = service.shutdown_and_exit(17, |code| {
            actual_code.store(code, Ordering::Release);
            calls.fetch_add(1, Ordering::AcqRel);
        });
        tokio::pin!(wait);
        let pending = tokio::time::timeout(Duration::from_millis(30), &mut wait).await;
        assert_eq!(calls.load(Ordering::Acquire), 0);
        release_tx.send(()).unwrap();
        thread.join().unwrap();
        assert!(
            pending.is_err(),
            "exit must wait for the original creation terminal state"
        );
        tokio::time::timeout(Duration::from_secs(1), wait)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual_code.load(Ordering::Acquire), 17);
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(service.begin_create().is_err());
    }
}

#[cfg(test)]
mod queued_exit_regression {
    use super::*;
    #[tokio::test]
    async fn completed_creation_cannot_bypass_an_original_pending_exit_waiter() {
        let service = RemoteClientService::default();
        let reserved = service.begin_create().unwrap();
        assert_eq!(service.request_exit(17), ExitDecision::Wait(17));
        assert_eq!(service.request_exit(0), ExitDecision::Prevent);
        drop(reserved);
        // The completion notification is now queued; the waiter has not run.
        assert_eq!(service.request_exit(0), ExitDecision::Prevent);
        assert_eq!(service.request_exit(17), ExitDecision::Prevent);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        service
            .shutdown_and_exit(17, |code| {
                assert_eq!(code, 17);
                assert_eq!(service.request_exit(code), ExitDecision::Proceed);
                assert_eq!(service.request_exit(0), ExitDecision::Prevent);
                calls.fetch_add(1, Ordering::AcqRel);
            })
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(service
            .shutdown_and_exit(17, |_| panic!("duplicate exit callback"))
            .await
            .is_err());
        assert!(service.begin_create().is_err());
    }
    #[test]
    fn idle_exit_closes_admission_and_preserves_first_code() {
        let service = RemoteClientService::default();
        assert_eq!(service.request_exit(17), ExitDecision::Proceed);
        assert_eq!(service.request_exit(0), ExitDecision::Prevent);
        assert_eq!(service.request_exit(17), ExitDecision::Proceed);
        assert!(service.begin_create().is_err());
    }
    #[tokio::test]
    async fn unknown_factory_result_never_emits_or_restarts_exit() {
        let service = RemoteClientService::default();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _reservation = service.begin_create().unwrap();
            assert_eq!(service.request_exit(17), ExitDecision::Wait(17));
            panic!("TEST_ONLY unknown factory result");
        }));
        assert!(service
            .shutdown_and_exit(17, |_| panic!("unknown exit callback"))
            .await
            .is_err());
        assert_eq!(service.request_exit(17), ExitDecision::Prevent);
        assert_eq!(service.request_exit(0), ExitDecision::Prevent);
        assert!(service.begin_create().is_err());
    }
}
