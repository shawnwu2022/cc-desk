//! Compile the actual capability and parser modules on all three OS families.
//! No substitute implementations, Tauri shims, or private user roots.
#![allow(dead_code)]
#[path = "../../../src-tauri/src/cli/native_projection/catalog.rs"]
mod catalog;
#[path = "../../../src-tauri/src/cli/environment.rs"]
mod environment;
#[path = "../../../src-tauri/src/cli/profiles.rs"]
mod profiles;
#[path = "../../../src-tauri/src/cli/native_projection/registry.rs"]
mod registry;
#[path = "../../../src-tauri/src/cli/native_projection/scoped_fs.rs"]
mod scoped_fs;
#[path = "../../../src-tauri/src/cli/native_projection/selection.rs"]
mod selection;
#[path = "../../../src-tauri/src/cli/types.rs"]
mod types;
#[path = "../../../src-tauri/src/cli/native_projection/wire.rs"]
mod wire;

mod cli {
    pub(crate) use crate::{environment, types};
    #[cfg(test)]
    pub(crate) mod native_projection {
        pub(crate) use crate::wire;
    }
}
