//! Prefix-correct canonical query encoding through the locked emulator.
use super::*;
use alacritty_terminal::vte::ansi::Handler;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    PrimaryDa,
    Status,
    Cursor,
}
#[derive(Clone, Copy)]
pub(super) enum MarkKind {
    Query(Kind),
    Mode(ModeMutation),
}
#[derive(Clone, Copy)]
pub(super) enum ModeMutation {
    Focus(bool),
    Alternate(bool),
}
impl ModeMutation {
    fn predict(
        self,
        mut mode: alacritty_terminal::term::TermMode,
    ) -> alacritty_terminal::term::TermMode {
        use alacritty_terminal::term::TermMode;
        match self {
            Self::Focus(enabled) => mode.set(TermMode::FOCUS_IN_OUT, enabled),
            Self::Alternate(enabled) => mode.set(TermMode::ALT_SCREEN, enabled),
        }
        mode
    }
    fn structural(self) -> bool {
        matches!(self, Self::Alternate(_))
    }
}
pub(super) struct CanonicalMark {
    pub(super) offset: usize,
    pub(super) kind: MarkKind,
}
pub(super) fn mode_mutation(
    params: &alacritty_terminal::vte::Params,
    intermediates: &[u8],
    action: char,
) -> Option<ModeMutation> {
    if intermediates != b"?" || !matches!(action, 'h' | 'l') || params.iter().count() != 1 {
        return None;
    }
    match params.iter().next()? {
        [1004] => Some(ModeMutation::Focus(action == 'h')),
        [1049] => Some(ModeMutation::Alternate(action == 'h')),
        _ => None,
    }
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
        sink: impl FnMut(CanonicalEvent<'_>) -> Result<(), E>,
    ) -> Result<(), CanonicalFeedError<E>> {
        self.feed_internal(bytes, true, sink)
    }
    pub(super) fn feed_internal<E>(
        &mut self,
        bytes: &[u8],
        emit_queries: bool,
        mut sink: impl FnMut(CanonicalEvent<'_>) -> Result<(), E>,
    ) -> Result<(), CanonicalFeedError<E>> {
        let marks = self
            .validate_batch(bytes)
            .map_err(CanonicalFeedError::Terminal)?;
        self.preflight_epochs(&marks)
            .map_err(CanonicalFeedError::Terminal)?;
        // A callback panic must not leave the validator ahead of canonical state
        // while claiming the continuation is usable. Only full success clears it.
        self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
        if let Err(error) = sink(CanonicalEvent::Begin {
            queries: marks
                .iter()
                .filter(|mark| matches!(mark.kind, MarkKind::Query(_)))
                .count(),
        }) {
            self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
            return Err(CanonicalFeedError::Sink(error));
        }
        let mut start = 0;
        for mark in marks {
            let before = *self.term.mode();
            self.processor
                .advance(&mut self.term, &bytes[start..mark.offset]);
            start = mark.offset;
            match mark.kind {
                MarkKind::Mode(mutation) => {
                    let after = *self.term.mode();
                    if after != mutation.predict(before) {
                        return Err(self.mode_drift());
                    }
                    if before != after {
                        self.epochs.mode_epoch += 1; // exact preflight proved the full count fits
                        if mutation.structural() {
                            self.epochs.structure_epoch += 1;
                        }
                    }
                }
                MarkKind::Query(kind) => {
                    if *self.term.mode() != before {
                        return Err(self.mode_drift());
                    }
                    if emit_queries {
                        let query = CanonicalQuery {
                            kind,
                            term: &mut self.term,
                            sink: &self.event_sink,
                        };
                        if let Err(error) = sink(CanonicalEvent::Query(query)) {
                            return Err(CanonicalFeedError::Sink(error));
                        }
                    }
                }
            }
        }
        let before = *self.term.mode();
        self.processor.advance(&mut self.term, &bytes[start..]);
        if *self.term.mode() != before {
            return Err(self.mode_drift());
        }
        self.degraded = None;
        Ok(())
    }
    fn preflight_epochs(&mut self, marks: &[CanonicalMark]) -> Result<(), TerminalFault> {
        let mut mode = *self.term.mode();
        let mut mode_changes = 0u64;
        let mut structural_changes = 0u64;
        for mark in marks {
            if let MarkKind::Mode(mutation) = mark.kind {
                let next = mutation.predict(mode);
                if mode != next {
                    mode_changes += 1;
                    if mutation.structural() {
                        structural_changes += 1;
                    }
                }
                mode = next;
            }
        }
        if self.epochs.mode_epoch.checked_add(mode_changes).is_none()
            || self
                .epochs
                .structure_epoch
                .checked_add(structural_changes)
                .is_none()
        {
            self.degraded = Some(TerminalSafeCode::StateVersionExceeded);
            return Err(TerminalFault::Degraded(
                TerminalSafeCode::StateVersionExceeded,
            ));
        }
        Ok(())
    }
    fn mode_drift<E>(&mut self) -> CanonicalFeedError<E> {
        self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
        CanonicalFeedError::Terminal(TerminalFault::Degraded(
            TerminalSafeCode::UnsupportedSequence,
        ))
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;
    #[test]
    fn epoch_overflow_refuses_the_whole_batch_before_even_a_prior_query_callback() {
        for structural in [false, true] {
            let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
            if structural {
                core.epochs.structure_epoch = u64::MAX;
            } else {
                core.epochs.mode_epoch = u64::MAX;
            }
            let wire: &[u8] = if structural {
                b"\x1b[c\x1b[?1049h"
            } else {
                b"\x1b[c\x1b[?1004h"
            };
            let mut callbacks = 0;
            assert!(
                core.feed_with_queries(wire, |_| {
                    callbacks += 1;
                    Ok::<(), ()>(())
                })
                .is_err()
            );
            assert_eq!(callbacks, 0);
            assert!(core.is_degraded());
            assert_eq!(core.row_text(0).as_deref(), Some(""));
        }
    }
    #[test]
    fn no_op_mode_requests_at_the_last_epoch_do_not_wrap_or_falsely_advance() {
        let mut core = TerminalCore::new_with_queries(3, 10).unwrap();
        core.epochs.mode_epoch = u64::MAX;
        core.epochs.structure_epoch = u64::MAX;
        core.feed(b"\x1b[?1004l\x1b[?1049l\x1b[?9001h\x1b[1t")
            .unwrap();
        assert_eq!(core.epochs.mode_epoch, u64::MAX);
        assert_eq!(core.epochs.structure_epoch, u64::MAX);
        assert!(!core.is_degraded());
    }
    #[test]
    fn resize_epoch_overflow_freezes_without_changing_the_grid() {
        for structural in [false, true] {
            let mut core = TerminalCore::new(3, 10).unwrap();
            core.feed(b"base").unwrap();
            if structural {
                core.epochs.structure_epoch = u64::MAX;
            } else {
                core.epochs.dimension_epoch = u64::MAX;
            }
            assert_eq!(
                core.resize_preflight(4, 12),
                Err(TerminalCoreError::EpochOverflow)
            );
            assert_eq!(core.dimensions(), (3, 10));
            assert!(!core.is_degraded());
            assert_eq!(core.resize(4, 12), Err(TerminalCoreError::EpochOverflow));
            assert_eq!(core.term.screen_lines(), 3);
            assert_eq!(core.term.columns(), 10);
            assert_eq!(core.row_text(0).as_deref(), Some("base"));
            assert!(core.is_degraded());
            assert_eq!(core.resize(3, 10), Err(TerminalCoreError::Degraded));
        }
    }
}
