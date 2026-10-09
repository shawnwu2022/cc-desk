//! Dedicated native transaction thread. Registry-bearing source/coordinator
//! values never enter Tauri managed state or an asynchronous executor future.
use super::{
    journal::{JournalBinding, JournalStore, ManifestRole},
    manager_document::ManagerDocumentBinding,
    manager_types::{ManagerAction, ManagerBlockReason, ManagerStatus},
    windows::{
        manager_handoff::InitialManager, package::RetainedPackage, process::ExactProcess,
        source_session::SourceCaptureSession, startup::InstallationControl,
    },
};
use crate::cli::{profiles::error, types::SafeError};
use parking_lot::Mutex;
use std::sync::{mpsc, Arc, Weak};
use tokio::sync::oneshot;

type ActionResult = Result<ManagerStatus, SafeError>;
type PreparationResult = Result<(), SafeError>;

/// Original native window, resource witness and authenticated document header.
/// The worker rechecks this object immediately before each accepted action.
pub(crate) struct ManagerDocumentProof {
    binding: Arc<ManagerDocumentBinding>,
    webview: tauri::Webview,
    headers: tauri::http::HeaderMap,
    transaction: String,
}
impl ManagerDocumentProof {
    pub(crate) fn admit(
        binding: Arc<ManagerDocumentBinding>,
        webview: tauri::Webview,
        headers: tauri::http::HeaderMap,
        transaction: &str,
    ) -> Result<Arc<Self>, SafeError> {
        let proof = Arc::new(Self {
            binding,
            webview,
            headers,
            transaction: transaction.into(),
        });
        proof.check()?;
        Ok(proof)
    }
    pub(crate) fn check(&self) -> Result<(), SafeError> {
        if self.binding.admit_native(&self.webview, &self.headers)? != self.transaction {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
    pub(crate) fn check_transaction(&self, binding: &JournalBinding) -> Result<(), SafeError> {
        self.check()?;
        if self.transaction != binding.transaction_id {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
}

/// 重开的命令状态仅负责排重与显示；缓存不能代替原生检查点和独占控制。
pub(crate) struct ReentryCommandState {
    transaction: String,
    latest: ActionResult,
    spent_through: Option<u64>,
    returning: bool,
}
impl ReentryCommandState {
    pub(crate) fn new(transaction: &str) -> Self {
        Self {
            transaction: transaction.into(),
            latest: Err(error("HISTORY_MANAGER_NOT_READY")),
            spent_through: None,
            returning: false,
        }
    }
    pub(crate) fn returning(&self) -> bool {
        self.returning
    }
    pub(crate) fn status(&self) -> ActionResult {
        self.latest.clone()
    }
    pub(crate) fn fail(&mut self, failure: SafeError) {
        self.latest = Err(failure);
    }
    pub(crate) fn publish(&mut self, mut status: ManagerStatus) -> Result<(), SafeError> {
        if status.transaction_id != self.transaction {
            return Err(error("FORBIDDEN"));
        }
        if self
            .latest
            .as_ref()
            .is_ok_and(|previous| previous.generation.get() > status.generation.get())
        {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        // 已发送/运行中的同一代次不能因为旧的只读投影重新获得按钮。
        if self.returning
            || self
                .spent_through
                .is_some_and(|generation| status.generation.get() <= generation)
        {
            status.allowed_actions = vec![ManagerAction::Refresh];
        }
        self.latest = Ok(status);
        Ok(())
    }
}

/// 守卫由真正的阻塞工作持有，请求 future 丢失不会提前释放执行中标记。
pub(crate) struct ReentryReturnOperation {
    state: Option<Arc<Mutex<ReentryCommandState>>>,
}
impl ReentryReturnOperation {
    pub(crate) fn begin(
        state: &Arc<Mutex<ReentryCommandState>>,
        expected_generation: u64,
    ) -> Result<Self, SafeError> {
        let mut current = state.lock();
        if current.returning {
            return Err(error("HISTORY_OPERATION_PENDING"));
        }
        let status = current.latest.as_ref().map_err(Clone::clone)?;
        if status.generation.get() != expected_generation {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        if !status
            .allowed_actions
            .contains(&ManagerAction::ReturnToPrevious)
            || current
                .spent_through
                .is_some_and(|generation| expected_generation <= generation)
        {
            return Err(error("HISTORY_RETURN_CHECKPOINT_BLOCKED"));
        }
        current.spent_through = Some(expected_generation);
        current.returning = true;
        if let Ok(status) = &mut current.latest {
            status.allowed_actions = vec![ManagerAction::Refresh];
        }
        Ok(Self {
            state: Some(state.clone()),
        })
    }
    pub(crate) fn finish(mut self, result: ActionResult) -> ActionResult {
        let state = self.state.take().expect("owned return operation");
        let mut state = state.lock();
        // 完成回执也不能携带新的写入动作。再次操作必须经过新的只读检查。
        let result = result.and_then(|status| {
            state.publish(status)?;
            state.status()
        });
        if let Err(failure) = &result {
            state.latest = Err(failure.clone());
        }
        state.returning = false;
        result
    }
}
impl Drop for ReentryReturnOperation {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            let mut state = state.lock();
            state.latest = Err(error("HISTORY_RECOVERY_REQUIRED"));
            state.returning = false;
        }
    }
}

/// A queued command is still only a request. It cannot authorize an effect
/// until check observes its original document and the current actual journal.
pub(crate) struct AuthenticatedManagerCommand {
    action: ManagerAction,
    expected_generation: u64,
    document: Arc<ManagerDocumentProof>,
    completed: oneshot::Sender<ActionResult>,
}
impl AuthenticatedManagerCommand {
    pub(crate) fn action(&self) -> ManagerAction {
        self.action
    }
    pub(crate) fn expected_generation(&self) -> u64 {
        self.expected_generation
    }
    pub(crate) fn check(
        &self,
        binding: &JournalBinding,
        store: &JournalStore,
    ) -> Result<(), SafeError> {
        self.document.check()?;
        if self.document.transaction != binding.transaction_id {
            return Err(error("FORBIDDEN"));
        }
        let inspected = store.inspect(binding)?;
        let journal = inspected
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if inspected.blocked || journal.generation() != self.expected_generation {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        self.document.check()
    }
    /// Rechecks only document ownership after an attempted effect; the command's
    /// old generation must not be reused as if nothing had happened.
    pub(crate) fn finish(self, result: ActionResult) {
        let result = self.document.check().and(result);
        let _ = self.completed.send(result);
    }
    /// Converts a current, explicit Return into one native operation. Later
    /// cleanup/restore stages recheck the original document and the backend's
    /// actual journal generation without pretending the UI sent a new request.
    pub(crate) fn accept_return(
        self,
        binding: &JournalBinding,
        store: &JournalStore,
    ) -> Result<AcceptedManagerReturn, SafeError> {
        let result = self.check(binding, store).and_then(|()| {
            if self.action != ManagerAction::ReturnToPrevious {
                return Err(error("FORBIDDEN"));
            }
            Ok(())
        });
        if let Err(failure) = result {
            self.finish(Err(failure.clone()));
            return Err(failure);
        }
        Ok(AcceptedManagerReturn {
            command: self,
            binding: binding.clone(),
        })
    }
}

/// Never deserialized or constructed from a status/generation projection.
/// Cleanup of an owned unstarted child and the matching Return share this one
/// original authenticated intent; it cannot authorize another transaction.
pub(crate) struct AcceptedManagerReturn {
    command: AuthenticatedManagerCommand,
    binding: JournalBinding,
}
impl AcceptedManagerReturn {
    pub(crate) fn verify(
        &self,
        binding: &JournalBinding,
        store: &JournalStore,
        actual_generation: u64,
    ) -> Result<(), SafeError> {
        self.command.document.check()?;
        if &self.binding != binding || self.command.document.transaction != binding.transaction_id {
            return Err(error("FORBIDDEN"));
        }
        let inspected = store.inspect(binding)?;
        let journal = inspected
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if inspected.blocked || journal.generation() != actual_generation {
            return Err(error("HISTORY_GENERATION_CHANGED"));
        }
        self.command.document.check()
    }
    pub(crate) fn finish(self, result: ActionResult) {
        self.command.finish(result);
    }
}

#[derive(Default)]
struct ProgressCache {
    latest: Option<ActionResult>,
}
impl ProgressCache {
    fn read(&self) -> ActionResult {
        self.latest
            .clone()
            .unwrap_or_else(|| Err(error("HISTORY_MANAGER_NOT_READY")))
    }
}

/// The coordinator owns this on its dedicated thread. Every visible status is
/// projected from a real healthy inspected journal and the actual retained
/// package. Offered actions remain diagnostic; commands re-admit all authority.
pub(crate) struct ProgressPublisher {
    installation: Arc<InstallationControl>,
    binding: JournalBinding,
    package: Arc<RetainedPackage>,
    cache: Arc<Mutex<ProgressCache>>,
    commands: mpsc::Receiver<AuthenticatedManagerCommand>,
}
impl ProgressPublisher {
    pub(crate) fn publish(
        &self,
        store: &mut JournalStore,
        blocked: Option<ManagerBlockReason>,
        actions: &[ManagerAction],
    ) -> ActionResult {
        self.package.verify_retained()?;
        let inspected = store.inspect(&self.binding)?;
        if inspected.blocked {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        let journal = inspected
            .last_valid
            .as_ref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?;
        if journal.manifest(ManifestRole::ManagerHandoff).is_none() {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        store.verify_windows_binding(
            self.installation.root(),
            &self.binding,
            journal.generation(),
        )?;
        let status = ManagerStatus::project(journal, self.package.selection(), blocked, actions)?;
        self.package.verify_retained()?;
        self.cache.lock().latest = Some(Ok(status.clone()));
        Ok(status)
    }
    pub(crate) fn fail(&self, failure: SafeError) {
        self.cache.lock().latest = Some(Err(failure));
    }
    pub(crate) fn recv_command(&self) -> Result<AuthenticatedManagerCommand, SafeError> {
        self.commands
            .recv()
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))
    }
    pub(crate) fn try_command(&self) -> Result<Option<AuthenticatedManagerCommand>, SafeError> {
        match self.commands.try_recv() {
            Ok(command) => Ok(Some(command)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(error("HISTORY_HANDOFF_CHANGED")),
        }
    }
    /// Timeout supplies no quiescence evidence. The coordinator continues its
    /// exact process-handle observations when no command arrives.
    pub(crate) fn recv_command_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> Result<Option<AuthenticatedManagerCommand>, SafeError> {
        match self.commands.recv_timeout(timeout) {
            Ok(command) => Ok(Some(command)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(error("HISTORY_HANDOFF_CHANGED")),
        }
    }
    fn publish_waiting_source(&self) -> PreparationResult {
        let control = self.installation.acquire_control()?;
        let mut store = JournalStore::open_windows_transaction(
            self.installation.root().clone(),
            &self.binding.transaction_id,
        )?;
        store.bind_existing(&self.binding)?;
        self.publish(
            &mut store,
            Some(ManagerBlockReason::SourceStillRunning),
            &[ManagerAction::Refresh],
        )?;
        control
            .verify_root(self.installation.root())
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StartupPhase {
    Unprepared,
    Prepared,
    Activated,
    Failed,
}
struct WorkerStartup {
    preparation: Mutex<Option<oneshot::Receiver<PreparationResult>>>,
    publication: Mutex<Option<mpsc::SyncSender<()>>>,
    phase: Mutex<StartupPhase>,
}
impl WorkerStartup {
    fn new(
        preparation: oneshot::Receiver<PreparationResult>,
        publication: mpsc::SyncSender<()>,
    ) -> Self {
        Self {
            preparation: Mutex::new(Some(preparation)),
            publication: Mutex::new(Some(publication)),
            phase: Mutex::new(StartupPhase::Unprepared),
        }
    }
    async fn await_prepared(&self) -> PreparationResult {
        let receive = self
            .preparation
            .lock()
            .take()
            .ok_or_else(|| error("HISTORY_MANAGER_NOT_READY"))?;
        let result = receive
            .await
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))
            .and_then(|result| result);
        *self.phase.lock() = if result.is_ok() {
            StartupPhase::Prepared
        } else {
            StartupPhase::Failed
        };
        if result.is_err() {
            drop(self.publication.lock().take());
        }
        result
    }
    fn activate(&self) -> PreparationResult {
        let mut phase = self.phase.lock();
        if *phase != StartupPhase::Prepared {
            return Err(error("HISTORY_MANAGER_NOT_READY"));
        }
        *phase = StartupPhase::Failed;
        let sender = self
            .publication
            .lock()
            .take()
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))?;
        sender
            .try_send(())
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        *phase = StartupPhase::Activated;
        Ok(())
    }
}

/// Send-safe command/diagnostic ports only; no session, registry key, source
/// boundary, fence or installer owner crosses this channel.
pub(crate) struct ManagerWorker {
    startup: WorkerStartup,
    preparation_observer: Mutex<Option<oneshot::Receiver<PreparationResult>>>,
    cache: Arc<Mutex<ProgressCache>>,
    commands: mpsc::SyncSender<AuthenticatedManagerCommand>,
    shutdown: Mutex<Option<mpsc::SyncSender<()>>>,
}
impl ManagerWorker {
    pub(crate) fn start(owner: &Arc<Mutex<InitialManager>>) -> Result<Self, SafeError> {
        let (prepared, preparation) = oneshot::channel();
        let (observed, preparation_observer) = oneshot::channel();
        let (published, publication) = mpsc::sync_channel(1);
        let (commands, command_receiver) = mpsc::sync_channel(1);
        let (shutdown, shutdown_receiver) = mpsc::sync_channel(1);
        let cache = Arc::new(Mutex::new(ProgressCache::default()));
        let thread_cache = cache.clone();
        let owner = Arc::downgrade(owner);
        std::thread::Builder::new()
            .name("cc-desk-source-manager".into())
            .spawn(move || {
                run_worker(
                    owner,
                    prepared,
                    observed,
                    publication,
                    command_receiver,
                    shutdown_receiver,
                    thread_cache,
                );
            })
            .map_err(|_| error("HISTORY_MANAGER_UI_UNAVAILABLE"))?;
        Ok(Self {
            startup: WorkerStartup::new(preparation, published),
            preparation_observer: Mutex::new(Some(preparation_observer)),
            cache,
            commands,
            shutdown: Mutex::new(Some(shutdown)),
        })
    }
    pub(crate) async fn await_prepared(&self) -> PreparationResult {
        self.startup.await_prepared().await
    }
    pub(crate) fn take_preparation_observer(
        &self,
    ) -> Result<oneshot::Receiver<PreparationResult>, SafeError> {
        self.preparation_observer
            .lock()
            .take()
            .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))
    }
    /// Called only in the actual native Ready publication callback, after the
    /// protected Ready record succeeded. The worker independently checks it.
    pub(crate) fn ready_published(&self) -> PreparationResult {
        self.startup.activate()
    }
    pub(crate) fn status(&self) -> ActionResult {
        self.cache.lock().read()
    }
    pub(crate) fn shutdown(&self) {
        if let Some(shutdown) = self.shutdown.lock().take() {
            let _ = shutdown.try_send(());
        }
    }
    pub(crate) async fn dispatch(
        &self,
        action: ManagerAction,
        expected_generation: u64,
        document: Arc<ManagerDocumentProof>,
    ) -> ActionResult {
        document.check()?;
        if *self.startup.phase.lock() != StartupPhase::Activated || action == ManagerAction::Refresh
        {
            return Err(error("HISTORY_MANAGER_NOT_READY"));
        }
        let (completed, receive) = oneshot::channel();
        let command = AuthenticatedManagerCommand {
            action,
            expected_generation,
            document: document.clone(),
            completed,
        };
        self.commands
            .try_send(command)
            .map_err(|failure| match failure {
                mpsc::TrySendError::Full(_) => error("HISTORY_OPERATION_PENDING"),
                mpsc::TrySendError::Disconnected(_) => error("HISTORY_HANDOFF_CHANGED"),
            })?;
        let result = receive
            .await
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        document.check()?;
        result
    }
}
impl Drop for ManagerWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn upgrade_owner(
    owner: &Weak<Mutex<InitialManager>>,
) -> Result<Arc<Mutex<InitialManager>>, SafeError> {
    owner
        .upgrade()
        .ok_or_else(|| error("HISTORY_HANDOFF_CHANGED"))
}
fn hold_until_shutdown<T>(evidence: T, shutdown: &mpsc::Receiver<()>) {
    let _ = shutdown.recv();
    drop(evidence);
}

fn report_preparation(
    prepared: oneshot::Sender<PreparationResult>,
    observer: oneshot::Sender<PreparationResult>,
    result: PreparationResult,
) -> bool {
    // The backend observer must hear failure even if there is no frontend
    // request waiting for preparation (or that request has been dropped).
    let _ = observer.send(result.clone());
    prepared.send(result).is_ok()
}

fn run_worker(
    owner: Weak<Mutex<InitialManager>>,
    prepared: oneshot::Sender<PreparationResult>,
    observed: oneshot::Sender<PreparationResult>,
    publication: mpsc::Receiver<()>,
    commands: mpsc::Receiver<AuthenticatedManagerCommand>,
    shutdown: mpsc::Receiver<()>,
    cache: Arc<Mutex<ProgressCache>>,
) {
    // Only Send-safe seed handles enter this native thread. Every registry
    // capture is constructed by prepare below on this exact thread.
    let inputs = (|| {
        let owner = upgrade_owner(&owner)?;
        let owner = owner.lock();
        owner
            .child
            .verify_material()
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        let source = ExactProcess::reopen(
            owner
                .child
                .source()
                .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?
                .identity(),
        )
        .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        let waiter = ExactProcess::reopen(source.identity())
            .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        Ok::<_, SafeError>((
            owner.installation.clone(),
            owner.data.clone(),
            owner.binding.clone(),
            owner.child.retained_package_owner(),
            source,
            waiter,
        ))
    })();
    let (installation, data, binding, package, source, waiter) = match inputs {
        Ok(inputs) => inputs,
        Err(failure) => {
            cache.lock().latest = Some(Err(failure.clone()));
            report_preparation(prepared, observed, Err(failure));
            return;
        }
    };
    let progress = ProgressPublisher {
        installation: installation.clone(),
        binding: binding.clone(),
        package: package.clone(),
        cache,
        commands,
    };
    let session = match SourceCaptureSession::prepare(installation, data, binding, source, package)
    {
        Ok(session) => session,
        Err(failure) => {
            progress.fail(failure.error().clone());
            report_preparation(prepared, observed, Err(failure.error().clone()));
            hold_until_shutdown(failure, &shutdown);
            return;
        }
    };
    if let Err(failure) = progress.publish_waiting_source() {
        progress.fail(failure.clone());
        report_preparation(prepared, observed, Err(failure));
        hold_until_shutdown(session, &shutdown);
        return;
    }
    if !report_preparation(prepared, observed, Ok(())) || publication.recv().is_err() {
        hold_until_shutdown(session, &shutdown);
        return;
    }
    let exited_source = (|| {
        {
            let initial = upgrade_owner(&owner)?;
            let initial = initial.lock();
            initial
                .child
                .ready_reference()
                .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
            initial
                .child
                .verify_material()
                .map_err(|_| error("HISTORY_HANDOFF_CHANGED"))?;
        }
        loop {
            if owner.upgrade().is_none()
                || !matches!(shutdown.try_recv(), Err(mpsc::TryRecvError::Empty))
            {
                return Err(error("HISTORY_HANDOFF_CHANGED"));
            }
            if waiter
                .terminal(200)
                .map_err(|_| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))?
                .is_some()
            {
                break;
            }
        }
        // Release this duplicate image reader before source_session acquires
        // the share-zero image fence. The child transfers its exact owner once.
        drop(waiter);
        let initial = upgrade_owner(&owner)?;
        let mut initial = initial.lock();
        initial
            .child
            .take_exited_source()
            .map_err(|_| error("HISTORY_SOURCE_EXIT_UNCONFIRMED"))
    })();
    let exited_source = match exited_source {
        Ok(source) => source,
        Err(failure) => {
            progress.fail(failure);
            hold_until_shutdown(session, &shutdown);
            return;
        }
    };
    let acquired = match session.acquire_after_exit(exited_source) {
        Ok(acquired) => acquired,
        Err(failure) => {
            progress.fail(failure.error().clone());
            hold_until_shutdown(failure, &shutdown);
            return;
        }
    };
    let owner = match upgrade_owner(&owner) {
        Ok(owner) => owner,
        Err(failure) => {
            progress.fail(failure);
            hold_until_shutdown(acquired, &shutdown);
            return;
        }
    };
    // This is the real coordinator dependency, never a placeholder success.
    // Its local outcome retains evidence and publishes its safe failure itself.
    let cache = progress.cache.clone();
    let outcome = super::windows::coordinator::run_acquired(acquired.into_parts(), owner, progress);
    if let Err(failure) = &outcome {
        cache.lock().latest = Some(Err(failure.error().clone()));
    }
    hold_until_shutdown(outcome, &shutdown);
}

#[cfg(test)]
#[allow(non_snake_case)]
#[path = "../tests/version_history_manager_worker_windows.rs"]
mod tests;
