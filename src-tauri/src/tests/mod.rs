#![allow(non_snake_case)]

#[cfg(test)]
mod checks;
#[cfg(test)]
mod commands;
#[cfg(test)]
mod env;
#[cfg(test)]
mod hook_events;
#[cfg(test)]
pub(crate) mod native_cli_harness;
#[cfg(test)]
mod native_cli_profile_api;
#[cfg(test)]
mod native_cli_profile_edges;
#[cfg(test)]
mod native_cli_profiles;
#[cfg(test)]
mod native_cli_storage;
#[cfg(test)]
mod native_cli_wire;
#[cfg(test)]
mod native_cli_workspace_schema;
#[cfg(test)]
mod platform;
#[cfg(test)]
mod pty;
#[cfg(test)]
mod pty_decoder;
#[cfg(test)]
mod session_name_index;
#[cfg(test)]
mod store;
#[cfg(test)]
mod store_profiling;

#[cfg(all(test, windows))]
mod paste_cli_submit;
#[cfg(test)]
mod paste_framing;
