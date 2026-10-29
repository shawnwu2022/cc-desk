//! Backend-only output capability and stateless Primary DA profile.
//! No network DTO, raw reply bytes, cursor/size/color claims or user receipts.
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
    pub(crate) fn feed(&mut self, bytes: Vec<u8>) -> Result<PendingOutput, Reject> {
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
    parser: TerminalQueries,
    rate: Rate,
}
impl ReplyState {
    pub(super) fn new(context: Arc<()>) -> Self {
        Self {
            context,
            parser: TerminalQueries::default(),
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
        let queries = match self.parser.feed(&request.bytes) {
            Ok(queries) => queries,
            Err(_) => {
                sealed.store(true, Ordering::Release);
                return OutputResult::Rejected(Reject::Unsupported);
            }
        };
        if !self.rate.admits(queries.len(), Instant::now()) {
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
            self.rate.record(Instant::now());
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
