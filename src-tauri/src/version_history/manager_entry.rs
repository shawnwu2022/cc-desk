//! Startup grammar only. A canonical transaction selector is not authority;
//! the manager must independently reopen the protected transaction/material.
use crate::cli::{profiles::error, types::SafeError};
use std::ffi::{OsStr, OsString};

const MANAGER_NAME: &str = "cc-desk-version-manager.exe";
pub(crate) struct ManagerRequest {
    transaction_id: String,
}
impl ManagerRequest {
    pub(crate) fn transaction_id(&self) -> &str {
        &self.transaction_id
    }
}
pub(crate) enum DesktopEntryRequest {
    Ordinary,
    Manager(ManagerRequest),
    ManagerReentry,
}

pub(crate) fn observed_request() -> Result<DesktopEntryRequest, SafeError> {
    let image = std::env::current_exe().map_err(|_| error("HISTORY_MANAGER_ENTRY_INVALID"))?;
    let basename = image
        .file_name()
        .ok_or_else(|| error("HISTORY_MANAGER_ENTRY_INVALID"))?;
    classify(basename, &std::env::args_os().collect::<Vec<_>>())
}
/// The basename is observed by the backend from current_exe, never argv[0].
pub(crate) fn classify(
    basename: &OsStr,
    arguments: &[OsString],
) -> Result<DesktopEntryRequest, SafeError> {
    let is_manager = basename
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(MANAGER_NAME));
    if is_manager {
        // Explorer can reopen the independently retained manager without an
        // argument/shortcut. The protected active marker supplies the selector;
        // the current exact copied image is verified before any UI is created.
        if arguments.len() == 1 {
            return Ok(DesktopEntryRequest::ManagerReentry);
        }
        if arguments.len() != 3 || arguments[1] != OsStr::new("--version-manager") {
            return Err(error("HISTORY_MANAGER_ENTRY_INVALID"));
        }
        let transaction_id = arguments[2]
            .to_str()
            .ok_or_else(|| error("HISTORY_MANAGER_ENTRY_INVALID"))?;
        super::journal::validate_id(transaction_id)
            .map_err(|_| error("HISTORY_MANAGER_ENTRY_INVALID"))?;
        return Ok(DesktopEntryRequest::Manager(ManagerRequest {
            transaction_id: transaction_id.into(),
        }));
    }
    if arguments.iter().skip(1).any(|value| {
        value.to_str().is_some_and(|value| {
            value == "--version-manager" || value.starts_with("--version-manager=")
        })
    }) {
        return Err(error("HISTORY_MANAGER_ENTRY_INVALID"));
    }
    Ok(DesktopEntryRequest::Ordinary)
}
