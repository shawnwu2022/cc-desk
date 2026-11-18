//! 首次原生确认前即登记 App 归属；不持密钥、网络或持久写权限。
use super::*;
use crate::remote_host_approval::unattended::{final_approval::FinalApproval, UnattendedApproval};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[derive(Default)]
struct Handles {
    preparation: Option<Arc<UnattendedApproval>>,
    final_approval: Option<Arc<FinalApproval>>,
}
pub(crate) struct PreparingRequest {
    id: String,
    closed: AtomicBool,
    transferred: AtomicBool,
    close_complete: AtomicBool,
    in_flight: AtomicUsize,
    handles: Mutex<Handles>,
    admission: Mutex<Option<crate::run_supervisor::NativeRunAdmission>>,
    resource_outstanding: AtomicBool,
    #[cfg(test)]
    closing_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
pub(crate) struct PreparingOperation {
    request: Arc<PreparingRequest>,
    completed: bool,
    admission: Option<crate::run_supervisor::NativeRunAdmission>,
}
impl PreparingRequest {
    pub(super) fn new(admission: crate::run_supervisor::NativeRunAdmission) -> Arc<Self> {
        Arc::new(Self {
            id: uuid::Uuid::new_v4().to_string(),
            closed: AtomicBool::new(false),
            transferred: AtomicBool::new(false),
            close_complete: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            handles: Mutex::new(Handles::default()),
            admission: Mutex::new(Some(admission)),
            resource_outstanding: AtomicBool::new(true),
            #[cfg(test)]
            closing_hook: Mutex::new(None),
        })
    }
    #[cfg(test)]
    pub(crate) fn test_before_cancel(&self, hook: impl FnOnce() + Send + 'static) {
        *self.closing_hook.lock() = Some(Box::new(hook));
    }
    pub(crate) fn id(&self) -> &str {
        &self.id
    }
    pub(crate) fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub(crate) fn begin(self: &Arc<Self>) -> Result<PreparingOperation, StoreError> {
        let _gate = self.handles.lock();
        if self.closed() || self.transferred.load(Ordering::Acquire) {
            return Err(StoreError::Unauthorized);
        }
        if self
            .in_flight
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(StoreError::Busy);
        }
        Ok(PreparingOperation {
            request: self.clone(),
            completed: false,
            admission: self.admission.lock().take(),
        })
    }
    pub(crate) fn track_preparation(
        &self,
        approval: Arc<UnattendedApproval>,
    ) -> Result<(), StoreError> {
        let mut handles = self.handles.lock();
        if self.closed() || handles.preparation.is_some() {
            drop(handles);
            approval.cancel();
            return Err(StoreError::Unauthorized);
        }
        handles.preparation = Some(approval);
        Ok(())
    }
    pub(crate) fn track_final(&self, approval: Arc<FinalApproval>) -> Result<(), StoreError> {
        let mut handles = self.handles.lock();
        if self.closed() || handles.final_approval.is_some() {
            drop(handles);
            approval.cancel();
            return Err(StoreError::Unauthorized);
        }
        handles.final_approval = Some(approval);
        Ok(())
    }
    pub(crate) fn request_close(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        #[cfg(test)]
        if let Some(hook) = self.closing_hook.lock().take() {
            hook();
        }
        let handles = self.handles.lock();
        let first = handles.preparation.clone();
        let last = handles.final_approval.clone();
        drop(handles);
        if let Some(last) = last {
            last.cancel();
        }
        if let Some(first) = first {
            first.cancel();
        }
        self.release_closed_admission();
        self.close_complete.store(true, Ordering::Release);
    }
    fn release_closed_admission(&self) {
        if !self.closed() {
            return;
        }
        if let Some(mut slot) = self.admission.try_lock() {
            let original = slot.take();
            drop(slot);
            if original.is_some() {
                drop(original);
                self.resource_outstanding.store(false, Ordering::Release);
            }
        }
    }
    pub(super) fn matches_control(
        &self,
        control: &Arc<crate::remote_host_approval::unattended::final_approval::FinalControl>,
    ) -> bool {
        !self.closed()
            && self
                .handles
                .lock()
                .final_approval
                .as_ref()
                .is_some_and(|approval| approval.matches_control(control))
    }
    pub(super) fn transferred(&self) {
        self.transferred.store(true, Ordering::Release);
    }
    pub(super) fn outstanding(&self) -> bool {
        let closed = self.closed();
        self.resource_outstanding.load(Ordering::Acquire)
            || self.in_flight.load(Ordering::Acquire) != 0
            || (closed && !self.close_complete.load(Ordering::Acquire))
            || (!closed && !self.transferred.load(Ordering::Acquire))
    }
}
impl PreparingOperation {
    pub(crate) fn admission(
        &self,
    ) -> Result<&crate::run_supervisor::NativeRunAdmission, StoreError> {
        if self.request.closed() {
            return Err(StoreError::Unauthorized);
        }
        self.admission.as_ref().ok_or(StoreError::Unauthorized)
    }
    pub(crate) fn take_admission(
        &mut self,
    ) -> Result<crate::run_supervisor::NativeRunAdmission, StoreError> {
        if self.request.closed() {
            return Err(StoreError::Unauthorized);
        }
        self.admission.take().ok_or(StoreError::Unauthorized)
    }

    pub(crate) fn complete(&mut self) {
        self.completed = true;
    }
}
impl Drop for PreparingOperation {
    fn drop(&mut self) {
        if !self.completed {
            self.request.request_close();
        }
        let mut slot = self.request.admission.lock();
        if self.request.closed() || self.request.transferred.load(Ordering::Acquire) {
            let original = self.admission.take();
            drop(slot);
            drop(original);
            self.request
                .resource_outstanding
                .store(false, Ordering::Release);
        } else {
            *slot = self.admission.take();
            drop(slot);
            // 关闭可能发生在归还之后；由本操作完成原资源 Drop 后再退在途。
            self.request.release_closed_admission();
        }
        self.request.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}
