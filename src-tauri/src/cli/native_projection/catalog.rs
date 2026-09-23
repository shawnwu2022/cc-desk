//! Supported on-disk projections; never an assertion of live CLI activation.
use super::scoped_fs::{Budget, ReadResult, Root};
use super::wire::{ResourceItem, ResourceKind};
use crate::cli::types::CliKind;
use std::path::PathBuf;
pub(crate) struct Catalog<'a> {
    pub(crate) cli: CliKind,
    pub(crate) root: &'a Root,
    pub(crate) project: Option<&'a Root>,
    pub(crate) project_paths: &'a [PathBuf],
    pub(crate) check: &'a dyn Fn() -> ReadResult<()>,
}
pub(crate) struct Options<'a> {
    pub(crate) kind: ResourceKind,
    pub(crate) query: Option<&'a str>,
    pub(crate) session_id: Option<&'a str>,
}
pub(crate) fn read(_: &Catalog<'_>, _: &Options<'_>, _: &mut Budget) -> ReadResult<Vec<ResourceItem>> {
    Err("SOURCE_READER_NOT_IMPLEMENTED")
}
#[cfg(test)]
#[path = "../../tests/native_cli_projection_reader.rs"]
mod tests;
