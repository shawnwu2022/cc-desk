//! Bounded terminal parser adapter for synthetic E04 fixtures.
//!
//! This crate is not a complete terminal emulator. It establishes the OSC
//! trust boundary required before an emulator can consume untrusted PTY bytes.

mod budget;
#[cfg(feature = "experimental-emulator")]
mod emulator;
#[cfg(feature = "experimental-emulator")]
mod encoding;
#[cfg(feature = "experimental-emulator")]
pub use encoding::{EncodingError, SnapshotEncoder};
mod parser;
mod queries;

pub use budget::OSC_PAYLOAD_LIMIT;
#[cfg(feature = "experimental-emulator")]
pub use emulator::{
    BufferKind, CanonicalEvent, CanonicalFeedError, CanonicalQuery, CellRole, MAX_CANONICAL_MARKS,
    MAX_TERMINAL_COLUMNS, MAX_TERMINAL_FEED_BYTES, MAX_TERMINAL_ROWS, MIN_TERMINAL_COLUMNS,
    ProjectionError, RejectedEvents, SemanticCell, SemanticColor, SemanticCursor,
    SemanticCursorShape, SemanticSnapshot, SemanticStyle, TerminalCore, TerminalCoreError,
    TerminalEpochs, TerminalEventSink, TerminalReply, Underline,
};
pub use parser::{TerminalFault, TerminalParser, TerminalSafeCode};
pub use queries::{TerminalQueries, TerminalQuery};
