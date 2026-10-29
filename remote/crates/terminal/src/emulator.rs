use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::Term;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::{Config, Osc52};
use alacritty_terminal::vte::Perform;
use alacritty_terminal::vte::ansi::Processor;
use unicode_width::UnicodeWidthChar;

use crate::{TerminalFault, TerminalParser, TerminalSafeCode};
#[path = "canonical.rs"]
mod canonical;
pub use canonical::{CanonicalEvent, CanonicalFeedError, CanonicalQuery, TerminalReply};
use canonical::{CanonicalMark, MarkKind, ReplyCapture};
#[path = "projection.rs"]
mod projection;
pub use projection::{
    BufferKind, CellRole, ProjectionError, SemanticCell, SemanticColor, SemanticCursor,
    SemanticCursorShape, SemanticSnapshot, SemanticStyle, Underline,
};

pub const MAX_TERMINAL_COLUMNS: usize = 240;
pub const MIN_TERMINAL_COLUMNS: usize = alacritty_terminal::term::MIN_COLUMNS;
pub const MAX_TERMINAL_ROWS: usize = 100;
pub const MAX_TERMINAL_FEED_BYTES: usize = 16_384;
pub const MAX_CANONICAL_MARKS: usize = 256;
/// Local terminal counters only; Host peer sync/barrier authority is separate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalEpochs {
    pub dimension_epoch: u64,
    pub mode_epoch: u64,
    pub structure_epoch: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCoreError {
    Degraded,
    EpochOverflow,
    ZeroRows,
    ZeroColumns,
    ColumnsBelowMinimum { requested: usize, minimum: usize },
    RowsExceeded { requested: usize, maximum: usize },
    ColumnsExceeded { requested: usize, maximum: usize },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RejectedEvents {
    pub clipboard: u64,
    pub pty_writes: u64,
    pub native_effects: u64,
}

#[derive(Default)]
struct EventCounters {
    clipboard: AtomicU64,
    pty_writes: AtomicU64,
    native_effects: AtomicU64,
}

/// A fail-closed listener which drops events except a scoped pure query encoding.
///
/// Outside that private scope it records counters only. The bounded query scope
/// retains at most one ten-byte response. No PTY or clipboard API is connected.
#[derive(Clone, Default)]
pub struct TerminalEventSink {
    counters: Arc<EventCounters>,
    capture: Arc<Mutex<ReplyCapture>>,
}

impl TerminalEventSink {
    pub fn rejected_events(&self) -> RejectedEvents {
        RejectedEvents {
            clipboard: self.counters.clipboard.load(Ordering::Relaxed),
            pty_writes: self.counters.pty_writes.load(Ordering::Relaxed),
            native_effects: self.counters.native_effects.load(Ordering::Relaxed),
        }
    }
}

impl EventListener for TerminalEventSink {
    fn send_event(&self, event: Event) {
        let counter = match event {
            Event::ClipboardStore(..) | Event::ClipboardLoad(..) => &self.counters.clipboard,
            Event::PtyWrite(bytes) => {
                if let Ok(mut capture) = self.capture.lock()
                    && capture.active
                {
                    if capture.bytes.is_some() || bytes.len() > 10 {
                        capture.failed = true;
                    } else {
                        capture.bytes = Some(bytes.into_bytes());
                    }
                    return;
                }
                &self.counters.pty_writes
            }
            Event::MouseCursorDirty | Event::CursorBlinkingChange | Event::Wakeup => return,
            Event::Title(..)
            | Event::ResetTitle
            | Event::ColorRequest(..)
            | Event::TextAreaSizeRequest(..)
            | Event::Bell
            | Event::Exit
            | Event::ChildExit(..) => &self.counters.native_effects,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct TerminalSize {
    rows: usize,
    columns: usize,
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Experimental, software-only terminal state.
///
/// This slice has no PTY, renderer, user account, or native event handlers.
/// It deliberately configures zero scrollback history until the complete
/// history and allocation budgets are enforced.
pub struct TerminalCore {
    validator: TerminalParser,
    input_gate: RestrictedInputGate,
    degraded: Option<TerminalSafeCode>,
    processor: Processor,
    term: Term<TerminalEventSink>,
    event_sink: TerminalEventSink,
    epochs: TerminalEpochs,
}

impl fmt::Debug for TerminalCore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TerminalCore")
            .field("rows", &self.term.screen_lines())
            .field("columns", &self.term.columns())
            .field("degraded", &self.is_degraded())
            .finish_non_exhaustive()
    }
}

impl TerminalCore {
    pub fn new(rows: usize, columns: usize) -> Result<Self, TerminalCoreError> {
        let size = validate_size(rows, columns)?;
        let event_sink = TerminalEventSink::default();
        let config = Config {
            scrolling_history: 0,
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let term = Term::new(config, &size, event_sink.clone());

        Ok(Self {
            validator: TerminalParser::new(),
            input_gate: RestrictedInputGate::default(),
            degraded: None,
            processor: Processor::new(),
            term,
            event_sink,
            epochs: TerminalEpochs::default(),
        })
    }

    /// Validate a complete batch before applying it to the emulator.
    ///
    /// The independent validator is persistent across calls. If an OSC exceeds
    /// the patched 4096-byte bound, the core becomes degraded and the failing
    /// batch is never applied to the Alacritty state.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), TerminalFault> {
        match self.feed_internal(bytes, false, |_| Ok::<(), std::convert::Infallible>(())) {
            Ok(()) => Ok(()),
            Err(CanonicalFeedError::Terminal(error)) => Err(error),
            Err(CanonicalFeedError::Sink(impossible)) => match impossible {},
        }
    }

    fn validate_batch(&mut self, bytes: &[u8]) -> Result<Vec<CanonicalMark>, TerminalFault> {
        if let Some(code) = self.degraded {
            return Err(TerminalFault::Degraded(code));
        }
        if bytes.len() > MAX_TERMINAL_FEED_BYTES {
            self.degraded = Some(TerminalSafeCode::InputBatchExceeded);
            return Err(TerminalFault::Degraded(
                TerminalSafeCode::InputBatchExceeded,
            ));
        }

        self.input_gate.marks.clear();
        self.input_gate.query_count = 0;
        let blocked_osc52 = self.validator.blocked_osc52_count();
        let validation = (|| {
            for (index, byte) in bytes.iter().enumerate() {
                self.input_gate.offset = index + 1;
                self.validator
                    .feed(&mut self.input_gate, std::slice::from_ref(byte))?;
            }
            Ok(())
        })();
        if let Err(error @ TerminalFault::Degraded(code)) = validation {
            self.degraded = Some(code);
            return Err(error);
        }

        let rejected = if self.validator.take_string_sequence_entered()
            || self.validator.blocked_osc52_count() != blocked_osc52
        {
            Some(TerminalSafeCode::UnsupportedSequence)
        } else {
            self.input_gate.take_rejection()
        };
        if let Some(code) = rejected {
            self.degraded = Some(code);
            return Err(TerminalFault::Degraded(code));
        }

        Ok(std::mem::take(&mut self.input_gate.marks))
    }

    pub fn resize(&mut self, rows: usize, columns: usize) -> Result<(), TerminalCoreError> {
        if self.is_degraded() {
            return Err(TerminalCoreError::Degraded);
        }
        let size = validate_size(rows, columns)?;
        if rows == self.term.screen_lines() && columns == self.term.columns() {
            return Ok(());
        }
        let epochs = match (
            self.epochs.dimension_epoch.checked_add(1),
            self.epochs.structure_epoch.checked_add(1),
        ) {
            (Some(dimension_epoch), Some(structure_epoch)) => TerminalEpochs {
                dimension_epoch,
                structure_epoch,
                ..self.epochs
            },
            _ => {
                self.degraded = Some(TerminalSafeCode::StateVersionExceeded);
                return Err(TerminalCoreError::EpochOverflow);
            }
        };
        self.degraded = Some(TerminalSafeCode::UnsupportedSequence);
        self.term.resize(size);
        self.epochs = epochs;
        self.degraded = None;
        Ok(())
    }
    pub fn epochs(&self) -> TerminalEpochs {
        self.epochs
    }

    /// Read-only backend access for local diagnostics and projection work.
    pub fn term(&self) -> &Term<TerminalEventSink> {
        &self.term
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&Cell> {
        if row >= self.term.screen_lines() || column >= self.term.columns() {
            return None;
        }

        let row = i32::try_from(row).ok()?;
        Some(&self.term.grid()[Line(row)][Column(column)])
    }

    pub fn row_text(&self, row: usize) -> Option<String> {
        (row < self.term.screen_lines()).then(|| {
            (0..self.term.columns())
                .map(|column| self.cell(row, column).expect("validated grid coordinate").c)
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
    }

    pub fn rejected_events(&self) -> RejectedEvents {
        self.event_sink.rejected_events()
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.is_some() || self.validator.is_degraded()
    }

    pub fn blocked_osc52_count(&self) -> u64 {
        self.validator.blocked_osc52_count()
    }

    /// Remote control is intentionally ineligible for this experimental slice.
    pub fn remote_control_eligible(&self) -> bool {
        false
    }
}

fn validate_size(rows: usize, columns: usize) -> Result<TerminalSize, TerminalCoreError> {
    if rows == 0 {
        return Err(TerminalCoreError::ZeroRows);
    }
    if columns == 0 {
        return Err(TerminalCoreError::ZeroColumns);
    }
    if columns < MIN_TERMINAL_COLUMNS {
        return Err(TerminalCoreError::ColumnsBelowMinimum {
            requested: columns,
            minimum: MIN_TERMINAL_COLUMNS,
        });
    }
    if rows > MAX_TERMINAL_ROWS {
        return Err(TerminalCoreError::RowsExceeded {
            requested: rows,
            maximum: MAX_TERMINAL_ROWS,
        });
    }
    if columns > MAX_TERMINAL_COLUMNS {
        return Err(TerminalCoreError::ColumnsExceeded {
            requested: columns,
            maximum: MAX_TERMINAL_COLUMNS,
        });
    }

    Ok(TerminalSize { rows, columns })
}

#[derive(Default)]
struct RestrictedInputGate {
    rejected: Option<TerminalSafeCode>,
    allow_queries: bool,
    offset: usize,
    query_count: usize,
    marks: Vec<CanonicalMark>,
}

impl RestrictedInputGate {
    fn mark(&mut self, kind: MarkKind) {
        if self.marks.len() == MAX_CANONICAL_MARKS {
            self.reject(TerminalSafeCode::CanonicalEventBudgetExceeded);
        } else {
            self.marks.push(CanonicalMark {
                offset: self.offset,
                kind,
            });
        }
    }
    fn conpty_compatibility_control(
        params: &alacritty_terminal::vte::Params,
        intermediates: &[u8],
        action: char,
    ) -> bool {
        // Exact, reviewed startup subset only. CSI1t is intentionally inert in
        // the locked Processor (never a native window activation). Focus is a
        // bounded TermMode flag. Win32-input-mode is explicitly unsupported:
        // Microsoft's ConPTY spec permits terminals to ignore this optional
        // request and retain legacy VT input; do not claim KEY_EVENT fidelity.
        (intermediates.is_empty() && action == 't' && params.iter().eq([&[1][..]]))
            || (intermediates == b"?"
                && matches!(action, 'h' | 'l')
                && (params.iter().eq([&[1004][..]]) || params.iter().eq([&[9001][..]])))
    }
    fn reject(&mut self, code: TerminalSafeCode) {
        self.rejected.get_or_insert(code);
    }

    fn take_rejection(&mut self) -> Option<TerminalSafeCode> {
        self.rejected.take()
    }

    fn allowed_csi(
        params: &alacritty_terminal::vte::Params,
        intermediates: &[u8],
        action: char,
    ) -> bool {
        if intermediates.is_empty() && action == 'm' {
            // Only fixed-size cell colors/flags; no extended colors, subparameters,
            // strings or native effects enter this deliberately narrow profile.
            return params.iter().count() <= 16
                && params.iter().all(|param| {
                    param.len() == 1
                        && matches!(param[0], 0..=4 | 7..=9 | 22..=24 | 27..=29
                            | 30..=37 | 39..=47 | 49 | 90..=97 | 100..=107)
                });
        }
        if intermediates == b"?" && matches!(action, 'h' | 'l') {
            return params.iter().eq([&[1049][..]]);
        }

        intermediates.is_empty()
            && action == 'H'
            && params.iter().count() <= 2
            && params.iter().all(|param| param.len() == 1)
    }
}

impl Perform for RestrictedInputGate {
    fn print(&mut self, character: char) {
        match character.width() {
            Some(0) => self.reject(TerminalSafeCode::CombiningScalar),
            Some(_) => {}
            None => self.reject(TerminalSafeCode::UnsupportedSequence),
        }
    }

    fn execute(&mut self, byte: u8) {
        if !matches!(byte, b'\r' | b'\n') {
            self.reject(TerminalSafeCode::UnsupportedSequence);
        }
    }

    fn hook(
        &mut self,
        _params: &alacritty_terminal::vte::Params,
        _intermediates: &[u8],
        _ignore: bool,
        _action: char,
    ) {
        self.reject(TerminalSafeCode::UnsupportedSequence);
    }

    fn put(&mut self, _byte: u8) {
        self.reject(TerminalSafeCode::UnsupportedSequence);
    }

    fn unhook(&mut self) {
        self.reject(TerminalSafeCode::UnsupportedSequence);
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        self.reject(TerminalSafeCode::UnsupportedSequence);
    }

    fn csi_dispatch(
        &mut self,
        params: &alacritty_terminal::vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if self.allow_queries && !ignore {
            if let Some(kind) = canonical::recognize(params, intermediates, action) {
                if self.query_count == 32 {
                    self.reject(TerminalSafeCode::ReplyQueryBudgetExceeded);
                } else {
                    self.query_count += 1;
                    self.mark(MarkKind::Query(kind));
                }
                return;
            }
            if Self::conpty_compatibility_control(params, intermediates, action) {
                if let Some(mode) = canonical::mode_mutation(params, intermediates, action) {
                    self.mark(MarkKind::Mode(mode));
                }
                return;
            }
        }
        if ignore || !Self::allowed_csi(params, intermediates, action) {
            self.reject(TerminalSafeCode::UnsupportedSequence);
        } else if let Some(mode) = canonical::mode_mutation(params, intermediates, action) {
            self.mark(MarkKind::Mode(mode));
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {
        self.reject(TerminalSafeCode::UnsupportedSequence);
    }
}
