//! Prefix-correct canonical query encoding through the locked emulator.
use super::*;
use alacritty_terminal::vte::ansi::Handler;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    PrimaryDa,
    Status,
    Cursor,
}
pub(super) struct QueryMark {
    pub(super) offset: usize,
    pub(super) kind: Kind,
}
pub(super) fn recognize(
    params: &alacritty_terminal::vte::Params,
    intermediates: &[u8],
    action: char,
) -> Option<Kind> {
    if !intermediates.is_empty() || params.iter().count() != 1 {
        return None;
    }
    let param = params.iter().next()?;
    if param.len() != 1 {
        return None;
    }
    match (action, param[0]) {
        ('c', 0) => Some(Kind::PrimaryDa),
        ('n', 5) => Some(Kind::Status),
        ('n', 6) => Some(Kind::Cursor),
        _ => None,
    }
}

#[derive(Default)]
pub(super) struct ReplyCapture {
    pub(super) active: bool,
    pub(super) failed: bool,
    pub(super) bytes: Option<Vec<u8>>,
}

/// No public byte constructor, Clone, Serialize or payload Debug.
pub struct TerminalReply(Vec<u8>);
impl TerminalReply {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Consumed only at the caller's commit point; the borrow prevents state mutation
/// or continuation while a query is waiting to be encoded.
pub struct CanonicalQuery<'a> {
    kind: Kind,
    term: &'a mut Term<TerminalEventSink>,
    sink: &'a TerminalEventSink,
}
impl CanonicalQuery<'_> {
    pub fn encode(self) -> Result<TerminalReply, TerminalFault> {
        let fault = || TerminalFault::Degraded(TerminalSafeCode::UnsupportedSequence);
        {
            let mut capture = self.sink.capture.lock().map_err(|_| fault())?;
            if capture.active {
                return Err(fault());
            }
            capture.active = true;
            capture.failed = false;
            capture.bytes = None;
        }
        // Only these pure, bounded upstream query encoders are callable. Other
        // emulator events retain the default-deny sink and no native effect.
        match self.kind {
            Kind::PrimaryDa => self.term.identify_terminal(None),
            Kind::Status => self.term.device_status(5),
            Kind::Cursor => self.term.device_status(6),
        }
        let mut capture = self.sink.capture.lock().map_err(|_| fault())?;
        capture.active = false;
        let bytes = capture.bytes.take();
        if capture.failed {
            return Err(fault());
        }
        bytes.map(TerminalReply).ok_or_else(fault)
    }
}

pub enum CanonicalEvent<'a> {
    Begin { queries: usize },
    Query(CanonicalQuery<'a>),
}
#[derive(Debug)]
pub enum CanonicalFeedError<E> {
    Terminal(TerminalFault),
    Sink(E),
}

impl TerminalCore {
    /// Explicit experimental profile; ordinary construction remains query-deny.
    pub fn new_with_queries(rows: usize, columns: usize) -> Result<Self, TerminalCoreError> {
        let mut core = Self::new(rows, columns)?;
        core.input_gate.allow_queries = true;
        Ok(core)
    }
    /// Validate the whole batch before effects. Apply canonical prefixes and
    /// temporarily lend each encoder at its exact sequence position. The caller
    /// must validate its capability/fence/stop before consuming the encoder.
    pub fn feed_with_queries<E>(
        &mut self,
        bytes: &[u8],
        mut sink: impl FnMut(CanonicalEvent<'_>) -> Result<(), E>,
    ) -> Result<(), CanonicalFeedError<E>> {
        let marks = self
            .validate_batch(bytes, true)
            .map_err(CanonicalFeedError::Terminal)?;
        // A callback panic must not leave the validator ahead of canonical state
        // while claiming the continuation is usable. Only full success clears it.
        self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
        if let Err(error) = sink(CanonicalEvent::Begin {
            queries: marks.len(),
        }) {
            self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
            return Err(CanonicalFeedError::Sink(error));
        }
        let mut start = 0;
        for mark in marks {
            self.processor
                .advance(&mut self.term, &bytes[start..mark.offset]);
            start = mark.offset;
            let query = CanonicalQuery {
                kind: mark.kind,
                term: &mut self.term,
                sink: &self.event_sink,
            };
            if let Err(error) = sink(CanonicalEvent::Query(query)) {
                self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
                return Err(CanonicalFeedError::Sink(error));
            }
        }
        self.processor.advance(&mut self.term, &bytes[start..]);
        self.degraded = None;
        Ok(())
    }
}
