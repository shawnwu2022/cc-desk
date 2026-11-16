//! 委托单代 socket 的原任务托管；仍无完整 receipt 或二代入口。
use super::*;
use crate::remote_host::delegated::DelegatedSocketStart;
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
pub(crate) struct TestMaterialBuilder(
    Box<dyn FnOnce(cc_desk_remote_record::grant::SessionScope) -> wire::WireInputs + Send>,
    Connector,
    #[cfg(test)] TestHooks,
);
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    after_keepalive: Option<Box<dyn FnOnce() + Send>>,
    before_go: Option<Box<dyn FnOnce() + Send>>,
    fail_material_spawn: bool,
}
#[cfg(test)]
impl TestMaterialBuilder {
    pub(crate) fn test_after_keepalive(mut self, hook: impl FnOnce() + Send + 'static) -> Self {
        self.2.after_keepalive = Some(Box::new(hook));
        self
    }
    pub(crate) fn test_before_go(mut self, hook: impl FnOnce() + Send + 'static) -> Self {
        self.2.before_go = Some(Box::new(hook));
        self
    }
    pub(crate) fn test_fail_spawn(mut self) -> Self {
        self.2.fail_material_spawn = true;
        self
    }
    pub(crate) fn new(
        build: impl FnOnce(cc_desk_remote_record::grant::SessionScope) -> TestMaterial + Send + 'static,
    ) -> Self {
        Self(
            Box::new(move |scope| wire::WireInputs::from_test(build(scope))),
            Connector::PublicLoopbackFixture,
            TestHooks::default(),
        )
    }
}
enum Launch {
    Reserved,
    Started,
    Closed,
}
type AdmissionPublisher = watch::Sender<Option<Result<(), HostNetworkError>>>;
pub(crate) struct SocketCustody {
    service: Weak<RemoteHostService>,
    network: Arc<HostNetworkAttempt>,
    material: Arc<MaterialCustody>,
    launch: Mutex<Launch>,
    publication_unknown: AtomicBool,
    published: Mutex<Option<AdmissionPublisher>>,
}
impl SocketCustody {
    pub(crate) fn reserve(service: &Arc<RemoteHostService>) -> Arc<Self> {
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
            service: Arc::downgrade(service),
            network,
            material: Arc::new(MaterialCustody::reserve()),
            launch: Mutex::new(Launch::Reserved),
            publication_unknown: AtomicBool::new(false),
            published: Mutex::new(Some(published)),
        })
    }
    pub(crate) fn request_close(&self) {
        self.network.request_close();
        *self.launch.lock() = Launch::Closed;
    }
    pub(crate) async fn wait_closed(&self) -> Result<(), HostNetworkError> {
        let result = self.network.wait_closed().await;
        if self.publication_unknown.load(Ordering::Acquire) {
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
        build: TestMaterialBuilder,
    ) -> Result<(), HostNetworkError> {
        // 最后 Session 析构会关闭本 custody；必须先持 keepalive，再取得任何锁。
        let keepalive = start.keepalive().ok_or(HostNetworkError::Closed)?;
        #[cfg(test)]
        let mut build = build;
        #[cfg(test)]
        if let Some(hook) = build.2.after_keepalive.take() {
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
        let connector = build.1;
        #[cfg(test)]
        let hooks = build.2;
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
        let task = tauri::async_runtime::spawn(async move {
            let mut wire = wire::NetworkWire::new();
            let mut transferred = false;
            let end = Instant::now() + Duration::from_secs(8);
            let clock = start.clock();
            let initially_closed = *stopped.borrow();
            let setup = async {
                go.await.map_err(|_| HostNetworkError::Unknown)?;
                if initially_closed || !start.starting_open() {
                    return Err(HostNetworkError::Closed);
                }
                original.material.start(move || (build.0)(scope))?;
                let material = loop {
                    if let Some(material) = original.material.take_ready()? {
                        break material;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                };
                transferred = true;
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
                let channel = wire
                    .setup(material, connector, clock.clone(), end, None)
                    .await?;
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
                start
                    .handoff(channel)
                    .map_err(|_| HostNetworkError::Handoff)?;
                clock.now_ms().map_err(|_| HostNetworkError::Authority)?;
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
                }
            } else {
                result
            };
            // 即使 setup 没有产出 channel，也必须回收原材料线程。
            start.close();
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
            service.network.mark_unknown();
            Err(HostNetworkError::Unknown)
        } else {
            Ok(())
        }
    }
}
