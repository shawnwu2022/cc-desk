//! 每代材料的原线程托管。生成完成、移交和释放是不同事实，不产生批准。
use super::*;
use std::thread::JoinHandle;

enum Phase {
    Reserved,
    Running(JoinHandle<wire::WireInputs>),
    Ready(Option<Box<wire::WireInputs>>),
    Transferred,
    Releasing(JoinHandle<()>),
    Released,
    NotSpawned,
    Unknown,
    Quarantined {
        _payload: std::mem::ManuallyDrop<Box<dyn std::any::Any + Send>>,
    },
}
pub(super) struct MaterialCustody {
    phase: Mutex<Phase>,
    #[cfg(test)]
    fail_spawn: AtomicBool,
}
impl MaterialCustody {
    #[cfg(test)]
    pub(super) fn test_fail_spawn(&self) {
        self.fail_spawn.store(true, Ordering::Release);
    }
    pub(super) fn reserve() -> Self {
        Self {
            phase: Mutex::new(Phase::Reserved),
            #[cfg(test)]
            fail_spawn: AtomicBool::new(false),
        }
    }
    // 生产材料供应器尚无入口。线程先等待门，原句柄登记之后才构造材料。
    pub(super) fn start(
        &self,
        build: impl FnOnce() -> wire::WireInputs + Send + 'static,
    ) -> Result<(), HostNetworkError> {
        let mut phase = self.phase.lock();
        if !matches!(*phase, Phase::Reserved) {
            return Err(HostNetworkError::Unknown);
        }
        #[cfg(test)]
        if self.fail_spawn.load(Ordering::Acquire) {
            *phase = Phase::Unknown;
            return Err(HostNetworkError::Unknown);
        }
        let (ready, start) = std::sync::mpsc::sync_channel(0);
        let worker = match std::thread::Builder::new()
            .name("delegated-material".into())
            .spawn(move || {
                start.recv().expect("registered original material worker");
                build()
            }) {
            Ok(worker) => worker,
            Err(_) => {
                *phase = Phase::Unknown;
                return Err(HostNetworkError::Unknown);
            }
        };
        *phase = Phase::Running(worker);
        if ready.send(()).is_err() {
            // 保留原句柄，后续仍须 join 其终局。
            return Err(HostNetworkError::Unknown);
        }
        Ok(())
    }
    pub(super) fn take_ready(&self) -> Result<Option<wire::WireInputs>, HostNetworkError> {
        let mut phase = self.phase.lock();
        if matches!(&*phase, Phase::Running(worker) if worker.is_finished()) {
            let Phase::Running(worker) = std::mem::replace(&mut *phase, Phase::Unknown) else {
                unreachable!()
            };
            // is_finished 只决定何时 join，结果必须来自原句柄。
            *phase = match worker.join() {
                Ok(material) => Phase::Ready(Some(Box::new(material))),
                Err(payload) => Phase::Quarantined {
                    _payload: std::mem::ManuallyDrop::new(payload),
                },
            };
        }
        match &mut *phase {
            Phase::Running(_) | Phase::Reserved => Ok(None),
            Phase::Ready(material) => {
                let material = material.take().ok_or(HostNetworkError::Unknown)?;
                *phase = Phase::Transferred;
                Ok(Some(*material))
            }
            _ => Err(HostNetworkError::Unknown),
        }
    }
    pub(super) fn released_by_network(&self) -> Result<(), HostNetworkError> {
        let mut phase = self.phase.lock();
        if !matches!(*phase, Phase::Transferred) {
            return Err(HostNetworkError::Unknown);
        }
        *phase = Phase::Released;
        Ok(())
    }
    // 取消发生在材料移交前：迟到材料也必须由原释放线程析构并 join。
    pub(super) async fn release_untransferred(&self) -> bool {
        loop {
            {
                let mut phase = self.phase.lock();
                if matches!(&*phase,Phase::Running(worker) if worker.is_finished()) {
                    let Phase::Running(worker) = std::mem::replace(&mut *phase, Phase::Unknown)
                    else {
                        unreachable!()
                    };
                    *phase = match worker.join() {
                        Ok(material) => Phase::Ready(Some(Box::new(material))),
                        Err(payload) => Phase::Quarantined {
                            _payload: std::mem::ManuallyDrop::new(payload),
                        },
                    };
                }
                if let Phase::Ready(material) = &mut *phase {
                    let material = material.take();
                    let (ready, start) = std::sync::mpsc::sync_channel(0);
                    match std::thread::Builder::new()
                        .name("delegated-release".into())
                        .spawn(move || {
                            start.recv().expect("registered original release worker");
                            drop(material);
                        }) {
                        Ok(worker) => {
                            *phase = Phase::Releasing(worker);
                            let _ = ready.send(());
                        }
                        Err(_) => {
                            *phase = Phase::Unknown;
                        }
                    }
                }
                if matches!(&*phase,Phase::Releasing(worker) if worker.is_finished()) {
                    let Phase::Releasing(worker) = std::mem::replace(&mut *phase, Phase::Unknown)
                    else {
                        unreachable!()
                    };
                    *phase = match worker.join() {
                        Ok(()) => Phase::Released,
                        Err(payload) => Phase::Quarantined {
                            _payload: std::mem::ManuallyDrop::new(payload),
                        },
                    };
                }
                if matches!(*phase, Phase::Reserved) {
                    *phase = Phase::NotSpawned;
                }
                match &*phase {
                    Phase::Released | Phase::NotSpawned => return true,
                    Phase::Unknown
                    | Phase::Quarantined { .. }
                    | Phase::Transferred
                    | Phase::Reserved => return false,
                    _ => {}
                }
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
}
