//! On-disk observations only. No CLI execution, activation inference, or ambient child paths.
use super::scoped_fs::{Budget, Entry, ReadResult, Root, TitleTail};
use super::wire::{ResourceItem, ResourceKind, TitleSource};
use crate::cli::types::CliKind;
use std::path::{Path, PathBuf};

#[path = "history.rs"]
mod history;
#[path = "metadata.rs"]
mod metadata;

/// 一次观察内单个已注册项目的 admission 身份与已校验路径拼法。
pub(crate) struct ProjectRef<'a> {
    pub(crate) id: &'a str,
    pub(crate) root: &'a Root,
    pub(crate) paths: &'a [PathBuf],
}

pub(crate) struct Catalog<'a> {
    pub(crate) cli: CliKind,
    pub(crate) root: &'a Root,
    /// 空集合 = root-wide 观察（不过滤、无归属）；metadata 类读取只允许恰好一个。
    pub(crate) projects: &'a [ProjectRef<'a>],
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
    // 授权复核语义：read() 入口一次；每个文件观察完成后复核一次（「读后出账
    // 前」防线，撤销后不得让已读内容继续入账）；观察出口由 registry 调用
    // Grant::current 全量复核并丢弃整个观察。目录枚举不产出文件内容，身份由
    // list() 内部的持有句柄 key 校验保证，不再逐目录做 O(项目) 次全量复核——
    // 本机真实规模（~250 个会话目录、8 个注册项目）下那占整个读取的一半以上。
    fn check(&self) -> ReadResult<()> {
        (self.check)()?;
        self.root.current()?;
        for project in self.projects {
            project.root.current()?;
        }
        if let Some(root) = self.user_config {
            root.current()?;
        }
        Ok(())
    }
    fn bytes(&self, root: &Root, path: &str, budget: &mut Budget) -> ReadResult<Option<Vec<u8>>> {
        let bytes = root.read(Path::new(path), budget)?;
        self.check()?;
        Ok(bytes)
    }
    fn entries(&self, root: &Root, path: &str, budget: &mut Budget) -> ReadResult<Vec<Entry>> {
        root.list(Path::new(path), budget)
    }
    fn history_prefix(
        &self,
        path: &str,
        budget: &mut Budget,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        let result = if self.cli == CliKind::Codex {
            self.root.history_header(Path::new(path), budget)?
        } else {
            // Leading metadata and long first prompts can put Claude's cwd
            // beyond a fixed window. Observe complete records adaptively.
            self.root.history_until(
                Path::new(path),
                budget,
                &mut history::claude_metadata_probe(),
            )?
        };
        self.check()?;
        Ok(result)
    }
    fn history_title_tail(
        &self,
        path: &str,
        header_bytes: usize,
        budget: &mut Budget,
    ) -> ReadResult<Option<TitleTail>> {
        let result = self
            .root
            .history_title_tail(Path::new(path), header_bytes, budget)?;
        self.check()?;
        Ok(result)
    }
}
pub(crate) fn read(
    catalog: &Catalog<'_>,
    options: &Options<'_>,
    budget: &mut Budget,
) -> ReadResult<Vec<ResourceItem>> {
    catalog.check()?;
    // 项目文档/技能等资源类别是单项目投影契约；批量观察只服务历史枚举。
    if !matches!(
        options.kind,
        ResourceKind::History | ResourceKind::Messages | ResourceKind::Search
    ) && catalog.projects.len() > 1
    {
        return Err("SOURCE_UNSUPPORTED");
    }
    // opencode 守卫必须在 match 之前：metadata 的 else 分支是 codex_settings()，
    // 若 Opencode 落进 `_` 臂会被当作 Codex 读取 config.toml（对抗审查 S1，静默错径）。
    if catalog.cli == crate::cli::types::CliKind::Opencode {
        if options.kind != ResourceKind::History {
            return Err("SOURCE_UNSUPPORTED");
        }
        let items = super::opencode_db::read_history(catalog.root, catalog.projects)?;
        return Ok(items);
    }
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

#[cfg(test)]
#[path = "../../tests/native_cli_projection_metadata_edges.rs"]
mod metadata_edges;
