//! Private serialized view capture; not a remote ACK or authorization channel.
use super::*;
use cc_desk_remote_terminal::{
    MAX_TERMINAL_COLUMNS, MAX_TERMINAL_ROWS, SemanticCell, SemanticSnapshot,
};
const MAX_VIEWS: usize = 2;
const MAX_VIEW_BYTES: usize = 4 * 1024 * 1024;
const _: () = assert!(
    std::mem::size_of::<SemanticCell>() * MAX_TERMINAL_COLUMNS * MAX_TERMINAL_ROWS
        + std::mem::size_of::<SemanticSnapshot>()
        <= MAX_VIEW_BYTES
);
#[derive(Default)]
struct ViewCredits {
    used: AtomicUsize,
    #[cfg(feature = "experimental-host-owner")]
    _owner: Option<Arc<owner::OwnerActivation>>,
}
/// Created once by a trusted Host owner and shared across its session factories.
/// Resource accounting only; this is not a Host identity or authorization token.
#[derive(Clone)]
pub(crate) struct HostViewBudget(Arc<ViewCredits>);
impl HostViewBudget {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self(Arc::new(ViewCredits::default()))
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(super) fn for_owner(owner: Arc<owner::OwnerActivation>) -> Self {
        Self(Arc::new(ViewCredits {
            used: AtomicUsize::new(0),
            _owner: Some(owner),
        }))
    }
}
struct ViewCredit(Arc<ViewCredits>);
impl ViewCredits {
    fn reserve(self: &Arc<Self>) -> Result<Arc<ViewCredit>, Reject> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(1).filter(|next| *next <= MAX_VIEWS)
            })
            .map_err(|_| Reject::Full)?;
        Ok(Arc::new(ViewCredit(self.clone())))
    }
}
impl Drop for ViewCredit {
    fn drop(&mut self) {
        let previous = self.0.used.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0);
    }
}
pub(crate) struct CapturedProjection {
    target: Arc<Target>,
    writer_epoch: u64,
    through_offset: u64,
    revision: u64,
    view: SemanticSnapshot,
    _credit: Arc<ViewCredit>,
}
/// Host-issued channel barrier context; fixed test bytes in qualification only.
/// Construction does not authenticate a peer or confer remote access.
pub(crate) struct ProjectionContext {
    pub(crate) binding: [u8; 32],
    pub(crate) barrier: [u8; 16],
    pub(crate) sync_epoch: u64,
}
impl CapturedProjection {
    pub(crate) fn encoder(
        &self,
        context: &ProjectionContext,
        minor: u16,
    ) -> Result<cc_desk_remote_terminal::SnapshotEncoder<'_>, cc_desk_remote_terminal::EncodingError>
    {
        use cc_desk_remote_record::projection::{Stamp, Target as WireTarget};
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let e = self.view.epochs();
        cc_desk_remote_terminal::SnapshotEncoder::new(
            &self.view,
            Stamp {
                target: WireTarget {
                    host: hex(&self.target.host),
                    session: hex(&self.target.session),
                    run: self.target.run.clone(),
                    generation: self.target.generation.to_string(),
                    incarnation: self.target.incarnation.to_string(),
                },
                binding: hex(&context.binding),
                barrier: hex(&context.barrier),
                revision: self.revision.to_string(),
                dimension_epoch: e.dimension_epoch.to_string(),
                mode_epoch: e.mode_epoch.to_string(),
                structure_epoch: e.structure_epoch.to_string(),
                sync_epoch: context.sync_epoch.to_string(),
            },
            minor,
        )
    }
    pub(crate) fn target(&self) -> &Target {
        &self.target
    }
    pub(crate) fn writer_epoch(&self) -> u64 {
        self.writer_epoch
    }
    pub(crate) fn through_offset(&self) -> u64 {
        self.through_offset
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn view(&self) -> &SemanticSnapshot {
        &self.view
    }
    pub(super) fn new(
        request: &ProjectionRequest,
        target: Arc<Target>,
        writer_epoch: u64,
        through_offset: u64,
        revision: u64,
        view: SemanticSnapshot,
    ) -> Self {
        Self {
            target,
            writer_epoch,
            through_offset,
            revision,
            view,
            _credit: request.credit.clone(),
        }
    }
}
pub(crate) struct PendingProjection {
    reply: mpsc::Receiver<Result<CapturedProjection, Reject>>,
    _credit: Arc<ViewCredit>,
}
impl PendingProjection {
    pub(crate) fn wait(
        &self,
        budget: Duration,
    ) -> Result<Result<CapturedProjection, Reject>, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
pub(super) struct ProjectionRequest {
    pub(super) context: Arc<()>,
    pub(super) reply: mpsc::SyncSender<Result<CapturedProjection, Reject>>,
    credit: Arc<ViewCredit>,
}
pub(super) struct ProjectionHandle {
    tx: mpsc::SyncSender<Command>,
    context: Arc<()>,
    sealed: Arc<AtomicBool>,
    credits: Arc<ViewCredits>,
}
impl ProjectionHandle {
    pub(super) fn new(input: &LocalInputHandle, context: Arc<()>, budget: HostViewBudget) -> Self {
        Self {
            tx: input.tx.clone(),
            context,
            sealed: input.sealed.clone(),
            credits: budget.0,
        }
    }
    pub(super) fn capture(&self) -> Result<PendingProjection, Reject> {
        if self.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        let credit = self.credits.reserve()?;
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .try_send(Command::Projection(ProjectionRequest {
                context: self.context.clone(),
                reply: tx,
                credit: credit.clone(),
            }))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })?;
        Ok(PendingProjection {
            reply: rx,
            _credit: credit,
        })
    }
}
