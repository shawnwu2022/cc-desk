//! Default-off application custody. No JS owner/identity/credential factory.

use crate::cli::native_runtime::{HostNativeObservation, HostNativeShutdown, NativeRuntime};
use crate::remote_client_ipc::RemoteClientService;
#[path = "remote_host_activation.rs"]
pub(crate) mod activation;
#[path = "remote_host_driver.rs"]
pub(crate) mod driver;
use crate::run_supervisor::NativeRunPin;
use activation::{ActivationOperation, ActivationReservation};
#[path = "remote_host/delegated.rs"]
pub(crate) mod delegated;
#[cfg(windows)]
#[path = "remote_host_unattended.rs"]
pub(crate) mod unattended;
use cc_desk_remote_host::native_host::{NativeHost, WriterRetirement};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};
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
    begin_app_shutdown(
        &service,
        app.state::<Arc<crate::remote_host_controller::RemoteHostController>>()
            .inner(),
    );
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

// Tauri 已通过原 handoff 拒绝门；先关闭委托，再进入旧 controller 的状态锁。
pub(crate) fn begin_app_shutdown(
    service: &RemoteHostService,
    controller: &crate::remote_host_controller::RemoteHostController,
) {
    #[cfg(windows)]
    service.unattended.request_close();
    controller.begin_shutdown();
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
/// Original per-channel task custody. Cancelling a join observer leaves the
/// original JoinHandle in its slot; a panic remains Unknown rather than retired.
struct PeerPump {
    join: tokio::sync::Mutex<Option<tauri::async_runtime::JoinHandle<Result<(), ()>>>>,
    joined: AtomicU8,
}
impl PeerPump {
    fn known_joined(&self) -> bool {
        // 只读真实 join 缓存；Service 锁内只 try_lock，竞争时保留，不跨 await。
        self.joined.load(Ordering::Acquire) == 1
            && self.join.try_lock().is_ok_and(|slot| slot.is_none())
    }
    async fn join(&self) -> bool {
        let mut slot = self.join.lock().await;
        if let Some(worker) = slot.as_mut() {
            let success = matches!(worker.await, Ok(Ok(())));
            *slot = None;
            self.joined
                .store(if success { 1 } else { 2 }, Ordering::Release);
        }
        self.joined.load(Ordering::Acquire) == 1
    }
}
enum HostSlot {
    Empty,
    Available(Box<NativeHost>),
    CheckedOut(u64),
}
/// reservation 只保护原 Host 的归属；任何 Host 回调都在 service 锁外。
struct HostCheckout<'a> {
    service: &'a RemoteHostService,
    token: u64,
    host: Option<Box<NativeHost>>,
    origin: Option<crate::run_supervisor::NativeRunNetworkOrigin>,
    stopped: bool,
    finished: bool,
    #[cfg(test)]
    before_return: Option<Box<dyn FnOnce() + Send>>,
}
impl HostCheckout<'_> {
    fn finish(mut self) {
        #[cfg(test)]
        if let Some(before_return) = self.before_return.take() {
            before_return();
        }
        loop {
            let mut state = self.service.state.lock();
            if state.closing && self.host.is_some() && !self.stopped {
                drop(state);
                self.stop();
                continue;
            }
            assert!(matches!(state.host, HostSlot::CheckedOut(token) if token == self.token));
            state.host = self
                .host
                .take()
                .map_or(HostSlot::Empty, HostSlot::Available);
            state.host_stopped = self.stopped;
            self.finished = true;
            break;
        }
    }
    fn stop(&mut self) {
        if !self.stopped {
            if let Some(host) = &mut self.host {
                host.begin_shutdown();
                self.stopped = true;
            }
        }
    }
}
impl Drop for HostCheckout<'_> {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self.service.state.lock();
        assert!(matches!(state.host, HostSlot::CheckedOut(token) if token == self.token));
        state.host = self
            .host
            .take()
            .map_or(HostSlot::Empty, HostSlot::Available);
        state.host_stopped = self.stopped;
        if !self.finished {
            state.closing = true;
            state.exit = ExitIntent::Unknown;
            // 上游可能仍在展开栈；这里只同步记录 Unknown，不调用任何 Waker。
            self.service.network.latch_unknown();
        }
    }
}
struct NativeShutdownCheckout<'a> {
    service: &'a RemoteHostService,
    owner: Option<HostNativeShutdown>,
    finished: bool,
}
impl Drop for NativeShutdownCheckout<'_> {
    fn drop(&mut self) {
        let mut state = self.service.state.lock();
        state.native = self.owner.take();
        if !self.finished {
            state.closing = true;
            state.exit = ExitIntent::Unknown;
            self.service.network.latch_unknown();
        }
    }
}
pub(crate) struct ServiceState {
    // Locked/off: no JS registration or production provider factory is exposed.
    host: HostSlot,
    pub(crate) delegation: Option<Arc<()>>,
    pub(crate) one_shot_claimed: bool,
    host_stopped: bool,
    next_host_checkout: u64,
    origin: Option<crate::run_supervisor::NativeRunNetworkOrigin>,
    pins: Vec<NativeRunPin>,
    native: Option<HostNativeShutdown>,
    pub(crate) closing: bool,
    pub(crate) source_slot: crate::remote_host_network::HostSourceSlot,
    #[cfg(test)]
    pub(crate) source_wait_pending: bool,
    retired: bool,
    activation: Option<Arc<ActivationOperation>>,
    driver: Option<driver::DriverLink>,
    #[cfg(test)]
    activation_waiting: bool,
    #[cfg(test)]
    native_observed_terminal: bool,
    exit: ExitIntent,
    waiter: Option<tauri::async_runtime::JoinHandle<()>>,
    waiter_starting: bool,
    peer_pumps: Vec<Arc<PeerPump>>,
    #[cfg(test)]
    after_handoff: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    retirement_gate: Option<(
        std::sync::mpsc::Sender<()>,
        tokio::sync::oneshot::Receiver<()>,
    )>,
}
pub(crate) struct RemoteHostService {
    #[cfg(windows)]
    pub(crate) unattended: unattended::UnattendedCustody,
    pub(crate) state: Mutex<ServiceState>,
    pub(crate) network: Arc<crate::remote_host_network::HostNetworkCustody>,
    #[cfg(windows)]
    pub(crate) registration: Arc<crate::remote_host_controller::startup::RegistrationCustody>,
}
impl Default for RemoteHostService {
    fn default() -> Self {
        Self {
            #[cfg(windows)]
            unattended: Default::default(),
            network: Arc::new(Default::default()),
            #[cfg(windows)]
            registration: Arc::new(Default::default()),
            state: Mutex::new(ServiceState {
                host: HostSlot::Empty,
                delegation: None,
                one_shot_claimed: false,
                host_stopped: false,
                next_host_checkout: 0,
                origin: None,
                pins: Vec::new(),
                native: None,
                closing: false,
                source_slot: Default::default(),
                #[cfg(test)]
                source_wait_pending: false,
                retired: false,
                activation: None,
                driver: None,
                #[cfg(test)]
                activation_waiting: false,
                #[cfg(test)]
                native_observed_terminal: false,
                exit: ExitIntent::Idle,
                waiter: None,
                waiter_starting: false,
                peer_pumps: Vec::new(),
                #[cfg(test)]
                after_handoff: None,
                #[cfg(test)]
                retirement_gate: None,
            }),
        }
    }
}
impl RemoteHostService {
    #[allow(dead_code, reason = "native activation supplier is not installed")]
    pub(crate) fn reserve_activation(
        &self,
    ) -> Result<ActivationReservation, crate::cli::types::SafeError> {
        let mut state = self.state.lock();
        if state.closing
            || state.retired
            || state.activation.is_some()
            || state.delegation.is_some()
        {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        state.one_shot_claimed = true;
        let reservation = ActivationReservation::new();
        state.activation = Some(reservation.operation());
        Ok(reservation)
    }
    #[cfg(windows)]
    pub(crate) fn reserve_driven_activation(
        &self,
    ) -> Result<
        (
            ActivationReservation,
            driver::DriverOwner,
            driver::DriverStart,
        ),
        crate::cli::types::SafeError,
    > {
        let mut state = self.state.lock();
        if state.closing
            || state.retired
            || state.activation.is_some()
            || state.delegation.is_some()
        {
            return Err(crate::cli::profiles::error("REMOTE_HOST_LOCKED"));
        }
        state.one_shot_claimed = true;
        let reservation = ActivationReservation::new();
        let (owner, start) = driver::DriverOwner::new(reservation.operation());
        state.driver = Some(owner.link());
        state.activation = Some(reservation.operation());
        Ok((reservation, owner, start))
    }
    fn activation_retired(state: &ServiceState) -> bool {
        state
            .driver
            .as_ref()
            .is_none_or(|driver| driver.snapshot().retired())
            && state
                .activation
                .as_ref()
                .is_none_or(|operation| operation.snapshot().retired())
    }
    #[cfg(test)]
    pub(crate) fn test_activation_observation(&self) -> Option<activation::ActivationObservation> {
        self.state
            .lock()
            .activation
            .as_ref()
            .map(|operation| operation.snapshot())
    }
    #[cfg(test)]
    pub(crate) fn test_activation_exit_observed(&self) -> bool {
        self.state.lock().activation_waiting
    }
    #[cfg(test)]
    pub(crate) async fn test_retire_activation(&self) -> bool {
        self.wait_activation().await
    }
    async fn wait_activation(&self) -> bool {
        loop {
            #[cfg(test)]
            {
                self.state.lock().activation_waiting = true;
            }
            let (operation, driver) = {
                let state = self.state.lock();
                (state.activation.clone(), state.driver.clone())
            };
            let driver = driver.map(|driver| driver.advance());
            let Some(operation) = operation else {
                return true;
            };
            let observation = operation.retire();
            if !observation.outstanding && driver.is_none_or(|d| !d.outstanding) {
                return !observation.unknown && driver.is_none_or(|d| !d.unknown);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    fn checkout_host(&self, admitting: bool) -> Option<HostCheckout<'_>> {
        self.checkout_host_for(admitting, None)
    }
    fn checkout_host_for(
        &self,
        admitting: bool,
        delegation: Option<&Arc<()>>,
    ) -> Option<HostCheckout<'_>> {
        let mut state = self.state.lock();
        if admitting {
            match delegation {
                Some(token)
                    if state
                        .delegation
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, token)) => {}
                Some(_) => return None,
                None if state.delegation.is_some() => return None,
                None => state.one_shot_claimed = true,
            }
            // 旧 Arc 只观察原终局，无安装或输入能力，不以引用数冒充任务事实。
            state.peer_pumps.retain(|pump| !pump.known_joined());
        }
        if matches!(state.host, HostSlot::CheckedOut(_))
            || (admitting && (state.closing || state.peer_pumps.len() >= 64))
        {
            return None;
        }
        let token = state.next_host_checkout.checked_add(1)?;
        state.next_host_checkout = token;
        let host = match std::mem::replace(&mut state.host, HostSlot::CheckedOut(token)) {
            HostSlot::Available(host) => Some(host),
            HostSlot::Empty => None,
            HostSlot::CheckedOut(_) => unreachable!("reservation checked under same lock"),
        };
        let mut checkout = HostCheckout {
            service: self,
            token,
            host,
            origin: state.origin.clone(),
            stopped: state.host_stopped,
            finished: false,
            #[cfg(test)]
            before_return: None,
        };
        let closing = state.closing;
        drop(state);
        if closing {
            checkout.stop();
        }
        Some(checkout)
    }
    fn with_host<T>(&self, operation: impl FnOnce(Option<&mut NativeHost>) -> T) -> Option<T> {
        let mut checkout = self.checkout_host(false)?;
        let result = operation(checkout.host.as_deref_mut());
        checkout.finish();
        Some(result)
    }
    #[cfg(test)]
    pub(crate) fn test_state_is_unlocked(&self) -> bool {
        self.state.try_lock().is_some()
    }
    #[cfg(test)]
    pub(crate) fn test_hold_retirement(
        &self,
    ) -> (
        std::sync::mpsc::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered_tx, entered) = std::sync::mpsc::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel();
        let mut state = self.state.lock();
        assert!(state.retirement_gate.is_none());
        state.retirement_gate = Some((entered_tx, release_rx));
        (entered, release)
    }
    #[cfg(test)]
    pub(crate) fn test_after_handoff(&self, hook: Box<dyn FnOnce() + Send>) {
        self.state.lock().after_handoff = Some(hook);
    }
    pub(crate) fn network_admission_open(&self) -> bool {
        let state = self.state.lock();
        !state.closing && state.delegation.is_none() && !state.source_slot.is_closed()
    }
    pub(crate) fn commit_network_handoff(
        &self,
        published: &tokio::sync::watch::Sender<
            Option<Result<(), crate::remote_host_network::HostNetworkError>>,
        >,
        stopped: &tokio::sync::watch::Receiver<bool>,
        end: tokio::time::Instant,
    ) -> Result<(), crate::remote_host_network::HostNetworkError> {
        use crate::remote_host_network::HostNetworkError;
        let mut outcome = Err(HostNetworkError::Closed);
        // 锁序为 publication 值、service、stop 读锁；此闭包没有外部回调。
        published.send_if_modified(|value| {
            let state = self.state.lock();
            let stop = stopped.borrow();
            outcome = if state.closing
                || state.delegation.is_some()
                || state.source_slot.is_closed()
                || *stop
                || !matches!(stopped.has_changed(), Ok(false))
                || value.is_some()
            {
                Err(HostNetworkError::Closed)
            } else if tokio::time::Instant::now() >= end {
                Err(HostNetworkError::Deadline)
            } else {
                Ok(())
            };
            let committed = outcome.is_ok();
            if committed {
                *value = Some(Ok(()));
            }
            // Tokio 释放 publication 值锁后才唤醒；拒绝保留原 cleanup 的最终错误发布。
            drop(stop);
            drop(state);
            committed
        });
        outcome
    }
    #[cfg(test)]
    pub(crate) fn test_pin_count(&self) -> usize {
        self.state.lock().pins.len()
    }
    #[cfg(all(test, windows))]
    pub(crate) fn test_original_managed_facts(
        &self,
        run: &crate::cli::run_registry::RunKey,
    ) -> Option<crate::run_supervisor::ManagedRetirementFacts> {
        self.state
            .lock()
            .native
            .as_mut()?
            .test_original_managed_facts(run)
    }
    #[cfg(test)]
    pub(crate) async fn test_join_exit_worker(&self) -> bool {
        let worker = self.state.lock().waiter.take();
        if let Some(worker) = worker {
            worker.await.is_ok()
        } else {
            false
        }
    }
    #[cfg(test)]
    pub(crate) fn test_exit_worker_held(&self) -> bool {
        self.state.lock().waiter.is_some()
    }
    // Native provider entry. It accepts only an opaque original document/run
    // admission and a real verified channel, never a shutdown pin or JS authority.
    #[allow(
        dead_code,
        reason = "production native provider/identity bootstrap remains Locked"
    )]
    pub(crate) fn adopt_native(
        self: &Arc<Self>,
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
    pub(crate) fn adopt_admitted(
        self: &Arc<Self>,
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
        if !admission.is_owned_by(runtime) {
            return Err(Box::new((
                crate::cli::profiles::error("REMOTE_HOST_CLOSED"),
                admission,
            )));
        }
        let Some(mut checkout) = self.checkout_host(true) else {
            return Err(Box::new((
                crate::cli::profiles::error("REMOTE_HOST_CLOSED"),
                admission,
            )));
        };
        let result = (|| {
            if checkout.host.is_none() {
                let host = match admission.create_host(channel.clone()) {
                    Ok(host) => host,
                    Err(error) => return Err(Box::new((error, admission))),
                };
                // Keep this SAME permanent-admission Host even if the following
                // handoff rejects. Never drop/recreate it on a wrong-resource retry.
                checkout.host = Some(Box::new(host));
            }
            if let Some(origin) = &checkout.origin {
                admission.rebind_origin(
                    origin,
                    checkout
                        .host
                        .as_deref_mut()
                        .expect("retained original Host"),
                    channel.clone(),
                )?;
            } else {
                let origin = match admission.network_origin() {
                    Ok(origin) => origin,
                    Err(error) => return Err(Box::new((error, admission))),
                };
                let pin = admission.adopt(
                    checkout
                        .host
                        .as_deref_mut()
                        .expect("retained original Host"),
                    channel.clone(),
                )?;
                #[cfg(test)]
                {
                    let hook = self.state.lock().after_handoff.take();
                    if let Some(hook) = hook {
                        hook();
                    }
                }
                let mut state = self.state.lock();
                state.origin = Some(origin);
                state.pins.push(pin);
            }
            let original = self.clone();
            let worker =
                tauri::async_runtime::spawn(async move { original.pump_peer(channel).await });
            // Publish the original handle while the service lock is still held.
            // The worker cannot observe its Host before this custody is registered.
            self.state.lock().peer_pumps.push(Arc::new(PeerPump {
                join: tokio::sync::Mutex::new(Some(worker)),
                joined: AtomicU8::new(0),
            }));
            Ok(())
        })();
        checkout.finish();
        result
    }
    async fn pump_peer(
        &self,
        channel: Arc<cc_desk_remote_host::native_host::NativeChannel>,
    ) -> Result<(), ()> {
        let mut connection = loop {
            if channel.authority().check().is_err() {
                break None;
            }
            let closing = self.state.lock().closing;
            let connection = if closing {
                None
            } else {
                self.with_host(|host| {
                    host.and_then(|host| {
                        host.drive_peer_installs();
                        host.peer_connection(&channel).ok()
                    })
                })
                .flatten()
            };
            if closing {
                break None;
            }
            if let Some(connection) = connection {
                break Some(connection);
            }
            if channel
                .peer_install_deadline()
                .is_none_or(|end| tokio::time::Instant::now() >= end)
            {
                break None;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        if let Some(connection) = connection.as_mut() {
            let scope = channel.authority().scope().clone();
            if let Ok(fence) = channel.installed_fence() {
                loop {
                    if self.state.lock().closing || channel.authority().check().is_err() {
                        break;
                    }
                    match channel.receive(&scope, fence, Duration::from_secs(1)).await {
                        Ok(record) => {
                            if connection
                                .dispatch_received(&record, Duration::from_secs(1))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        // Receive leaves its original bounded inbox intact on a
                        // no-effect idle timeout. Writes/dispatch are NEVER retried.
                        Err(cc_desk_remote_client::DataError::Deadline) => continue,
                        Err(_) => break,
                    }
                }
            }
        }
        drop(connection);
        channel.authority().request_close();
        let result = channel
            .wait_closed_checked()
            .await
            .map(|_| ())
            .map_err(|_| ());
        // Phone/channel loss never seals the native writer or stops its process.
        result
    }
    async fn join_peer_pump(&self) -> bool {
        // closing 后不再有新 reservation；原 checkout 返回前，late pump 仍须登记。
        let pumps = loop {
            let ready = {
                let state = self.state.lock();
                if matches!(state.host, HostSlot::CheckedOut(_)) {
                    None
                } else {
                    Some(state.peer_pumps.clone())
                }
            };
            if let Some(pumps) = ready {
                break pumps;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let mut known = true;
        for pump in pumps {
            known &= pump.join().await;
        }
        known
    }
    #[cfg(test)]
    pub(crate) fn test_begin_host_shutdown(&self) {
        let material = {
            let mut state = self.state.lock();
            state.closing = true;
            state.source_slot.close()
        };
        self.release_source_material(material);
        self.with_host(|_| ());
    }
    #[cfg(test)]
    pub(crate) fn test_writer_retirement(&self) -> WriterRetirement {
        self.with_host(|host| host.map_or(WriterRetirement::Joined, NativeHost::observe_shutdown))
            .unwrap_or(WriterRetirement::Pending)
    }
    #[cfg(test)]
    pub(crate) async fn test_join_network(&self) -> bool {
        let (joined, attempts) = tokio::join!(self.join_peer_pump(), self.network.join_all());
        let joined = joined && attempts;
        let network = loop {
            if let Some(network) =
                self.with_host(|host| host.and_then(|host| host.network_retirement()))
            {
                break network;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        if let Some(network) = network {
            let known = network.wait_checked().await.is_ok();
            return joined && known;
        }
        joined
    }
    pub(crate) fn request_exit(&self, code: i32, maintenance_blocked: bool) -> ExitDecision {
        // A maintenance refusal cannot seal admission or replace the first code.
        if maintenance_blocked {
            return ExitDecision::Prevent;
        }
        #[cfg(windows)]
        self.unattended.request_close();
        let mut state = self.state.lock();
        state.closing = true;
        // 原子失效不调用回调；同锁封闭后，晚发布 handle 仍由原槽接收。
        if let Some(operation) = &state.activation {
            operation.cancel();
        }
        let material = state.source_slot.close();
        let decision = match state.exit {
            ExitIntent::Idle => {
                state.exit = ExitIntent::Waiting(code);
                ExitDecision::Wait(code)
            }
            ExitIntent::Ready(original) if original == code => ExitDecision::Proceed,
            _ => ExitDecision::Prevent,
        };
        drop(state);
        #[cfg(windows)]
        self.registration.begin_shutdown();
        self.release_source_material(material);
        decision
    }
    fn complete_exit(&self, code: i32) -> bool {
        #[cfg(windows)]
        if !self.unattended.observe().known() {
            return false;
        }
        let mut state = self.state.lock();
        if !state.retired
            || !Self::activation_retired(&state)
            || state.source_slot.material_returned() != Some(Ok(()))
            || !matches!(state.exit,ExitIntent::Waiting(original) if original==code)
        {
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
            || state.waiter_starting
        {
            return;
        }
        state.waiter_starting = true;
        // 原 Tokio spawn 不同步 poll；先保管唯一 handle，再由同一 waiter 停止原 owners。
        state.waiter = Some(self.spawn_waiter(code, client, None, Some(native), exit));
        state.waiter_starting = false;
    }
    fn spawn_waiter(
        self: &Arc<Self>,
        code: i32,
        client: Arc<RemoteClientService>,
        mut networks: Option<cc_desk_remote_host::native_host::NetworkRetirement>,
        prepare: Option<Arc<NativeRuntime>>,
        exit: impl FnOnce(i32) + Send + 'static,
    ) -> tauri::async_runtime::JoinHandle<()> {
        let original = self.clone();
        tauri::async_runtime::spawn(async move {
            if let Some(native) = prepare {
                let pins = std::mem::take(&mut original.state.lock().pins);
                original.network.begin_shutdown();
                #[cfg(windows)]
                original.registration.begin_shutdown();
                client.begin_shutdown();
                let native = native.begin_host_shutdown(pins);
                original.state.lock().native = Some(native);
                networks = original
                    .with_host(|host| host.and_then(|host| host.network_retirement()))
                    .flatten();
                original.drain_closed_source_materials();
            }
            let network_owner = original.clone();
            let (
                client_result,
                network_known,
                owned_known,
                pump_known,
                attempts_known,
                materials_known,
                activation_known,
                registration_known,
                unattended_known,
            ) = tokio::join!(
                client.shutdown(),
                async move {
                    if let Some(networks) = networks {
                        networks.wait_checked().await.is_ok()
                    } else {
                        network_owner.wait_host_network().await
                    }
                },
                original.observe_owned(),
                original.join_peer_pump(),
                original.network.join_all(),
                original.wait_source_materials(),
                original.wait_activation(),
                async {
                    #[cfg(windows)]
                    {
                        original.registration.join_all().await
                    }
                    #[cfg(not(windows))]
                    {
                        true
                    }
                },
                async {
                    #[cfg(windows)]
                    {
                        original.unattended.wait().await
                    }
                    #[cfg(not(windows))]
                    {
                        true
                    }
                }
            );
            if client_result.is_err()
                || !network_known
                || !owned_known
                || !pump_known
                || !attempts_known
                || !materials_known
                || !activation_known
                || !registration_known
                || !unattended_known
            {
                original.mark_unknown();
                return;
            }
            original.drain_closed_source_materials();
            {
                let mut state = original.state.lock();
                if state.source_slot.material_returned() != Some(Ok(()))
                    || !Self::activation_retired(&state)
                {
                    state.exit = ExitIntent::Unknown;
                    return;
                }
                state.retired = true;
            }
            if original.complete_exit(code) {
                exit(code);
            }
        })
    }
    async fn wait_source_materials(&self) -> bool {
        loop {
            match self.source_material_returned() {
                Some(result) => return result.is_ok(),
                None => {
                    #[cfg(test)]
                    {
                        self.state.lock().source_wait_pending = true;
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            }
        }
    }
    async fn observe_owned(&self) -> bool {
        #[cfg(test)]
        {
            // 原唯一观察者保管 gate；不在 service 锁内等待，失败不伪造退休。
            let gate = { self.state.lock().retirement_gate.take() };
            if let Some((entered, release)) = gate {
                let _ = entered.send(());
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(5), release).await,
                    Ok(Ok(()))
                ) {
                    return false;
                }
            }
        }
        loop {
            #[cfg(windows)]
            self.unattended.advance_quarantine();
            let writer = self
                .with_host(|host| {
                    host.map_or(WriterRetirement::Joined, NativeHost::observe_shutdown)
                })
                .unwrap_or(WriterRetirement::Pending);
            // 原 shutdown owner 临时移出，observe 可能唤醒 input/Job 等原 owner。
            let mut native = NativeShutdownCheckout {
                service: self,
                owner: self.state.lock().native.take(),
                finished: false,
            };
            let observation = native
                .owner
                .as_mut()
                .map(|native| native.observe())
                .unwrap_or(HostNativeObservation::Unknown);
            let mut uncovered = false;
            if writer == WriterRetirement::Joined
                && matches!(
                    observation,
                    HostNativeObservation::OwnedSessionReleased
                        | HostNativeObservation::NoRemoteNativeResources
                )
            {
                let late_pins = std::mem::take(&mut self.state.lock().pins);
                let covered = late_pins.iter().all(|pin| {
                    native
                        .owner
                        .as_ref()
                        .is_some_and(|owner| owner.covers_pin(pin))
                });
                if covered {
                    // 同一原 stop inventory 已实际退休；释放额外引用也不持 service 锁。
                    drop(late_pins);
                } else {
                    self.state.lock().pins.extend(late_pins);
                    uncovered = true;
                }
            }
            native.finished = true;
            drop(native);
            let (pending, unknown) = (
                writer == WriterRetirement::Pending
                    || observation == HostNativeObservation::Pending,
                uncovered
                    || writer == WriterRetirement::Unknown
                    || observation == HostNativeObservation::Unknown,
            );
            if pending {
                tokio::time::sleep(Duration::from_millis(25)).await;
                continue;
            }
            #[cfg(test)]
            {
                self.state.lock().native_observed_terminal = true;
            }
            return !unknown;
        }
    }
    async fn wait_host_network(&self) -> bool {
        loop {
            if let Some(network) =
                self.with_host(|host| host.and_then(|host| host.network_retirement()))
            {
                return if let Some(network) = network {
                    network.wait_checked().await.is_ok()
                } else {
                    true
                };
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    fn empty_native() -> Arc<NativeRuntime> {
        use crate::cli::{launch_service::LaunchService, storage::WorkspaceRepository};
        let dir = tempfile::tempdir().unwrap();
        Arc::new(NativeRuntime::new(Arc::new(LaunchService::new(
            WorkspaceRepository::open(dir.path().join("workspace.json")).unwrap(),
            Some(Default::default()),
            None,
        ))))
    }
    async fn activation_observed(service: &RemoteHostService) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while !service.state.lock().activation_waiting {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn HostActivation_DirectServiceExit_013() {
        let service = Arc::new(RemoteHostService::default());
        let reservation = service.reserve_activation().unwrap();
        let operation = reservation.operation();
        assert!(service.reserve_activation().is_err());
        assert_eq!(service.request_exit(9, true), ExitDecision::Prevent);
        assert!(!service.state.lock().closing);
        assert_eq!(service.request_exit(17, false), ExitDecision::Wait(17));
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        service.start_exit(
            17,
            empty_native(),
            Arc::new(RemoteClientService::default()),
            move |code| {
                let _ = tx.send(code);
            },
        );
        activation_observed(&service).await;
        assert!(operation.snapshot().outstanding);
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(!service.state.lock().retired);
        assert!(!service.complete_exit(17));
        drop(reservation);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), rx)
                .await
                .unwrap()
                .unwrap(),
            17
        );
        let waiter = service.state.lock().waiter.take().unwrap();
        waiter.await.unwrap();
        assert!(service.reserve_activation().is_err());
        assert_eq!(service.request_exit(18, false), ExitDecision::Prevent);
    }
    #[test]
    fn HostActivation_FinalExitFence_014() {
        let service = RemoteHostService::default();
        let reservation = service.reserve_activation().unwrap();
        service.request_exit(1, false);
        service.state.lock().retired = true;
        assert!(!service.complete_exit(1));
        drop(reservation);
        assert!(service.complete_exit(1));
    }
    #[tokio::test]
    async fn HostActivation_UnknownStillJoins_015() {
        let service = Arc::new(RemoteHostService::default());
        let reservation = service.reserve_activation().unwrap();
        let operation = reservation.operation();
        reservation
            .spawn(|_| panic!("TEST_ONLY_ACTIVATION_PANIC"))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while operation.advance_join().outstanding {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(operation.snapshot().unknown);
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, held) = std::sync::mpsc::sync_channel(1);
        let original = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            let _ = held.recv_timeout(Duration::from_secs(3));
        });
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        service.state.lock().native =
            Some(HostNativeShutdown::from_owned_worker_for_test(original));
        service.request_exit(23, false);
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        let waiter = service.spawn_waiter(
            23,
            Arc::new(RemoteClientService::default()),
            None,
            None,
            move |_| {
                let _ = tx.send(());
            },
        );
        service.state.lock().waiter = Some(waiter);
        activation_observed(&service).await;
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        release.send(()).unwrap();
        let waiter = service.state.lock().waiter.take().unwrap();
        tokio::time::timeout(Duration::from_secs(3), waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(!service.state.lock().retired);
        assert!(matches!(service.state.lock().exit, ExitIntent::Unknown));
        assert!(!service.complete_exit(23));
    }
    #[test]
    fn HostActivation_LastServiceRelease_016() {
        let service = Arc::new(RemoteHostService::default());
        let weak = Arc::downgrade(&service);
        let reservation = service.reserve_activation().unwrap();
        let operation = reservation.operation();
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, held) = std::sync::mpsc::sync_channel(1);
        let (returned_tx, returned) = std::sync::mpsc::sync_channel(1);
        reservation
            .spawn_prepared(move |_| {
                let worker_id = std::thread::current().id();
                activation::PreparationResult::Prepared(Box::new(move || {
                    assert_ne!(std::thread::current().id(), worker_id);
                    let original = weak.upgrade().unwrap();
                    entered_tx.send(()).unwrap();
                    let _ = held.recv_timeout(Duration::from_secs(3));
                    drop(original);
                    let _ = returned_tx.send(());
                    true
                }))
            })
            .unwrap();
        let observer = activation::FixtureThread::spawn(move || {
            let until = std::time::Instant::now() + Duration::from_secs(3);
            while operation.worker_fact() != activation::WorkerFact::Joined {
                assert!(std::time::Instant::now() < until);
                operation.advance_join();
                std::thread::yield_now();
            }
            assert!(operation.snapshot().outstanding);
            operation.finish_preparation()
        });
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        drop(service);
        release.send(()).unwrap();
        assert!(
            returned.recv_timeout(Duration::from_secs(3)).is_ok(),
            "last service release must not self-join its worker"
        );
        assert!(observer.join().unwrap().retired());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn HostActivation_ReleaseDoesNotBlockOtherOwners_017() {
        struct Material {
            entered: std::sync::mpsc::SyncSender<()>,
            release: std::sync::mpsc::Receiver<()>,
        }
        impl Drop for Material {
            fn drop(&mut self) {
                let _ = self.entered.send(());
                let _ = self.release.recv_timeout(Duration::from_secs(10));
            }
        }
        let service = Arc::new(RemoteHostService::default());
        let reservation = service.reserve_activation().unwrap();
        let operation = reservation.operation();
        let (entered_tx, entered) = std::sync::mpsc::sync_channel(1);
        let (release, held) = std::sync::mpsc::sync_channel(1);
        let material = Material {
            entered: entered_tx,
            release: held,
        };
        reservation
            .spawn_prepared(move |_| {
                activation::PreparationResult::Prepared(Box::new(move || {
                    drop(material);
                    panic!("closing must discard, never register");
                }))
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while operation.worker_fact() != activation::WorkerFact::Joined {
                operation.advance_join();
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let original = std::thread::spawn(|| {});
        service.state.lock().native =
            Some(HostNativeShutdown::from_owned_worker_for_test(original));
        let (_native_entered, native_release) = service.test_hold_retirement();
        service.request_exit(37, false);
        let (exit_tx, mut exit_rx) = tokio::sync::oneshot::channel();
        let waiter = service.spawn_waiter(
            37,
            Arc::new(RemoteClientService::default()),
            None,
            None,
            move |code| {
                let _ = exit_tx.send(code);
            },
        );
        service.state.lock().waiter = Some(waiter);
        entered.recv_timeout(Duration::from_secs(3)).unwrap();
        native_release.send(()).unwrap();
        let native_joined_while_material_held =
            tokio::time::timeout(Duration::from_secs(3), async {
                while !service.state.lock().native_observed_terminal {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok();
        assert!(operation.snapshot().outstanding);
        assert!(matches!(
            exit_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        // 先释放并回收全部原 owner，再断言 RED，失败也不遗留线程。
        release.send(()).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), exit_rx)
                .await
                .unwrap()
                .unwrap(),
            37
        );
        let waiter = service.state.lock().waiter.take().unwrap();
        waiter.await.unwrap();
        assert!(
            native_joined_while_material_held,
            "blocked material release starved the original native owner join"
        );
    }
    #[tokio::test]
    async fn host_checkout_closing_before_return_stops_original_host() {
        let scope = cc_desk_remote_record::grant::SessionScope {
            session: "11".repeat(16),
            run: "TEST_ONLY_CHECKOUT_RETURN".into(),
            generation: "1".into(),
        };
        let (client, channel, gate, cancel) = crate::tests::remote_channels::channels(scope).await;
        let channel = Arc::new(channel);
        let service = Arc::new(RemoteHostService::default());
        let original = NativeHost::from_channel(channel.clone()).unwrap();
        service.state.lock().host = HostSlot::Available(Box::new(original));
        let mut checkout = service.checkout_host(true).unwrap();
        let closing = service.clone();
        checkout.before_return = Some(Box::new(move || closing.test_begin_host_shutdown()));
        checkout.finish();
        let slot = { std::mem::replace(&mut service.state.lock().host, HostSlot::Empty) };
        let HostSlot::Available(mut original) = slot else {
            panic!("original Host must return");
        };
        let stopped = original.network_retirement().is_some();
        original.begin_shutdown();
        cancel.cancel();
        client.authority().request_close();
        channel.authority().request_close();
        let _ = tokio::join!(client.wait_closed(), channel.wait_closed());
        drop(gate);
        assert!(
            stopped,
            "closing raced return and original Host was not stopped"
        );
    }
    #[test]
    fn host_checkout_busy_is_pending_and_closing_survives_return() {
        let service = RemoteHostService::default();
        let checkout = service.checkout_host(true).unwrap();
        assert!(service.checkout_host(true).is_none());
        assert_eq!(service.test_writer_retirement(), WriterRetirement::Pending);
        service.test_begin_host_shutdown();
        assert!(!service.network_admission_open());
        assert_eq!(service.test_writer_retirement(), WriterRetirement::Pending);
        checkout.finish();
        assert!(service.checkout_host(true).is_none());
        assert_eq!(service.test_writer_retirement(), WriterRetirement::Joined);
    }
    #[test]
    fn host_checkout_unwind_latches_unknown_without_losing_reservation() {
        let service = RemoteHostService::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _checkout = service.checkout_host(true).unwrap();
            panic!("TEST_ONLY_HOST_CHECKOUT_PANIC");
        }));
        assert!(result.is_err());
        assert!(matches!(service.state.lock().host, HostSlot::Empty));
        assert!(matches!(service.state.lock().exit, ExitIntent::Unknown));
        assert!(!service.network_admission_open());
        assert!(service.checkout_host(true).is_none());
    }
    #[tokio::test]
    async fn cancelling_peer_join_keeps_the_original_owned_task() {
        let (release, wait) = tokio::sync::oneshot::channel();
        let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = completed.clone();
        let worker = tauri::async_runtime::spawn(async move {
            let _ = wait.await;
            observed.store(true, Ordering::Release);
            Ok::<(), ()>(())
        });
        let pump = PeerPump {
            join: tokio::sync::Mutex::new(Some(worker)),
            joined: AtomicU8::new(0),
        };
        assert!(tokio::time::timeout(Duration::from_millis(10), pump.join())
            .await
            .is_err());
        assert!(pump.join.lock().await.is_some());
        assert!(!completed.load(Ordering::Acquire));
        release.send(()).unwrap();
        assert!(pump.join().await);
        assert!(completed.load(Ordering::Acquire));
    }
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

    #[tokio::test]
    #[allow(non_snake_case)]
    async fn AppHistory_JoinedPeerPumpsSurvive130Reservations_001() {
        let service = RemoteHostService::default();
        let mut old = Vec::new();
        for generation in 0..130 {
            let pump = Arc::new(PeerPump {
                join: tokio::sync::Mutex::new(Some(tauri::async_runtime::spawn(async { Ok(()) }))),
                joined: AtomicU8::new(0),
            });
            service.state.lock().peer_pumps.push(pump.clone());
            assert!(pump.join().await);
            old.push(pump);
            let checkout = service.checkout_host(true);
            assert!(
                checkout.is_some(),
                "joined history exhausted generation {generation}"
            );
            checkout.unwrap().finish();
            assert!(service.state.lock().peer_pumps.len() < 64);
        }
        // 已回收旧 Arc 只能观察同一缓存事实，不代表新任务或授权。
        for pump in old {
            assert!(pump.join().await);
        }
    }

    #[tokio::test]
    async fn AppHistory_UnconsumedFinishedHandlesKeepCapacity_002() {
        let service = RemoteHostService::default();
        let mut original = Vec::new();
        for _ in 0..64 {
            let pump = Arc::new(PeerPump {
                join: tokio::sync::Mutex::new(Some(tauri::async_runtime::spawn(async { Ok(()) }))),
                joined: AtomicU8::new(0),
            });
            service.state.lock().peer_pumps.push(pump.clone());
            original.push(pump);
        }
        tokio::task::yield_now().await;
        assert!(service.checkout_host(true).is_none());
        assert_eq!(service.state.lock().peer_pumps.len(), 64);
        assert!(original[0].join().await);
        service.checkout_host(true).unwrap().finish();
        assert_eq!(service.state.lock().peer_pumps.len(), 63);
        for pump in original {
            assert!(pump.join().await);
        }
    }
    #[tokio::test]
    async fn AppHistory_CancelledObserverAndSlotContentionKeepOriginal_003() {
        let service = RemoteHostService::default();
        let (release, wait) = tokio::sync::oneshot::channel();
        let pump = Arc::new(PeerPump {
            join: tokio::sync::Mutex::new(Some(tauri::async_runtime::spawn(async move {
                wait.await.map_err(|_| ())?;
                Ok(())
            }))),
            joined: AtomicU8::new(0),
        });
        service.state.lock().peer_pumps.push(pump.clone());
        assert!(tokio::time::timeout(Duration::from_millis(10), pump.join())
            .await
            .is_err());
        service.checkout_host(true).unwrap().finish();
        assert_eq!(service.state.lock().peer_pumps.len(), 1);
        release.send(()).unwrap();
        assert!(pump.join().await);
        let slot = pump.join.lock().await;
        service.checkout_host(true).unwrap().finish();
        assert_eq!(service.state.lock().peer_pumps.len(), 1);
        drop(slot);
        service.checkout_host(true).unwrap().finish();
        assert!(service.state.lock().peer_pumps.is_empty());
        assert!(pump.join().await);
    }
    #[tokio::test]
    async fn AppHistory_UnknownRetainedAndShutdownNeverReopens_004() {
        let service = RemoteHostService::default();
        let pump = Arc::new(PeerPump {
            join: tokio::sync::Mutex::new(Some(tauri::async_runtime::spawn(async {
                panic!("TEST_ONLY original pump panic");
                #[allow(unreachable_code)]
                Ok(())
            }))),
            joined: AtomicU8::new(0),
        });
        service.state.lock().peer_pumps.push(pump.clone());
        assert!(!pump.join().await);
        service.checkout_host(true).unwrap().finish();
        assert_eq!(service.state.lock().peer_pumps.len(), 1);
        service.state.lock().closing = true;
        assert!(service.checkout_host(true).is_none());
        assert_eq!(service.state.lock().peer_pumps.len(), 1);
        assert!(!pump.join().await);
    }
}
