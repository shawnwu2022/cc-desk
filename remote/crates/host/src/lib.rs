//! Host components; default-off native adapter, no listener or endpoint enrollment.
#[cfg(all(test, feature = "experimental-host-owner"))]
#[path = "local_writer_authorized_test_support.rs"]
pub(crate) mod bound_test_support;
pub mod journal;
pub mod journal_digest;
mod journal_storage;
#[cfg(feature = "experimental-host-owner")]
#[allow(dead_code)]
mod lifecycle_router;
#[cfg(feature = "experimental-local-writer")]
#[allow(
    dead_code,
    reason = "default-off internal prerequisite; no remote IO factory is admitted"
)]
mod local_writer;
mod path_guard;
