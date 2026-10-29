//! Bounded terminal parser adapter for synthetic E04 fixtures.
//!
//! This crate is not a complete terminal emulator. It establishes the OSC
//! trust boundary required before an emulator can consume untrusted PTY bytes.

mod budget;
#[cfg(feature = "experimental-emulator")]
mod emulator;
mod parser;
mod queries;

pub use budget::OSC_PAYLOAD_LIMIT;
#[cfg(feature = "experimental-emulator")]
pub use emulator::{
    CanonicalEvent, CanonicalFeedError, CanonicalQuery, MAX_TERMINAL_COLUMNS,
    MAX_TERMINAL_FEED_BYTES, MAX_TERMINAL_ROWS, RejectedEvents, TerminalCore, TerminalCoreError,
    TerminalEventSink, TerminalReply,
};
pub use parser::{TerminalFault, TerminalParser, TerminalSafeCode};
pub use queries::{TerminalQueries, TerminalQuery};
