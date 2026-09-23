//! D11 output-route behavioral scaffold; no active application path.
#![allow(dead_code)]

use super::profiles::error;
use super::types::SafeError;
use tauri::http::HeaderMap;
use tauri::ipc::{Channel, IpcResponse};

pub(crate) const CHANNEL_HEADER: &str = "x-cc-desk-output-channel";
pub(crate) type AuthorityCheck = Box<dyn Fn() -> Result<(), SafeError> + Send + Sync>;

pub(crate) fn parse_channel(_headers: &HeaderMap) -> Result<u32, SafeError> {
    Err(error("OUTPUT_CHANNEL_NOT_IMPLEMENTED"))
}

pub(crate) struct OutputRoutes;

pub(crate) struct OutputRoute<T> {
    _channel: Channel<T>,
}

impl<T> std::fmt::Debug for OutputRoute<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OutputRoute(<redacted>)")
    }
}

impl OutputRoutes {
    pub(crate) fn new(_capacity: usize) -> Self {
        Self
    }

    pub(crate) fn bind<T>(
        &self,
        _id: u32,
        _authorize: AuthorityCheck,
        _create: impl FnOnce() -> Result<Channel<T>, SafeError>,
    ) -> Result<OutputRoute<T>, SafeError> {
        Err(error("OUTPUT_ROUTE_NOT_IMPLEMENTED"))
    }
}

impl<T: IpcResponse> OutputRoute<T> {
    pub(crate) fn send(&self, _value: T) -> Result<(), SafeError> {
        Err(error("OUTPUT_ROUTE_NOT_IMPLEMENTED"))
    }
}
