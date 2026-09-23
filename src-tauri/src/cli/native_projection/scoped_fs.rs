//! Fail-closed scaffold for the retained-directory reader; implemented after RED.
use std::path::Path;
pub(crate) type ReadResult<T> = Result<T, &'static str>;
#[derive(Clone, Copy)]
pub(crate) struct Limits { pub file_bytes: usize, pub total_bytes: usize, pub entries: usize }
impl Default for Limits { fn default() -> Self { Self { file_bytes: 2*1024*1024, total_bytes: 16*1024*1024, entries: 4096 } } }
pub(crate) struct Budget;
impl Budget { pub(crate) fn new(_: Limits) -> Self { Self } }
pub(crate) struct Root;
pub(crate) struct Entry { pub name: String, pub is_dir: bool, pub is_file: bool }
impl Root {
    pub(crate) fn open(_: &Path) -> ReadResult<Self> { Ok(Self) }
    pub(crate) fn key(&self) -> &str { "unimplemented" }
    pub(crate) fn current(&self) -> ReadResult<()> { Err("SOURCE_READER_NOT_IMPLEMENTED") }
    pub(crate) fn read(&self, _: &Path, _: &mut Budget) -> ReadResult<Option<Vec<u8>>> { Err("SOURCE_READER_NOT_IMPLEMENTED") }
    pub(crate) fn list(&self, _: &Path, _: &mut Budget) -> ReadResult<Vec<Entry>> { Err("SOURCE_READER_NOT_IMPLEMENTED") }
}
#[cfg(test)]
#[path = "../../tests/native_cli_scoped_fs.rs"]
mod tests;
