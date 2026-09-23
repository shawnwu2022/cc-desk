//! On-disk observations only. No CLI execution, activation inference, or ambient child paths.
use super::scoped_fs::{Budget, Entry, ReadResult, Root};
use super::wire::{ResourceItem, ResourceKind};
use crate::cli::types::CliKind;
use std::path::{Path, PathBuf};

#[path = "history.rs"]
mod history;
#[path = "metadata.rs"]
mod metadata;

pub(crate) struct Catalog<'a> {
    pub(crate) cli: CliKind,
    pub(crate) root: &'a Root,
    pub(crate) project: Option<&'a Root>,
    pub(crate) project_paths: &'a [PathBuf],
    // Default-Claude's sibling config is a separately admitted, fixed-file read.
    pub(crate) user_config: Option<&'a Root>,
    pub(crate) check: &'a dyn Fn() -> ReadResult<()>,
}
pub(crate) struct Options<'a> {
    pub(crate) kind: ResourceKind,
    pub(crate) query: Option<&'a str>,
    pub(crate) session_id: Option<&'a str>,
}
impl Catalog<'_> {
    fn check(&self) -> ReadResult<()> {
        (self.check)()?;
        self.root.current()?;
        if let Some(root) = self.project {
            root.current()?;
        }
        if let Some(root) = self.user_config {
            root.current()?;
        }
        Ok(())
    }
    fn bytes(&self, root: &Root, path: &str, budget: &mut Budget) -> ReadResult<Option<Vec<u8>>> {
        self.check()?;
        let bytes = root.read(Path::new(path), budget)?;
        self.check()?;
        Ok(bytes)
    }
    fn entries(&self, root: &Root, path: &str, budget: &mut Budget) -> ReadResult<Vec<Entry>> {
        self.check()?;
        let entries = root.list(Path::new(path), budget)?;
        self.check()?;
        Ok(entries)
    }
}
pub(crate) fn read(
    catalog: &Catalog<'_>,
    options: &Options<'_>,
    budget: &mut Budget,
) -> ReadResult<Vec<ResourceItem>> {
    catalog.check()?;
    let result = match options.kind {
        ResourceKind::History | ResourceKind::Messages | ResourceKind::Search => {
            history::read(catalog, options, budget)
        }
        _ => metadata::read(catalog, options.kind, budget),
    }?;
    catalog.check()?;
    budget.checkpoint()?;
    Ok(result)
}
fn text(bytes: &[u8]) -> ReadResult<&str> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SOURCE_INVALID_TEXT")?;
    if text.contains('\0') {
        return Err("SOURCE_INVALID_TEXT");
    }
    Ok(text)
}
fn validate_value(value: &serde_json::Value) -> ReadResult<()> {
    match value {
        serde_json::Value::String(s) if s.contains('\0') => Err("SOURCE_INVALID_TEXT"),
        serde_json::Value::Array(values) => values.iter().try_for_each(validate_value),
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if key.contains('\0') {
                    return Err("SOURCE_INVALID_TEXT");
                }
                validate_value(value)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}
fn bounded(text: &str, maximum: usize) -> (String, bool) {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), end < text.len())
}
fn child(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}
#[cfg(test)]
#[path = "../../tests/native_cli_projection_reader.rs"]
mod tests;
