//! 每代 socket 与原材料任务托管；完整回收事实只由 Session 汇合模块签发。
use super::*;
use crate::remote_host::delegated::{DelegatedAttemptIdentity, DelegatedSocketStart};
#[path = "materials.rs"]
mod materials;
use materials::MaterialCustody;

#[cfg(test)]
pub(crate) struct TestMaterial {
    pub(crate) address: SocketAddr,
    pub(crate) relay: RelayAdmissionIdentity,
    pub(crate) tunnel: [u8; 16],
    pub(crate) local: LocalIdentity,
    pub(crate) phone_pin: PeerPin,
    pub(crate) binding: HostSnapshot,
    pub(crate) browser: BrowserProfile,
    pub(crate) issuer: P256Spki,
    pub(crate) principal: [u8; 32],
    pub(crate) policy: NativeGrantPolicy,
    pub(crate) accepted: Option<Box<TestAcceptedGate>>,
}
pub(crate) struct MaterialSupply {
    build: Box<dyn FnOnce(cc_desk_remote_record::grant::SessionScope) -> wire::WireInputs + Send>,
    connector: Connector,
    end: Option<std::time::Instant>,
    identity: Option<DelegatedAttemptIdentity>,
    #[cfg(test)]
    hooks: TestHooks,
}
#[cfg(test)]
pub(crate) type TestMaterialBuilder = MaterialSupply;
impl MaterialSupply {
    pub(crate) fn delegated(
        start: &DelegatedSocketStart,
        authority: crate::remote_host_approval::unattended::final_approval::DelegatedSigningAuthorization,
        signers: crate::native_crypto::host::HostSignerSet,
    ) -> Result<Self, HostNetworkError> {
        let identity = start.identity();
        if !authority.matches_attempt(&identity)
            || *authority.scope() != start.scope().map_err(|_| HostNetworkError::Authority)?
        {
            return Err(HostNetworkError::Authority);
        }
        #[cfg(test)]
        assert!(
            authority.test_exact_binding(),
            "actual authorization rejects foreign attempt/cap"
        );
        let end = authority.data_end();
        let wire = wire::WireInputs::from_delegated(authority, signers)?;
        Ok(Self {
            build: Box::new(move |_| wire),
            connector: Connector::WebPki,
            end: Some(end),
            identity: Some(identity),
            #[cfg(test)]
            hooks: TestHooks::default(),
        })
    }
}
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    after_keepalive: Option<Box<dyn FnOnce() + Send>>,
    before_go: Option<Box<dyn FnOnce() + Send>>,
    fail_material_spawn: bool,
    before_retirement: Option<(
        tokio::sync::oneshot::Sender<()>,
        tokio::sync::oneshot::Receiver<()>,
    )>,
}
#[cfg(test)]
impl TestMaterialBuilder {
    pub(crate) fn test_loopback_observed(
        mut self,
        accepted: tokio::sync::oneshot::Sender<()>,
    ) -> Self {
        self.connector = Connector::PublicLoopbackFixture;
        let previous = self.build;
        self.build = Box::new(move |scope| previous(scope).test_outer_observer(accepted));
        self
    }
    pub(crate) fn test_activation_hook(
        mut self,
        after: bool,
        hook: impl FnOnce() + Send + 'static,
    ) -> Self {
        let previous = self.build;
        self.build =
            Box::new(move |scope| previous(scope).test_activation_hook(after, Box::new(hook)));
        self
    }
    pub(crate) fn test_live_snapshot_pull(mut self) -> Self {
        let previous = self.build;
        self.build = Box::new(move |scope| previous(scope).test_live_snapshot_pull());
        self
    }
    pub(crate) fn test_live_keepalive(mut self) -> Self {
        let previous = self.build;
        self.build = Box::new(move |scope| previous(scope).test_live());
        self
    }
    pub(crate) fn test_after_keepalive(mut self, hook: impl FnOnce() + Send + 'static) -> Self {
        self.hooks.after_keepalive = Some(Box::new(hook));
        self
    }
    pub(crate) fn test_before_go(mut self, hook: impl FnOnce() + Send + 'static) -> Self {
        self.hooks.before_go = Some(Box::new(hook));
        self
    }
    pub(crate) fn test_hold_retirement(
        mut self,
        entered: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    ) -> Self {
        self.hooks.before_retirement = Some((entered, release));
        self
    }
    pub(crate) fn test_fail_spawn(mut self) -> Self {
        self.hooks.fail_material_spawn = true;
        self
    }
    pub(crate) fn new(
        build: impl FnOnce(cc_desk_remote_record::grant::SessionScope) -> TestMaterial + Send + 'static,
    ) -> Self {
        Self {
            build: Box::new(move |scope| wire::WireInputs::from_test(build(scope))),
            connector: Connector::PublicLoopbackFixture,
            identity: None,
            end: None,
            hooks: TestHooks::default(),
        }
    }
}
enum Launch {
    Reserved,
    Started,
    Closed,
}
type AdmissionPublisher = watch::Sender<Option<Result<(), HostNetworkError>>>;
struct InstalledData {
    identity: DelegatedAttemptIdentity,
    channel: Arc<RunningChannel>,
    budget: crate::remote_unattended::capability::network::NetworkBudget,
    end: std::time::Instant,
}
pub(crate) struct SocketCustody {
    identity: DelegatedAttemptIdentity,
    terminal_clock: Arc<crate::remote_unattended::capability::network::NetworkClock>,
    original_channel: Mutex<Option<Arc<RunningChannel>>>,
    installed_data: Mutex<Option<InstalledData>>,
    service: Weak<RemoteHostService>,
    network: Arc<HostNetworkAttempt>,
    material: Arc<MaterialCustody>,
    launch: Mutex<Launch>,
    publication_unknown: AtomicBool,
    published: Mutex<Option<AdmissionPublisher>>,
}
impl SocketCustody {
    pub(crate) fn reserve(
        service: &Arc<RemoteHostService>,
        start: &DelegatedSocketStart,
    ) -> Arc<Self> {
        let (stop, _) = watch::channel(false);
        let (published, admitted) = watch::channel(None);
        let network = Arc::new(HostNetworkAttempt {
            custody: Arc::downgrade(&service.network),
            stop,
            admitted,
            join: tokio::sync::Mutex::new(None),
            terminal: Arc::new(AtomicU8::new(0)),
        });
        Arc::new(Self {
            identity: start.identity(),
            terminal_clock: start.terminal_clock(),
            original_channel: Mutex::new(None),
            installed_data: Mutex::new(None),
            service: Arc::downgrade(service),
            network,
            material: Arc::new(MaterialCustody::reserve()),
            launch: Mutex::new(Launch::Reserved),
            publication_unknown: AtomicBool::new(false),
            published: Mutex::new(Some(published)),
        })
    }
    pub(crate) fn installed_data_state(&self) -> Option<Result<(), HostNetworkError>> {
        if *self.network.stop.borrow() {
            return Some(Err(HostNetworkError::Closed));
        }
        let installed = self.installed_data.lock();
        let fact = installed.as_ref()?;
        Some((|| {
            if !self.matches_original(&fact.identity, &fact.channel)
                || *self.network.stop.borrow()
                || std::time::Instant::now() >= fact.end
            {
                return Err(HostNetworkError::Closed);
            }
            fact.budget
                .check()
                .map_err(|_| HostNetworkError::Authority)?;
            fact.channel
                .authority()
                .check()
                .map_err(|_| HostNetworkError::Authority)
        })())
    }
    pub(crate) fn check_installed_data(&self) -> Result<(), HostNetworkError> {
        self.installed_data_state()
            .unwrap_or(Err(HostNetworkError::Authority))
    }
    pub(crate) fn matches_identity(&self, identity: &DelegatedAttemptIdentity) -> bool {
        self.identity.matches(identity)
    }
    pub(crate) fn matches_original(
        &self,
        identity: &DelegatedAttemptIdentity,
        channel: &Arc<RunningChannel>,
    ) -> bool {
        self.identity.matches(identity)
            && self
                .original_channel
                .lock()
                .as_ref()
                .is_some_and(|original| Arc::ptr_eq(original, channel))
    }
    pub(crate) fn request_close(&self) {
        self.network.request_close();
        *self.launch.lock() = Launch::Closed;
    }
    pub(crate) async fn wait_closed(&self) -> Result<(), HostNetworkError> {
        let result = self.network.wait_closed().await;
        if self.publication_unknown.load(Ordering::Acquire) || result.is_err() {
            self.terminal_clock.close();
            Err(HostNetworkError::Unknown)
        } else {
            result
        }
    }
    pub(crate) async fn wait_admitted(&self) -> Result<(), HostNetworkError> {
        self.network.wait_admitted().await
    }
    pub(crate) fn start(
        self: &Arc<Self>,
        mut start: DelegatedSocketStart,
        build: MaterialSupply,
    ) -> Result<(), HostNetworkError> {
        // 真实材料必须同时具备同attempt的逐record service准入门。
        if build.identity.is_some() && !start.has_request_gate() {
            return Err(HostNetworkError::Authority);
        }
        // 最后 Session 析构会关闭本 custody；必须先持 keepalive，再取得任何锁。
        if !self.identity.matches(&start.identity())
            || build
                .identity
                .as_ref()
                .is_some_and(|identity| !identity.matches(&start.identity()))
        {
            return Err(HostNetworkError::Authority);
        }
        let keepalive = start.keepalive().ok_or(HostNetworkError::Closed)?;
        // 资源采样与 Owner 预算绑定先于 launch/service 锁，不在锁内回调。
        let budget = start.budget().map_err(|_| HostNetworkError::Authority)?;
        #[cfg(test)]
        let mut build = build;
        #[cfg(test)]
        if let Some(hook) = build.hooks.after_keepalive.take() {
            hook();
        }
        let mut launch = self.launch.lock();
        if !matches!(*launch, Launch::Reserved) || *self.network.stop.borrow() {
            return Err(HostNetworkError::Closed);
        }
        // 尚未对 service 公开，因此登记锁必须在任何 spawn 前取得。
        let mut joined = self
            .network
            .join
            .try_lock()
            .map_err(|_| HostNetworkError::Unknown)?;
        let service = self.service.upgrade().ok_or(HostNetworkError::Closed)?;
        let service_state = service.state.lock();
        if !start.permits_start(&keepalive, &service, &service_state) {
            *launch = Launch::Closed;
            return Err(HostNetworkError::Closed);
        }
        let mut network_state = service.network.open_state()?;
        if network_state.attempts.len() >= 64 {
            return Err(HostNetworkError::Closed);
        }
        *launch = Launch::Started;
        let scope = start.scope().map_err(|_| HostNetworkError::Authority)?;
        let lifetime_end = build
            .end
            .map_or(budget.deadline(), |end| end.min(budget.deadline()));
        let connector = build.connector;
        #[cfg(test)]
        let hooks = build.hooks;
        #[cfg(test)]
        if hooks.fail_material_spawn {
            self.material.test_fail_spawn();
        }
        let original = self.clone();
        let mut stopped = self.network.stop.subscribe();
        let published = self
            .published
            .lock()
            .take()
            .ok_or(HostNetworkError::Unknown)?;
        let (ready, go) = tokio::sync::oneshot::channel();
        #[cfg(test)]
        let retirement_hook = hooks.before_retirement;
        let task = tauri::async_runtime::spawn(async move {
            let mut wire = wire::NetworkWire::new();
            let mut transferred = false;
            let lifetime_end = Instant::from_std(lifetime_end);
            let end = (Instant::now() + Duration::from_secs(8)).min(lifetime_end);
            let clock = budget.clock();
            let initially_closed = *stopped.borrow();
            let setup = async {
                go.await.map_err(|_| HostNetworkError::Unknown)?;
                if initially_closed || !start.starting_open() {
                    return Err(HostNetworkError::Closed);
                }
                original.material.start(move || (build.build)(scope))?;
                let material = loop {
                    if let Some(material) = original.material.take_ready()? {
                        break material;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                };
                transferred = true;
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
                let channel = wire
                    .setup_delegated(material, connector, &budget, end)
                    .await?;
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
                *original.original_channel.lock() = Some(channel.clone());
                let installed_budget = start.budget().map_err(|_| HostNetworkError::Authority)?;
                let installed_channel = channel.clone();
                start
                    .handoff(channel)
                    .map_err(|_| HostNetworkError::Handoff)?;
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
                wire.activate_live_after_install(&budget, end, &original.network.stop)
                    .await?;
                budget.check().map_err(|_| HostNetworkError::Authority)?;
                if let Some(fact) = installed_channel
                    .live_keepalive_fact(security::Role::Host)
                    .map_err(|_| HostNetworkError::Authority)?
                {
                    fact.validate_for(&installed_channel, security::Role::Host)
                        .map_err(|_| HostNetworkError::Authority)?;
                    if !fact.uses_clock(&installed_budget.clock())
                        || *original.network.stop.borrow()
                    {
                        return Err(HostNetworkError::Authority);
                    }
                    *original.installed_data.lock() = Some(InstalledData {
                        identity: original.identity.clone(),
                        channel: installed_channel,
                        budget: installed_budget,
                        end: lifetime_end.into_std(),
                    });
                }
                Ok(())
            };
            let result = {
                let setup = guarded(setup);
                tokio::pin!(setup);
                tokio::select! {biased;
                    _=stopped.changed()=>Err(HostNetworkError::Closed),
                    _=tokio::time::sleep_until(end)=>Err(HostNetworkError::Deadline),
                    result=&mut setup=>result,
                }
            };
            published.send_replace(Some(result));
            let result = if result.is_ok() {
                let channel = wire.running.as_ref().expect("original delegated channel");
                tokio::select! {biased;
                    _=stopped.changed()=>Ok(()),
                    result=channel.wait_closed_checked()=>result.map(|_|()).map_err(|_|HostNetworkError::Unknown),
                    _=tokio::time::sleep_until(lifetime_end)=>{ let _ = budget.check(); Ok(()) },
                    _=async {
                        let mut tick = tokio::time::interval(Duration::from_millis(100));
                        loop { tick.tick().await; if budget.check().is_err() { break; } }
                    }=>Ok(()),
                }
            } else {
                result
            };
            // 即使 setup 没有产出 channel，也必须回收原材料线程。
            original.network.request_close();
            start.close();
            #[cfg(test)]
            if let Some((entered, release)) = retirement_hook {
                let _ = entered.send(());
                let _ = release.await;
            }
            let wire_known = wire.close_and_join().await;
            drop(wire);
            let material_known = if transferred {
                wire_known && original.material.released_by_network().is_ok()
            } else {
                original.material.release_untransferred().await
            };
            let result = if wire_known && material_known {
                result
            } else {
                Err(HostNetworkError::Unknown)
            };
            if result == Err(HostNetworkError::Unknown) {
                original.terminal_clock.close();
            }
            original.network.terminal.store(
                if result == Err(HostNetworkError::Unknown) {
                    2
                } else {
                    1
                },
                Ordering::Release,
            );
            result
        });
        *joined = Some(task);
        // 同一 service/network 门内发布完整原句柄；shutdown 不可能看到空登记。
        network_state.attempts.push(self.network.clone());
        drop(joined);
        drop(network_state);
        drop(service_state);
        drop(launch);
        // 最后外部引用已消失时，此 Drop 先触发关闭，再开 go，不靠调度巧合。
        drop(keepalive);
        #[cfg(test)]
        if let Some(hook) = hooks.before_go {
            hook();
        }
        if ready.send(()).is_err() {
            self.publication_unknown.store(true, Ordering::Release);
            self.terminal_clock.close();
            service.network.mark_unknown();
            Err(HostNetworkError::Unknown)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
impl SocketCustody {
    pub(crate) fn test_original_channel(&self) -> Option<Arc<RunningChannel>> {
        self.original_channel.lock().clone()
    }
}
