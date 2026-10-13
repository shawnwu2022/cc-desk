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
use parking_lot::RwLock;
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

pub(crate) struct LaunchService {
    coordinator: LaunchCoordinator<NativeRun>,
    repository: WorkspaceRepository,
    inherited: Option<EnvMap>,
    supervisor: Option<Arc<dyn RunSupervisor>>,
    observer: Option<Arc<crate::observer_host::ObserverHost>>,
    shutting_down: RwLock<bool>,
}
impl LaunchService {
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
            shutting_down: RwLock::new(false),
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
                            Err(_) => {
                                log::warn!("Observer unavailable; native launch unchanged");
                                None
                            }
                        }
                    });
                let (snapshot, observer) = match prepared {
                    Some(prepared) => {
                        match snapshot.with_observer(&prepared.environment, &prepared.plugin_dir) {
                            Ok(next) => (next, Some(prepared.lease)),
                            Err(_) => (snapshot.clone(), None),
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
        if *self.shutting_down.read() {
            return Err(error("RUN_SUPERVISOR_STOPPING"));
        }
        let profile = self.repository.get_profile(&request.profile_id)?;
        let legacy = profile.read_legacy(&self.repository.metadata_directory().join("config.json"))?;
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
        // 子进程的输出必须被读走，否则 PTY 缓冲写满会阻塞它；内容本身无意义。
        if let Ok(mut reader) = pty.take_reader() {
            std::thread::spawn(move || {
                let mut discard = [0u8; 4096];
                while matches!(reader.read(&mut discard), Ok(read) if read > 0) {}
            });
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = pty.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(error("COMMAND_FAILED"))
                };
            }
            if Instant::now() >= deadline {
                // 先终止再回收：留下未知状态的一次性子进程会阻塞后续维护冻结。
                let _ = pty.terminate_root();
                let _ = pty.wait();
                return Err(error("COMMAND_TIMEOUT"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
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
