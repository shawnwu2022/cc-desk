//! Application-owned launch composition. No wire caller may install a supervisor.
#![allow(dead_code)]
use super::environment::EnvMap;
use super::invocation::build_invocation;
use super::launch::LaunchCoordinator;
use super::output_route::OutputRoute;
use super::profiles::error;
use super::routed_launch::RoutedResource;
use super::run_registry::{LaunchPhase, LaunchStatus, RunKey, RunRegistry};
use super::snapshot::{freeze_launch, CallerIdentity, FreezeContext, LaunchSnapshot};
use super::storage::WorkspaceRepository;
use super::types::{LaunchRequest, SafeError};
use crate::platform::launch::resolve_process;
use crate::platform::owned_pty::OwnedPty;
use crate::terminal_transport::OutputFrame;
use crate::version_history::maintenance::{PreparingStart, RuntimeKind};
use parking_lot::{Condvar, Mutex, RwLock};
use portable_pty::PtySize;
use std::cell::{Cell, RefCell};
use std::io::{self, Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) struct FrozenPty {
    pub(crate) pty: OwnedPty,
    pub(crate) snapshot: Arc<LaunchSnapshot>,
    pub(crate) observer: Option<crate::observer_registry::ObserverLease>,
    pub(crate) observer_failure: Option<&'static str>,
}
pub(crate) type NativeRun = RoutedResource<FrozenPty, OutputRoute<OutputFrame>>;

/// D14/D15 supply a backend consumer. It must retain/reap its owned run and
/// implement bounded output/drain policy. This interface is never deserialized.
pub(crate) trait RunSupervisor: Send + Sync {
    fn adopt(
        &self,
        registry: Arc<RunRegistry<NativeRun>>,
        run: &RunKey,
        resource: Arc<NativeRun>,
    ) -> Result<(), SafeError>;
}

#[derive(Default)]
struct CommandOwners {
    active: Mutex<usize>,
    settled: Condvar,
}

struct CommandOwner {
    owners: Arc<CommandOwners>,
    settled: bool,
}
impl CommandOwners {
    fn register(self: &Arc<Self>) -> Result<CommandOwner, SafeError> {
        let mut active = self.active.lock();
        // 与 launch receipt 同量级的硬上限，关闭等待不维护无界 owner 清单。
        if *active >= 4096 {
            return Err(error("REGISTRY_CAPACITY"));
        }
        *active += 1;
        Ok(CommandOwner {
            owners: self.clone(),
            settled: false,
        })
    }
}
impl Drop for CommandOwner {
    fn drop(&mut self) {
        // 只有真实 reap 或已证实未创建 child 才解除；panic/未知状态继续阻止清静证明。
        if self.settled {
            *self.owners.active.lock() -= 1;
            self.owners.settled.notify_all();
        }
    }
}

pub(crate) struct LaunchService {
    coordinator: LaunchCoordinator<NativeRun>,
    repository: WorkspaceRepository,
    inherited: Option<EnvMap>,
    supervisor: Option<Arc<dyn RunSupervisor>>,
    observer: Option<Arc<crate::observer_host::ObserverHost>>,
    shutting_down: Arc<RwLock<bool>>,
    command_owners: Arc<CommandOwners>,
}
impl LaunchService {
    pub(crate) fn rename_metadata(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        authority: super::native_projection::registry::RenameAuthority,
        title: String,
    ) -> Result<(), SafeError> {
        self.registry().check_caller(caller)?;
        let profile = self.repository.get_profile(&request.profile_id)?;
        let legacy =
            profile.read_legacy(&self.repository.metadata_directory().join("config.json"))?;
        let inherited = self.inherited_environment()?;
        let snapshot = freeze_launch(
            request,
            &profile,
            caller,
            &FreezeContext {
                inherited: &inherited,
                terminal: &EnvMap::new(),
                legacy: legacy.as_ref(),
                observer: None,
            },
        )?;
        let selected =
            super::native_projection::selection::locations(request.cli, snapshot.environment())
                .map_err(error)?;
        if selected.root != authority.root_path() {
            return Err(error("SCOPE_STALE"));
        }
        let private = if request.cli == super::types::CliKind::Claude {
            Some(authority.prepare_claude()?)
        } else {
            None
        };
        let claude_command = if let Some(namespace) = &private {
            let bridge = namespace
                .install_bridge(super::native_rename::claude::embedded_bridge())
                .map_err(error)?;
            Some(super::native_rename::claude::command(
                snapshot.environment(),
                &bridge,
                &namespace.path,
            )?)
        } else {
            None
        };
        let spec = if claude_command.is_none() {
            Some(resolve_process(&build_invocation(request, &snapshot)?)?)
        } else {
            None
        };
        let mut admission = self.admit_start()?;
        let registry = self.registry().clone();
        let caller = caller.clone();
        let shutting_down = self.shutting_down.clone();
        let owners = self.command_owners.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        // 先建立独立 owner，再创建 child；IPC 取消不释放进程或维护 admission。
        std::thread::Builder::new()
            .name("native-rename-owner".into())
            .spawn(move || {
                let run = || -> Result<(), SafeError> {
                    let shutdown = shutting_down.read();
                    if *shutdown {
                        return Err(error("RUN_SUPERVISOR_STOPPING"));
                    }
                    registry.check_caller(&caller)?;
                    authority.current()?;
                    let mut command = if let Some(command) = claude_command {
                        command
                    } else {
                        spec.ok_or_else(|| error("RENAME_FAILED"))?
                            .metadata_command()?
                    };
                    let mut owner = owners.register()?;
                    let ticket = admission.begin_creation();
                    let mut child = match command.spawn() {
                        Ok(child) => child,
                        Err(_) => {
                            ticket.no_child_created();
                            drop(private);
                            owner.settled = true;
                            return Err(error("RENAME_FAILED"));
                        }
                    };
                    let ticket = ticket.child_created();
                    drop(shutdown);
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        let current = || {
                            if *shutting_down.read() {
                                return Err(error("RUN_SUPERVISOR_STOPPING"));
                            }
                            registry.check_caller(&caller)
                        };
                        let _namespace = &private;
                        if _namespace.is_some() {
                            super::native_rename::claude::process(
                                &mut child, &authority, &title, &current,
                            )
                        } else {
                            super::native_rename::process(&mut child, &authority, &title, &current)
                        }
                    }))
                    .unwrap_or_else(|_| Err(error("RENAME_OUTCOME_UNKNOWN")));
                    // 结果可先回传；只有精确 child 真正 reap 后才释放 admission。
                    let _ = sender.send(result);
                    loop {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                drop(private);
                                ticket.reaped();
                                owner.settled = true;
                                return Ok(());
                            }
                            Ok(None) | Err(_) => {
                                let _ = child.kill();
                            }
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                };
                if let Err(failure) = run() {
                    let _ = sender.send(Err(failure));
                }
            })
            .map_err(|_| error("RENAME_FAILED"))?;
        receiver
            .recv()
            .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?
    }
    pub(crate) fn new(
        repository: WorkspaceRepository,
        inherited: Option<EnvMap>,
        supervisor: Option<Arc<dyn RunSupervisor>>,
    ) -> Self {
        Self {
            coordinator: LaunchCoordinator::new(4096),
            repository,
            inherited,
            supervisor,
            observer: None,
            shutting_down: Arc::new(RwLock::new(false)),
            command_owners: Arc::new(CommandOwners::default()),
        }
    }
    pub(crate) fn with_observer(mut self, host: Arc<crate::observer_host::ObserverHost>) -> Self {
        self.observer = Some(host);
        self
    }
    /// Backend-only source admission uses the same workspace and host environment as launch.
    pub(crate) fn repository(&self) -> &WorkspaceRepository {
        &self.repository
    }
    pub(crate) fn inherited_environment(&self) -> Result<EnvMap, SafeError> {
        match &self.inherited {
            Some(environment) => Ok(environment.clone()),
            None => super::environment::capture_environment(),
        }
    }
    pub(crate) fn registry(&self) -> &Arc<RunRegistry<NativeRun>> {
        self.coordinator.registry()
    }
    pub(crate) fn start(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        connect: impl FnOnce(&LaunchStatus) -> Result<OutputRoute<OutputFrame>, SafeError>,
    ) -> Result<LaunchStatus, SafeError> {
        if let Some(status) = self.registry().existing(caller, request)? {
            return Ok(status);
        }
        self.start_admitted(self.admit_start()?, caller, request, connect)
    }

    pub(crate) fn admit_start(&self) -> Result<PreparingStart, SafeError> {
        Ok(self
            .repository
            .admission()
            .begin_start(RuntimeKind::Native)?
            .preparing())
    }

    pub(crate) fn start_admitted(
        &self,
        mut admission: PreparingStart,
        caller: &CallerIdentity,
        request: &LaunchRequest,
        connect: impl FnOnce(&LaunchStatus) -> Result<OutputRoute<OutputFrame>, SafeError>,
    ) -> Result<LaunchStatus, SafeError> {
        // The long-lived read guard is acquired only immediately before process
        // construction. Channel/document admission happens before it and may
        // synchronously round-trip through the Tauri main event loop.
        let handoff_guard = RefCell::new(None);
        let spawned = Cell::new(false);
        let status = self.coordinator.start_routed(
            caller,
            request,
            || {
                // Checked inside prepare, so an existing receipt still wins before
                // this readiness gate, profile I/O or any route construction.
                if *self.shutting_down.read() {
                    return Err(error("RUN_SUPERVISOR_STOPPING"));
                }
                self.supervisor
                    .as_ref()
                    .ok_or_else(|| error("NATIVE_RUNTIME_NOT_READY"))?;
                let profile = self.repository.get_profile(&request.profile_id)?;
                let legacy = profile
                    .read_legacy(&self.repository.metadata_directory().join("config.json"))?;
                let inherited = self.inherited_environment()?;
                freeze_launch(
                    request,
                    &profile,
                    caller,
                    &FreezeContext {
                        inherited: &inherited,
                        terminal: &EnvMap::new(),
                        legacy: legacy.as_ref(),
                        observer: None,
                    },
                )
            },
            connect,
            |snapshot| {
                // Observer failure never fails preparation or creates another child.
                // Only the reservation winner can mint this per-run capability.
                let mut observer_failure = None;
                let prepared = self
                    .observer
                    .as_ref()
                    .filter(|_| snapshot.observer_requested())
                    .and_then(|host| {
                        let registry = Arc::downgrade(self.registry());
                        let owner = caller.clone();
                        let request_id = request.request_id.clone();
                        let expected = RunKey {
                            run_id: request.run_id.clone(),
                            generation: request.generation,
                        };
                        let authorize = Arc::new(move || {
                            registry.upgrade().is_some_and(|registry| {
                                registry.status(&owner, &request_id).is_ok_and(|status| {
                                    status.run == expected
                                        && matches!(
                                            status.phase,
                                            LaunchPhase::Starting | LaunchPhase::Running
                                        )
                                })
                            })
                        });
                        match host.prepare(
                            crate::observer_registry::ObserverRun {
                                run_id: request.run_id.clone(),
                                generation: request.generation,
                            },
                            crate::observer_registry::ObserverDelivery::native(
                                &caller.window_label,
                            ),
                            authorize,
                        ) {
                            Ok(prepared) => Some(prepared),
                            Err(failure) => {
                                observer_failure = Some(match failure.code.as_str() {
                                    "OBSERVER_SERVICE_UNAVAILABLE" => {
                                        "OBSERVER_SERVICE_UNAVAILABLE"
                                    }
                                    "OBSERVER_PLUGIN_UNAVAILABLE" => "OBSERVER_PLUGIN_UNAVAILABLE",
                                    _ => "OBSERVER_PREPARE_FAILED",
                                });
                                log::warn!("Observer unavailable; native launch unchanged");
                                None
                            }
                        }
                    });
                let (snapshot, observer) = match prepared {
                    Some(prepared) => {
                        match snapshot.with_observer(&prepared.environment, &prepared.plugin_dir) {
                            Ok(next) => (next, Some(prepared.lease)),
                            Err(_) => {
                                observer_failure = Some("OBSERVER_PREPARE_FAILED");
                                (snapshot.clone(), None)
                            }
                        }
                    }
                    None => (snapshot.clone(), None),
                };
                let shutdown = self.shutting_down.read();
                if *shutdown {
                    return Err(error("RUN_SUPERVISOR_STOPPING"));
                }
                *handoff_guard.borrow_mut() = Some(shutdown);

                let frozen = Arc::new(snapshot);
                let invocation = build_invocation(frozen.request(), &frozen)?;
                let spec = resolve_process(&invocation)?;
                let pty = OwnedPty::spawn_admitted(
                    &spec,
                    PtySize {
                        cols: request.cols,
                        rows: request.rows,
                        pixel_width: 0,
                        pixel_height: 0,
                    },
                    admission.begin_creation(),
                )?;
                spawned.set(true);
                Ok(FrozenPty {
                    pty,
                    snapshot: frozen,
                    observer,
                    observer_failure,
                })
            },
        )?;
        if spawned.get() {
            // Publication precedes external handoff. Even a panic, lost command
            // response or document revocation cannot discard the owned child.
            let resource = self.registry().retained_resource(&status.run)?;
            let supervisor = self
                .supervisor
                .as_ref()
                .expect("prepare required supervisor");
            let registry = self.registry().clone();
            let adopted = catch_unwind(AssertUnwindSafe(|| {
                supervisor.adopt(registry, &status.run, resource)
            }));
            if !matches!(adopted, Ok(Ok(()))) {
                return Err(error("RUN_HANDOFF_FAILED"));
            }
        }
        handoff_guard.borrow_mut().take();
        Ok(status)
    }

    pub(crate) fn begin_shutdown(&self) {
        *self.shutting_down.write() = true;
    }

    /// 普通 run 与一次性 command 分别保有 child；退出必须等待两类 owner 收口。
    pub(crate) fn wait_for_commands(&self) {
        const SHUTDOWN_REAP_TIMEOUT: Duration = Duration::from_secs(5);
        let started = Instant::now();
        let mut active = self.command_owners.active.lock();
        while *active != 0 {
            let remaining = SHUTDOWN_REAP_TIMEOUT.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                log::warn!("native command shutdown timed out with unresolved owners");
                return;
            }
            self.command_owners.settled.wait_for(&mut active, remaining);
        }
    }

    /// 一次性 CLI 子命令：复用启动完全相同的 profile/环境冻结与 argv 构造，但
    /// 不注册 run、不建立 document/输出路由、不返回 tab。用于 `codex unarchive`
    /// 这类必须由 CLI 自己执行的会话状态变更——Desk 不得搬动 CLI 的归档文件。
    /// 子进程仍走 PTY 与既有 admission 门禁，因此窗口/维护冻结语义保持一致；
    /// 输出丢弃，仅以退出状态判定成功。
    pub(crate) fn run_command(
        &self,
        caller: &CallerIdentity,
        request: &LaunchRequest,
    ) -> Result<(), SafeError> {
        self.registry().check_caller(caller)?;
        if *self.shutting_down.read() {
            return Err(error("RUN_SUPERVISOR_STOPPING"));
        }
        let profile = self.repository.get_profile(&request.profile_id)?;
        let legacy =
            profile.read_legacy(&self.repository.metadata_directory().join("config.json"))?;
        let inherited = self.inherited_environment()?;
        let snapshot = freeze_launch(
            request,
            &profile,
            caller,
            &FreezeContext {
                inherited: &inherited,
                terminal: &EnvMap::new(),
                legacy: legacy.as_ref(),
                // 一次性命令不安装 observer：它没有 run 身份可供授权回调核验。
                observer: None,
            },
        )?;
        let invocation = build_invocation(request, &snapshot)?;
        let spec = resolve_process(&invocation)?;
        let mut admission = self.admit_start()?;
        let registry = self.registry().clone();
        let caller = caller.clone();
        let shutting_down = self.shutting_down.clone();
        let command_owners = self.command_owners.clone();
        let size = PtySize {
            cols: request.cols,
            rows: request.rows,
            pixel_width: 0,
            pixel_height: 0,
        };
        let (sender, receiver) = std::sync::mpsc::sync_channel::<Result<(), SafeError>>(1);
        // worker 先建立、child 后创建；线程创建失败时从未产生无 owner 的 child。
        std::thread::Builder::new()
            .name("native-command-owner".into())
            .spawn(move || {
                let mut response = Some(sender);
                let mut reply = |result| {
                    if let Some(sender) = response.take() {
                        let _ = sender.send(result);
                    }
                };
                let shutdown = shutting_down.read();
                if *shutdown {
                    reply(Err(error("RUN_SUPERVISOR_STOPPING")));
                    return;
                }
                // 与 Ticket::begin 相同的提交点：撤销先到则不创建 child；提交后
                // document/回执消失也不撤销后台保有的精确 child。
                if let Err(failure) = registry.check_caller(&caller) {
                    reply(Err(failure));
                    return;
                }
                // read guard 仍在：begin_shutdown 不可能先越过门禁再漏掉此 owner。
                let mut command_owner = match command_owners.register() {
                    Ok(owner) => owner,
                    Err(failure) => {
                        reply(Err(failure));
                        return;
                    }
                };
                let pty = match OwnedPty::spawn_admitted(&spec, size, admission.begin_creation()) {
                    Ok(pty) => pty,
                    Err(failure) => {
                        command_owner.settled = true;
                        reply(Err(failure));
                        return;
                    }
                };
                // shutdown 的读门禁直到此 worker 已保有 child 才释放。
                drop(shutdown);
                let reader_failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let reader_state = reader_failed.clone();
                let mut stopping = match pty.take_reader() {
                    Ok(mut reader) => {
                        match std::thread::Builder::new()
                            .name("native-command-drain".into())
                            .spawn(move || {
                                let mut discard = [0u8; 4096];
                                loop {
                                    match reader.read(&mut discard) {
                                        Ok(0) => break,
                                        Ok(_) => {}
                                        Err(_) => {
                                            reader_state
                                                .store(true, std::sync::atomic::Ordering::Release);
                                            break;
                                        }
                                    }
                                }
                            }) {
                            Ok(_) => false,
                            Err(_) => {
                                reply(Err(error("COMMAND_FAILED")));
                                true
                            }
                        }
                    }
                    Err(failure) => {
                        reply(Err(failure));
                        true
                    }
                };
                let deadline = Instant::now() + Duration::from_secs(30);
                loop {
                    match pty.try_wait() {
                        Ok(Some(status)) => {
                            command_owner.settled = true;
                            reply(if status.success() {
                                Ok(())
                            } else {
                                Err(error("COMMAND_FAILED"))
                            });
                            return;
                        }
                        Ok(None) => {}
                        Err(failure) => {
                            reply(Err(failure));
                            stopping = true;
                        }
                    }
                    if !stopping {
                        let failure = if *shutting_down.read() {
                            Some(error("RUN_SUPERVISOR_STOPPING"))
                        } else if reader_failed.load(std::sync::atomic::Ordering::Acquire) {
                            Some(error("COMMAND_FAILED"))
                        } else if Instant::now() >= deadline {
                            Some(error("COMMAND_TIMEOUT"))
                        } else {
                            None
                        };
                        if let Some(failure) = failure {
                            reply(Err(failure));
                            stopping = true;
                        }
                    }
                    if stopping {
                        // 错误回执不是退出证明。终止/等待失败仍保留原始 handle 与
                        // admission ticket，定间隔重试到真实 reap，不再 blocking wait。
                        let _ = pty.terminate_root();
                    }
                    std::thread::sleep(Duration::from_millis(if stopping { 100 } else { 25 }));
                }
            })
            .map_err(|_| error("PROCESS_START_FAILED"))?;
        receiver.recv().map_err(|_| error("LAUNCH_STATE_UNKNOWN"))?
    }

    pub(crate) fn access(
        self: &Arc<Self>,
        caller: &CallerIdentity,
        run: &RunKey,
    ) -> Result<RunAccess, SafeError> {
        let resource = self.registry().resource(caller, run)?;
        Ok(RunAccess {
            registry: self.registry().clone(),
            caller: caller.clone(),
            run: run.clone(),
            resource,
        })
    }
}

pub(crate) struct RunAccess {
    registry: Arc<RunRegistry<NativeRun>>,
    caller: CallerIdentity,
    run: RunKey,
    resource: Arc<NativeRun>,
}
impl std::fmt::Debug for RunAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunAccess(<redacted>)")
    }
}
impl RunAccess {
    fn check(&self) -> Result<(), SafeError> {
        let current = self.registry.resource(&self.caller, &self.run)?;
        if !Arc::ptr_eq(&current, &self.resource) {
            return Err(error("FORBIDDEN"));
        }
        Ok(())
    }
    pub(crate) fn with_writer<T>(
        &self,
        operation: impl FnOnce(&mut (dyn Write + Send)) -> io::Result<T>,
    ) -> Result<T, SafeError> {
        self.check()?;
        self.resource
            .process
            .pty
            .with_writer(|writer| {
                // Revalidate after waiting for the per-PTY writer, not only before.
                Ok(self
                    .check()
                    .and_then(|()| operation(writer).map_err(|_| error("PTY_WRITE_FAILED"))))
            })
            .map_err(|_| error("PTY_WRITE_FAILED"))?
    }
    pub(crate) fn resize(&self, size: PtySize) -> Result<(), SafeError> {
        self.check()?;
        self.resource
            .process
            .pty
            .resize_checked(size, || self.check())
    }
    pub(crate) fn terminate_root(&self) -> Result<(), SafeError> {
        self.check()?;
        self.resource.process.pty.terminate_root()
    }
    pub(crate) fn snapshot(&self) -> Result<Arc<LaunchSnapshot>, SafeError> {
        self.check()?;
        Ok(self.resource.process.snapshot.clone())
    }
}
