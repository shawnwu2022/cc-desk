//! 只读关联观察不是子树锁；未观察到的 writer 冲突由独立原生 helper 处理。
use crate::cli::{profiles::error, types::SafeError};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

const MAX_READS: usize = 128;
const MAX_DEPTH: usize = 64;
fn unavailable() -> SafeError {
    error("ARCHIVE_RELATION_UNAVAILABLE")
}
fn parent(thread: &Value, expected: uuid::Uuid) -> Result<Option<uuid::Uuid>, SafeError> {
    if thread["id"]
        .as_str()
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        != Some(expected)
    {
        return Err(unavailable());
    }
    // 缺字段不等同于根会话；不能使用整棵会话共享的 sessionId 代替 thread.id。
    match thread.get("parentThreadId") {
        Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => uuid::Uuid::parse_str(id)
            .map(Some)
            .map_err(|_| unavailable()),
        _ => Err(unavailable()),
    }
}
fn ancestors(
    id: uuid::Uuid,
    parents: &mut HashMap<uuid::Uuid, Option<uuid::Uuid>>,
    read: &mut dyn FnMut(uuid::Uuid) -> Result<Value, SafeError>,
) -> Result<BTreeSet<uuid::Uuid>, SafeError> {
    let mut result = BTreeSet::new();
    let mut next = Some(id);
    while let Some(id) = next {
        if result.len() >= MAX_DEPTH || !result.insert(id) {
            return Err(unavailable());
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = parents.entry(id) {
            // 总预算包括已缓存的目标，跨 owner 共享，防止宽树放大查询。
            if result.len() > MAX_READS {
                return Err(unavailable());
            }
            entry.insert(parent(&read(id).map_err(|_| unavailable())?, id)?);
        }
        next = parents[&id];
        if parents.len() > MAX_READS {
            return Err(unavailable());
        }
    }
    Ok(result)
}

pub(crate) fn check(
    target: uuid::Uuid,
    thread: &Value,
    owners: &BTreeSet<uuid::Uuid>,
    read: &mut dyn FnMut(uuid::Uuid) -> Result<Value, SafeError>,
) -> Result<(), SafeError> {
    if owners.is_empty() {
        return Ok(());
    }
    if owners.contains(&target) {
        return Err(error("ARCHIVE_BUSY"));
    }
    let mut parents = HashMap::from([(target, parent(thread, target)?)]);
    let mut reads = 0usize;
    let mut bounded_read = |id| {
        if reads >= MAX_READS - 1 {
            return Err(unavailable());
        }
        reads += 1;
        read(id)
    };
    let target_ancestors = ancestors(target, &mut parents, &mut bounded_read)?;
    for owner in owners {
        if target_ancestors.contains(owner)
            || ancestors(*owner, &mut parents, &mut bounded_read)?.contains(&target)
        {
            return Err(error("ARCHIVE_BUSY"));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/native_cli_archive_relations.rs"]
mod tests;
