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

#[cfg(test)]
mod paste_framing;
#[cfg(all(test, windows))]
mod paste_cli_submit;
