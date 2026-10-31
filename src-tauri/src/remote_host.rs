//! Default-off application custody. No JS owner/identity/credential factory.

use crate::cli::native_runtime::{HostNativeObservation, HostNativeShutdown, NativeRuntime};
use crate::remote_client_ipc::RemoteClientService;
use crate::run_supervisor::NativeRunPin;
use cc_desk_remote_host::native_host::{NativeHost, WriterRetirement};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;

/// Both desktop close and explicit application exit use this same gate.
pub(crate) fn request_app_exit(
    app: &tauri::AppHandle,
    code: i32,
    native: Arc<NativeRuntime>,
) -> bool {
    let service = app.state::<Arc<RemoteHostService>>().inner().clone();
    match service.request_exit(code, false) {
        ExitDecision::Proceed => false,
        ExitDecision::Prevent => true,
        ExitDecision::Wait(original) => {
            let client = app.state::<Arc<RemoteClientService>>().inner().clone();
            let app = app.clone();
            service.start_exit(original, native, client, move |code| app.exit(code));
            true
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExitDecision {
    Proceed,
    Wait(i32),
    Prevent,
}
#[derive(Clone, Copy)]
enum ExitIntent {
    Idle,
    Waiting(i32),
    Ready(i32),
    Unknown,
}
struct ServiceState {
    // Locked/off: no JS registration or production provider factory is exposed.
    host: Option<NativeHost>,
    pins: Vec<NativeRunPin>,
    native: Option<HostNativeShutdown>,
    closing: bool,
    retired: bool,
    exit: ExitIntent,
    waiter: Option<tauri::async_runtime::JoinHandle<()>>,
}
pub(crate) struct RemoteHostService {
    state: Mutex<ServiceState>,
}
impl Default for RemoteHostService {
    fn default() -> Self {
        Self {
            state: Mutex::new(ServiceState {
                host: None,
                pins: Vec::new(),
                native: None,
                closing: false,
                retired: false,
                exit: ExitIntent::Idle,
                waiter: None,
            }),
        }
    }
}
impl RemoteHostService {
    // Native provider entry. It accepts only an opaque original document/run
    // admission and a real verified channel, never a shutdown pin or JS authority.
    #[allow(
        dead_code,
        reason = "production native provider/identity bootstrap remains Locked"
    )]
    pub(crate) fn adopt_native(
        &self,
        runtime: &NativeRuntime,
        webview: &tauri::Webview,
        headers: &tauri::http::HeaderMap,
        body: &tauri::ipc::InvokeBody,
        channel: Arc<cc_desk_remote_host::native_host::NativeChannel>,
    ) -> Result<
        (),
        Box<(
            crate::cli::types::SafeError,
            Option<crate::run_supervisor::NativeRunAdmission>,
        )>,
    > {
        let admission = runtime
            .admit_remote_run(webview, headers, body)
            .map_err(|error| Box::new((error, None)))?;
        self.adopt_admitted(admission, channel, runtime)
            .map_err(|rejected| {
                let (error, original) = *rejected;
                Box::new((error, Some(original)))
            })
    }
    fn adopt_admitted(
        &self,
        admission: crate::run_supervisor::NativeRunAdmission,
        channel: Arc<cc_desk_remote_host::native_host::NativeChannel>,
        runtime: &NativeRuntime,
    ) -> Result<
        (),
        Box<(
            crate::cli::types::SafeError,
            crate::run_supervisor::NativeRunAdmission,
        )>,
    > {
        let mut state = self.state.lock();
        if state.closing || !state.pins.is_empty() || !admission.is_owned_by(runtime) {
            return Err(Box::new((
                crate::cli::profiles::error("REMOTE_HOST_CLOSED"),
                admission,
            )));
        }
        if state.host.is_none() {
            let host = match admission.create_host(channel.clone()) {
                Ok(host) => host,
                Err(error) => return Err(Box::new((error, admission))),
            };
            // Keep this SAME permanent-admission Host even if the following
            // handoff rejects. Never drop/recreate it on a wrong-resource retry.
            state.host = Some(host);
        }
        let pin = admission.adopt(
            state.host.as_mut().expect("retained original Host"),
            channel,
        )?;
        // Successful admission consumption drops its extra RunAccess/original Arc;
        // shutdown retains only the original lifetime pin and Host-owned pending.
        state.pins.push(pin);
        Ok(())
    }
    pub(crate) fn request_exit(&self, code: i32, maintenance_blocked: bool) -> ExitDecision {
        // A maintenance refusal cannot seal admission or replace the first code.
        if maintenance_blocked {
            return ExitDecision::Prevent;
        }
        let mut state = self.state.lock();
        state.closing = true;
        match state.exit {
            ExitIntent::Idle => {
                state.exit = ExitIntent::Waiting(code);
                ExitDecision::Wait(code)
            }
            ExitIntent::Ready(original) if original == code => ExitDecision::Proceed,
            _ => ExitDecision::Prevent,
        }
    }
    fn complete_exit(&self, code: i32) -> bool {
        let mut state = self.state.lock();
        if !state.retired || !matches!(state.exit,ExitIntent::Waiting(original) if original==code) {
            return false;
        }
        state.exit = ExitIntent::Ready(code);
        true
    }
    fn mark_unknown(&self) {
        self.state.lock().exit = ExitIntent::Unknown;
    }
    pub(crate) fn start_exit(
        self: &Arc<Self>,
        code: i32,
        native: Arc<NativeRuntime>,
        client: Arc<RemoteClientService>,
        exit: impl FnOnce(i32) + Send + 'static,
    ) {
        let mut state = self.state.lock();
        if !matches!(state.exit,ExitIntent::Waiting(original) if original==code)
            || state.waiter.is_some()
        {
            return;
        }
        // All stops are signalled before awaiting a pending install or network IO.
        let networks = state.host.as_mut().and_then(|host| {
            host.begin_shutdown();
            host.network_retirement()
        });
        client.begin_shutdown();
        let pins = std::mem::take(&mut state.pins);
        state.native = Some(native.begin_host_shutdown(pins));
        state.waiter = Some(self.spawn_waiter(code, client, networks, exit));
    }
    fn spawn_waiter(
        self: &Arc<Self>,
        code: i32,
        client: Arc<RemoteClientService>,
        networks: Option<cc_desk_remote_host::native_host::NetworkRetirement>,
        exit: impl FnOnce(i32) + Send + 'static,
    ) -> tauri::async_runtime::JoinHandle<()> {
        let original = self.clone();
        tauri::async_runtime::spawn(async move {
            let (client_result, (), owned_known) = tokio::join!(
                client.shutdown(),
                async move {
                    if let Some(networks) = networks {
                        networks.wait().await;
                    }
                },
                original.observe_owned()
            );
            if client_result.is_err() || !owned_known {
                original.mark_unknown();
                return;
            }
            original.state.lock().retired = true;
            if original.complete_exit(code) {
                exit(code);
            }
        })
    }
    async fn observe_owned(&self) -> bool {
        loop {
            let (pending, unknown) = {
                let mut state = self.state.lock();
                let writer = state
                    .host
                    .as_mut()
                    .map(|host| host.observe_shutdown())
                    .unwrap_or(WriterRetirement::Joined);
                let native = state
                    .native
                    .as_mut()
                    .map(|native| native.observe())
                    .unwrap_or(HostNativeObservation::Unknown);
                (
                    writer == WriterRetirement::Pending || native == HostNativeObservation::Pending,
                    writer == WriterRetirement::Unknown || native == HostNativeObservation::Unknown,
                )
            };
            if pending {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            return !unknown;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exit_keeps_first_code_and_maintenance_refusal_does_not_seal() {
        let service = RemoteHostService::default();
        assert_eq!(service.request_exit(7, true), ExitDecision::Prevent);
        assert!(!service.state.lock().closing);
        assert_eq!(service.request_exit(17, false), ExitDecision::Wait(17));
        assert_eq!(service.request_exit(0, false), ExitDecision::Prevent);
        assert!(!service.complete_exit(0));
        assert!(
            !service.complete_exit(17),
            "an exit intent is not retirement evidence"
        );
        service.state.lock().retired = true;
        assert!(service.complete_exit(17));
        assert_eq!(service.request_exit(0, false), ExitDecision::Prevent);
        assert_eq!(service.request_exit(17, false), ExitDecision::Proceed);
    }
    #[test]
    fn unknown_exit_never_becomes_ready_or_reopens_admission() {
        let service = RemoteHostService::default();
        assert_eq!(service.request_exit(4, false), ExitDecision::Wait(4));
        service.mark_unknown();
        assert!(!service.complete_exit(4));
        assert_eq!(service.request_exit(4, false), ExitDecision::Prevent);
        assert!(service.state.lock().closing);
    }

    #[tokio::test]
    async fn locked_host_exit_joins_original_work_and_publishes_only_first_code_once() {
        use crate::cli::{launch_service::LaunchService, storage::WorkspaceRepository};
        let dir = tempfile::tempdir().unwrap();
        let launch = Arc::new(LaunchService::new(
            WorkspaceRepository::open(dir.path().join("workspace.json")).unwrap(),
            Some(Default::default()),
            None,
        ));
        let native = Arc::new(NativeRuntime::new(launch));
        let client = Arc::new(RemoteClientService::default());
        let service = Arc::new(RemoteHostService::default());
        assert_eq!(service.request_exit(17, false), ExitDecision::Wait(17));
        let (tx, rx) = tokio::sync::oneshot::channel();
        service.start_exit(17, native.clone(), client.clone(), move |code| {
            let _ = tx.send(code);
        });
        service.start_exit(17, native, client, |_| panic!("duplicate waiter"));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), rx)
                .await
                .unwrap()
                .unwrap(),
            17
        );
        let worker = service
            .state
            .lock()
            .waiter
            .take()
            .expect("original waiter retained");
        worker.await.unwrap();
        assert_eq!(service.request_exit(0, false), ExitDecision::Prevent);
        assert_eq!(service.request_exit(17, false), ExitDecision::Proceed);
        assert!(service.state.lock().retired);
    }

    #[tokio::test]
    async fn cancelling_observation_cannot_release_original_held_shutdown_worker() {
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, release_rx) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        });
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        let service = Arc::new(RemoteHostService::default());
        assert_eq!(service.request_exit(29, false), ExitDecision::Wait(29));
        service.state.lock().native = Some(HostNativeShutdown::from_owned_worker_for_test(worker));
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let wait = service.spawn_waiter(
            29,
            Arc::new(RemoteClientService::default()),
            None,
            move |code| {
                let _ = tx.send(code);
            },
        );
        service.state.lock().waiter = Some(wait);
        assert!(tokio::time::timeout(Duration::from_millis(30), &mut rx)
            .await
            .is_err());
        assert!(!service.state.lock().retired);
        assert!(service.state.lock().waiter.is_some());
        assert_eq!(service.request_exit(29, false), ExitDecision::Prevent);
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), rx)
                .await
                .unwrap()
                .unwrap(),
            29
        );
        let wait = service.state.lock().waiter.take().unwrap();
        wait.await.unwrap();
        assert_eq!(service.request_exit(29, false), ExitDecision::Proceed);
    }

    #[tokio::test]
    async fn owned_worker_panic_does_not_publish_retirement() {
        let worker = std::thread::spawn(|| panic!("TEST_ONLY_SHUTDOWN_PANIC"));
        let service = Arc::new(RemoteHostService::default());
        assert_eq!(service.request_exit(31, false), ExitDecision::Wait(31));
        service.state.lock().native = Some(HostNativeShutdown::from_owned_worker_for_test(worker));
        let (tx, rx) = tokio::sync::oneshot::channel();
        let wait = service.spawn_waiter(
            31,
            Arc::new(RemoteClientService::default()),
            None,
            move |_| {
                let _ = tx.send(());
            },
        );
        service.state.lock().waiter = Some(wait);
        assert!(tokio::time::timeout(Duration::from_secs(1), rx)
            .await
            .unwrap()
            .is_err());
        let wait = service.state.lock().waiter.take().unwrap();
        wait.await.unwrap();
        assert_eq!(service.request_exit(31, false), ExitDecision::Prevent);
        assert!(!service.state.lock().retired);
    }
}
