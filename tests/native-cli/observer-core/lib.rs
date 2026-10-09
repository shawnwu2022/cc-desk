//! Production observer modules without the GUI runtime. No implementation copies.
#![allow(dead_code, non_snake_case)]
#[path = "../../../src-tauri/src/hook_config.rs"]
mod hook_config;
#[path = "../../../src-tauri/src/hook_events.rs"]
mod hook_events;
#[path = "../../../src-tauri/src/observer_registry.rs"]
mod observer_registry;
#[path = "../../../src-tauri/src/cli/profiles.rs"]
mod profiles;
#[path = "../../../src-tauri/src/cli/types.rs"]
mod types;
mod cli {
    pub(crate) use crate::{environment, profiles, types};
}
#[path = "../../../src-tauri/src/cli/environment.rs"]
mod environment;
#[cfg(test)]
#[path = "../../../src-tauri/src/tests/native_cli_environment.rs"]
mod environment_tests;
#[cfg(test)]
#[path = "../../../src-tauri/src/tests/native_cli_observer_http.rs"]
mod http_tests;
#[path = "../../../src-tauri/src/observer_host.rs"]
mod observer_host;
#[path = "../../../src-tauri/src/observer_http.rs"]
mod observer_http;
#[cfg(test)]
#[path = "../../../src-tauri/src/tests/native_cli_observer.rs"]
mod tests;
