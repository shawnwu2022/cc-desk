//! Historical-release capability family. No generic updater/install entry point.
//! Commands must use the original authenticated document binding and recheck it
//! after network IO. Tokens alone do not authorize a document or installation.
#![allow(dead_code)] // Authenticated manager/ordinary UI wiring is staged separately.

pub(crate) mod catalog;
pub(crate) mod commands;
pub(crate) mod compatibility;
pub(crate) mod download;
pub(crate) mod journal;
pub(crate) mod maintenance;
pub(crate) mod manager;
pub(crate) mod manager_document;
pub(crate) mod manager_entry;
#[cfg(windows)]
pub(crate) mod manager_runtime;
pub(crate) mod manager_types;
pub(crate) mod payload_policy;
pub(crate) mod policy;
pub(crate) mod snapshot;
pub(crate) mod types;
pub(crate) mod verified_package;
pub(crate) mod windows;
