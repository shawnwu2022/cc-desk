//! Compile the actual capability and parser modules on all three OS families.
//! No substitute implementations, Tauri shims, or private user roots.
#![allow(dead_code)]
#[path = "../../../src-tauri/src/cli/native_projection/cache.rs"]
mod cache;
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

#[path = "../../../src-tauri/src/cli/native_rename.rs"]
mod native_rename;
#[path = "../../../src-tauri/src/cli/projection.rs"]
mod projection;
#[path = "../../../src-tauri/src/session_index_maintenance.rs"]
mod session_index_maintenance;
#[path = "../../../src-tauri/src/session_name_index.rs"]
mod session_name_index;
#[path = "../../../src-tauri/src/cli/source_scope.rs"]
mod source_scope;
mod snapshot {
    include!(concat!(env!("OUT_DIR"), "/snapshot.rs"));
}
mod store {
    include!(concat!(env!("OUT_DIR"), "/store.rs"));
}
mod platform {
    include!(concat!(env!("OUT_DIR"), "/platform.rs"));
}
mod version_history {
    #[cfg(windows)]
    pub(crate) mod windows {
        include!(concat!(env!("OUT_DIR"), "/windows.rs"));
    }
}

mod native_projection {
    #[allow(unused_imports)]
    pub(crate) use crate::{cache, registry, scoped_fs, wire};
}
mod cli {
    #[allow(unused_imports)]
    pub(crate) use crate::{
        environment, native_projection, native_rename, profiles, projection, snapshot, types,
    };
}
