//! Host components; no listener, endpoint enrollment or terminal admission exists.
pub mod journal;
pub mod journal_digest;
mod journal_storage;
#[cfg(feature = "experimental-local-writer")]
#[allow(
    dead_code,
    reason = "default-off internal prerequisite; no remote IO factory is admitted"
)]
mod local_writer;
mod path_guard;
