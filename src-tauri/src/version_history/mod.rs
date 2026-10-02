//! Historical-release capability family. No generic updater/install entry point.
//! Commands must use the original authenticated document binding and recheck it
//! after network IO. Tokens alone do not authorize a document or installation.
#![allow(dead_code)] // Authenticated manager/ordinary UI wiring is staged separately.

pub(crate) mod catalog;
pub(crate) mod policy;
pub(crate) mod types;
pub(crate) mod download;
pub(crate) mod verified_package;
