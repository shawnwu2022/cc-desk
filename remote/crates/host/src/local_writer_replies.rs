//! Backend-only output capability with static DA and opt-in canonical profiles.
//! No network DTO, caller-supplied reply bytes, size/color claims or user receipts.
use super::*;
use cc_desk_remote_terminal::{TerminalQueries, TerminalQuery};
use std::{collections::VecDeque, time::Instant};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OutputResult {
    Processed { replies: u8 },
    Rejected(Reject),
    Partial,
    Unknown,
}
pub(crate) struct PendingOutput {
    reply: mpsc::Receiver<OutputResult>,
    _credit: Arc<Credit>,
}
impl PendingOutput {
    pub(crate) fn wait(&self, budget: Duration) -> Result<OutputResult, WaitError> {
        self.reply.recv_timeout(budget).map_err(|e| match e {
            mpsc::RecvTimeoutError::Timeout => WaitError::Uncertain,
            mpsc::RecvTimeoutError::Disconnected => WaitError::Closed,
        })
    }
}
pub(super) struct OutputRequest {
    context: Arc<()>,
    offset: u64,
    bytes: Box<[u8]>,
    pub(super) reply: mpsc::SyncSender<OutputResult>,
    _credit: Arc<Credit>,
}
/// Minted once by the private terminal factory, not Clone/Serialize/Debug.
/// The identity binds the immutable exact Target and this parser activation.
pub(crate) struct HostOutputHandle {
    tx: mpsc::SyncSender<Command>,
    pub(super) context: Arc<()>,
    sealed: Arc<AtomicBool>,
    credits: Arc<Credits>,
}
impl HostOutputHandle {
    pub(super) fn new(input: &LocalInputHandle, context: Arc<()>) -> Self {
        Self {
            tx: input.tx.clone(),
            context,
            sealed: input.sealed.clone(),
            credits: input.credits.clone(),
        }
    }
    /// Fresh native-reader provenance, never subscriber/display replay. Once a
    /// chunk is queued, lost feedback does not authorize resending its offset.
    pub(crate) fn feed_at(&mut self, offset: u64, bytes: Vec<u8>) -> Result<PendingOutput, Reject> {
        if bytes.is_empty() || bytes.len() > MAX_INPUT {
            return Err(Reject::Bounds);
        }
        if self.sealed.load(Ordering::Acquire) {
            return Err(Reject::Sealed);
        }
        let credit = self.credits.reserve()?;
        let bytes = bytes.as_slice().into();
        let (tx, rx) = mpsc::sync_channel(1);
        let request = OutputRequest {
            context: self.context.clone(),
            offset,
            bytes,
            reply: tx,
            _credit: credit.clone(),
        };
        self.tx
            .try_send(Command::Output(request))
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => Reject::Full,
                mpsc::TrySendError::Disconnected(_) => Reject::Closed,
            })?;
        Ok(PendingOutput {
            reply: rx,
            _credit: credit,
        })
    }
}

#[derive(Default)]
struct Rate {
    commits: VecDeque<Instant>,
}
impl Rate {
    fn prune(&mut self, now: Instant) {
        while self
            .commits
            .front()
            .is_some_and(|then| now.saturating_duration_since(*then) >= Duration::from_secs(1))
        {
            self.commits.pop_front();
        }
    }
    fn admits(&mut self, count: usize, now: Instant) -> bool {
        self.prune(now);
        count <= 32 && self.commits.len() + count <= 32
    }
    fn record(&mut self, now: Instant) {
        self.prune(now);
        debug_assert!(self.commits.len() < 32);
        self.commits.push_back(now);
    }
}
pub(super) struct ReplyState {
    context: Arc<()>,
    next_offset: u64,
    revision: u64,
    parser: ReplyParser,
    rate: Rate,
}
enum ReplyParser {
    Static(Box<TerminalQueries>),
    #[cfg(feature = "experimental-canonical-replies")]
    Canonical(Box<cc_desk_remote_terminal::TerminalCore>),
}
#[cfg(feature = "experimental-canonical-replies")]
pub(crate) struct CanonicalStart {
    core: Box<cc_desk_remote_terminal::TerminalCore>,
    pub(super) budget: super::projection::HostViewBudget,
}
#[cfg(feature = "experimental-canonical-replies")]
impl CanonicalStart {
    pub(crate) fn new(
        rows: usize,
        columns: usize,
        budget: &super::HostViewBudget,
    ) -> Result<Self, Reject> {
        cc_desk_remote_terminal::TerminalCore::new_with_queries(rows, columns)
            .map(|core| Self {
                core: Box::new(core),
                budget: budget.clone(),
            })
            .map_err(|_| Reject::Bounds)
    }
}
impl ReplyState {
    #[cfg(feature = "experimental-host-owner")]
    pub(super) fn resize_baseline(
        &self,
    ) -> Result<((u16, u16), cc_desk_remote_terminal::TerminalEpochs), Reject> {
        let ReplyParser::Canonical(core) = &self.parser else {
            return Err(Reject::Unsupported);
        };
        if core.is_degraded() {
            return Err(Reject::Sync);
        }
        let (rows, columns) = core.dimensions();
        Ok(((rows as u16, columns as u16), core.epochs()))
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(super) fn resize_preflight(
        &self,
        rows: u16,
        columns: u16,
    ) -> Result<(bool, cc_desk_remote_terminal::TerminalEpochs), Reject> {
        let ReplyParser::Canonical(core) = &self.parser else {
            return Err(Reject::Unsupported);
        };
        let size = (usize::from(rows), usize::from(columns));
        let after = core.resize_preflight(size.0, size.1).map_err(|e| match e {
            cc_desk_remote_terminal::TerminalCoreError::EpochOverflow => Reject::Epoch,
            _ => Reject::Bounds,
        })?;
        Ok((core.dimensions() != size, after))
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(super) fn resize_commit(&mut self, rows: u16, columns: u16) -> Result<(), Reject> {
        let ReplyParser::Canonical(core) = &mut self.parser else {
            return Err(Reject::Unsupported);
        };
        core.resize(usize::from(rows), usize::from(columns))
            .map_err(|_| Reject::Continuity)
    }
    #[cfg(feature = "experimental-host-owner")]
    pub(super) fn epochs(&self) -> Option<cc_desk_remote_terminal::TerminalEpochs> {
        match &self.parser {
            ReplyParser::Canonical(core) if !core.is_degraded() => Some(core.epochs()),
            _ => None,
        }
    }
    pub(super) fn new(context: Arc<()>) -> Self {
        Self {
            context,
            next_offset: 0,
            revision: 0,
            parser: ReplyParser::Static(Box::default()),
            rate: Rate::default(),
        }
    }
    #[cfg(feature = "experimental-canonical-replies")]
    pub(super) fn canonical(context: Arc<()>, start: CanonicalStart) -> Self {
        Self {
            context,
            next_offset: 0,
            revision: 0,
            parser: ReplyParser::Canonical(start.core),
            rate: Rate::default(),
        }
    }
    pub(super) fn process(
        &mut self,
        request: &OutputRequest,
        stdin: &mut impl Write,
        sealed: &AtomicBool,
    ) -> OutputResult {
        if !Arc::ptr_eq(&request.context, &self.context) {
            return OutputResult::Rejected(Reject::Epoch);
        }
        if sealed.load(Ordering::Acquire) {
            return OutputResult::Rejected(Reject::Sealed);
        }
        if request.offset != self.next_offset {
            sealed.store(true, Ordering::Release);
            return OutputResult::Rejected(Reject::Continuity);
        }
        let Some(end_offset) = request.offset.checked_add(request.bytes.len() as u64) else {
            sealed.store(true, Ordering::Release);
            return OutputResult::Rejected(Reject::Bounds);
        };
        let Some(revision) = self.revision.checked_add(1) else {
            sealed.store(true, Ordering::Release);
            return OutputResult::Rejected(Reject::Bounds);
        };
        let result = match &mut self.parser {
            ReplyParser::Static(parser) => {
                process_static(parser, &mut self.rate, request, stdin, sealed)
            }
            #[cfg(feature = "experimental-canonical-replies")]
            ReplyParser::Canonical(core) => {
                process_canonical(core, &mut self.rate, request, stdin, sealed)
            }
        };
        if matches!(result, OutputResult::Processed { .. }) {
            self.next_offset = end_offset;
            self.revision = revision;
        }
        result
    }
    #[cfg(feature = "experimental-canonical-replies")]
    pub(super) fn capture(
        &mut self,
        request: &super::projection::ProjectionRequest,
        target: Arc<Target>,
        epoch: u64,
        sealed: &AtomicBool,
    ) -> Result<super::projection::CapturedProjection, Reject> {
        if !Arc::ptr_eq(&request.context, &self.context) {
            return Err(Reject::Epoch);
        }
        if sealed
            .compare_exchange(false, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Reject::Sealed);
        }
        let ReplyParser::Canonical(core) = &mut self.parser else {
            return Err(Reject::Unsupported);
        };
        let view = core.semantic_snapshot().map_err(|_| {
            sealed.store(true, Ordering::Release);
            Reject::Unsupported
        })?;
        Ok(super::projection::CapturedProjection::new(
            request,
            target,
            epoch,
            self.next_offset,
            self.revision,
            view,
        ))
    }
}
fn process_static(
    parser: &mut TerminalQueries,
    rate: &mut Rate,
    request: &OutputRequest,
    stdin: &mut impl Write,
    sealed: &AtomicBool,
) -> OutputResult {
    let queries = match parser.feed(&request.bytes) {
        Ok(queries) => queries,
        Err(_) => {
            sealed.store(true, Ordering::Release);
            return OutputResult::Rejected(Reject::Unsupported);
        }
    };
    if !rate.admits(queries.len(), Instant::now()) {
        sealed.store(true, Ordering::Release);
        return OutputResult::Rejected(Reject::Rate);
    }
    let mut replies = 0;
    for query in queries {
        // RMW is the commit point relative to seal, even though it leaves
        // active=false unchanged. Later seal cannot undo already committed IO.
        if sealed
            .compare_exchange(false, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return if replies == 0 {
                OutputResult::Rejected(Reject::Sealed)
            } else {
                OutputResult::Partial
            };
        }
        // Only static identity is supported. No stale sampled canonical state
        // is carried from parse time; stateful replies require a later contract.
        let bytes = match query {
            TerminalQuery::PrimaryDeviceAttributes => b"\x1b[?6c",
        };
        rate.record(Instant::now());
        match write_once(stdin, bytes) {
            Outcome::HostWritten => replies += 1,
            Outcome::Partial => {
                sealed.store(true, Ordering::Release);
                return OutputResult::Partial;
            }
            _ => {
                sealed.store(true, Ordering::Release);
                return OutputResult::Unknown;
            }
        }
    }
    OutputResult::Processed { replies }
}

#[cfg(feature = "experimental-canonical-replies")]
fn process_canonical(
    core: &mut cc_desk_remote_terminal::TerminalCore,
    rate: &mut Rate,
    request: &OutputRequest,
    stdin: &mut impl Write,
    sealed: &AtomicBool,
) -> OutputResult {
    use cc_desk_remote_terminal::{CanonicalEvent, CanonicalFeedError};
    let mut replies = 0;
    let result = core.feed_with_queries(&request.bytes, |event| match event {
        CanonicalEvent::Begin { queries } => {
            if !rate.admits(queries, Instant::now()) {
                sealed.store(true, Ordering::Release);
                Err(OutputResult::Rejected(Reject::Rate))
            } else {
                Ok(())
            }
        }
        CanonicalEvent::Query(query) => {
            if sealed
                .compare_exchange(false, false, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(if replies == 0 {
                    OutputResult::Rejected(Reject::Sealed)
                } else {
                    OutputResult::Partial
                });
            }
            // No parse-time response bytes. Canonical prefix state is exclusively
            // borrowed and sampled by the mature encoder after this commit point.
            let reply = query.encode().map_err(|_| {
                sealed.store(true, Ordering::Release);
                if replies == 0 {
                    OutputResult::Rejected(Reject::Unsupported)
                } else {
                    OutputResult::Partial
                }
            })?;
            rate.record(Instant::now());
            match write_once(stdin, reply.as_bytes()) {
                Outcome::HostWritten => {
                    replies += 1;
                    Ok(())
                }
                Outcome::Partial => {
                    sealed.store(true, Ordering::Release);
                    Err(OutputResult::Partial)
                }
                _ => {
                    sealed.store(true, Ordering::Release);
                    Err(OutputResult::Unknown)
                }
            }
        }
    });
    match result {
        Ok(()) => OutputResult::Processed { replies },
        Err(CanonicalFeedError::Sink(result)) => result,
        Err(CanonicalFeedError::Terminal(_)) => {
            sealed.store(true, Ordering::Release);
            OutputResult::Rejected(Reject::Unsupported)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "experimental-canonical-replies")]
    #[test]
    fn projection_semantic_fault_seals_the_actual_worker_without_a_valid_frame() {
        let dir = tempfile::tempdir().unwrap();
        let mut start = CanonicalStart::new(3, 10, &super::super::HostViewBudget::new()).unwrap();
        assert!(start.core.feed(b"\x1b]52;c;x\x07").is_err());
        let target = Target {
            host: [1; 32],
            session: [2; 16],
            run: "PUBLIC_TEST_ONLY_CAPTURE".into(),
            generation: 1,
            incarnation: 1,
        };
        let (mut writer, _) = LocalWriter::start_canonical_terminal(
            Journal::open(&dir.path().join("journal.sqlite")).unwrap(),
            Vec::<u8>::new(),
            target,
            [3; 32],
            1,
            DigestKey::from_backend_bytes([0x5a; 32]),
            start,
        )
        .unwrap();
        assert!(matches!(
            writer.capture().unwrap().wait(Duration::from_secs(2)),
            Ok(Err(Reject::Unsupported))
        ));
        assert!(writer.handle.sealed.load(Ordering::Acquire));
        let end = Instant::now() + Duration::from_secs(2);
        while !writer.try_join().unwrap() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn output_offset_overflow_seals_before_parsing_or_native_io() {
        let context = Arc::new(());
        let mut state = ReplyState::new(context.clone());
        state.next_offset = u64::MAX - 1;
        let (reply, _) = mpsc::sync_channel(1);
        let request = OutputRequest {
            context,
            offset: u64::MAX - 1,
            bytes: b"\x1b[".as_slice().into(),
            reply,
            _credit: Arc::new(Credits::default()).reserve().unwrap(),
        };
        let sealed = AtomicBool::new(false);
        let mut observed = Vec::new();
        assert_eq!(
            state.process(&request, &mut observed, &sealed),
            OutputResult::Rejected(Reject::Bounds)
        );
        assert!(observed.is_empty());
        assert!(sealed.load(Ordering::Acquire));
        assert_eq!(state.next_offset, u64::MAX - 1);
        assert_eq!(
            state.process(&request, &mut observed, &sealed),
            OutputResult::Rejected(Reject::Sealed)
        );
    }
    #[test]
    fn projection_revision_overflow_cannot_parse_or_write_another_batch() {
        let context = Arc::new(());
        let mut state = ReplyState::new(context.clone());
        state.revision = u64::MAX;
        let (reply, _) = mpsc::sync_channel(1);
        let request = OutputRequest {
            context,
            offset: 0,
            bytes: b"\x1b[c".as_slice().into(),
            reply,
            _credit: Arc::new(Credits::default()).reserve().unwrap(),
        };
        let sealed = AtomicBool::new(false);
        let mut observed = Vec::new();
        assert_eq!(
            state.process(&request, &mut observed, &sealed),
            OutputResult::Rejected(Reject::Bounds)
        );
        assert!(sealed.load(Ordering::Acquire));
        assert!(observed.is_empty());
        assert_eq!(state.next_offset, 0);
        assert_eq!(state.revision, u64::MAX);
    }
    #[test]
    fn reply_rate_does_not_reset_early_or_with_a_regressed_sample() {
        let now = Instant::now();
        let mut rate = Rate::default();
        for _ in 0..32 {
            rate.record(now);
        }
        assert!(!rate.admits(1, now + Duration::from_nanos(999_999_999)));
        assert!(!rate.admits(1, now - Duration::from_millis(1)));
        assert!(rate.admits(1, now + Duration::from_secs(1)));
        rate.record(now + Duration::from_secs(1));
        assert!(!rate.admits(32, now + Duration::from_secs(1)));
        assert!(rate.admits(31, now + Duration::from_secs(1)));
    }
}
