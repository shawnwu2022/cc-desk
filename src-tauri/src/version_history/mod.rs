//! Historical-release capability family. No generic updater/install entry point.
//! Commands must use the original authenticated document binding and recheck it
//! after network IO. Tokens alone do not authorize a document or installation.
#![allow(dead_code)] // Authenticated manager/ordinary UI wiring is staged separately.

#[cfg(all(feature = "history-roundtrip-acceptance", not(debug_assertions)))]
compile_error!("roundtrip acceptance requires debug assertions and cannot ship in release builds");
#[cfg(all(
    feature = "history-roundtrip-acceptance",
    not(all(windows, target_arch = "x86_64"))
))]
compile_error!("roundtrip acceptance is restricted to Windows x64");
#[cfg(any(
    test,
    all(
        feature = "history-roundtrip-acceptance",
        windows,
        target_arch = "x86_64",
        debug_assertions
    )
))]
pub(crate) mod acceptance;

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
#[cfg(windows)]
pub(crate) mod manager_worker;
pub(crate) mod payload_policy;
pub(crate) mod policy;
pub(crate) mod snapshot;
pub(crate) mod types;
pub(crate) mod verified_package;
pub(crate) mod windows;
