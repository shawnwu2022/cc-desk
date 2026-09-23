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
mod native_cli_availability;
#[cfg(all(test, windows))]
mod native_cli_channel_live;
#[cfg(test)]
mod native_cli_document;
#[cfg(test)]
mod native_cli_document_edges;
#[cfg(all(test, windows))]
mod native_cli_document_live;
#[cfg(test)]
mod native_cli_document_report;
#[cfg(test)]
mod native_cli_environment;
#[cfg(test)]
pub(crate) mod native_cli_harness;
#[cfg(test)]
mod native_cli_invocation;
#[cfg(test)]
mod native_cli_output_route;
#[cfg(test)]
mod native_cli_owned_pty;
#[cfg(test)]
mod native_cli_platform;
#[cfg(all(test, windows))]
mod native_cli_platform_limits;
#[cfg(test)]
mod native_cli_profile_api;
#[cfg(test)]
mod native_cli_profile_edges;
#[cfg(test)]
mod native_cli_profiles;
#[cfg(test)]
mod native_cli_registry;
#[cfg(test)]
mod native_cli_registry_edges;
#[cfg(test)]
mod native_cli_routed_launch;
#[cfg(test)]
mod native_cli_snapshot;
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
