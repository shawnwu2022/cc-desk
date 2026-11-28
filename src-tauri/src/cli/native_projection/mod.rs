//! Authenticated, read-only native observations. The CLI remains the configuration authority.
pub(crate) mod cache;
pub(crate) mod catalog;
#[cfg(all(windows, feature = "remote-observer"))]
pub(crate) mod catalogue_history_source;
pub(crate) mod diagnostics;
pub(crate) mod opencode_db;
pub(crate) mod registry;
pub(crate) mod scoped_fs;
pub(crate) mod selection;
pub(crate) mod service;
pub(crate) mod wire;
