use super::*;
use crate::version_history::windows::files::{
    ComponentName, Directory, FileAccess, FileIdentity, PinnedFile,
};
use std::{ffi::OsStr, io::Write};
const CONTAINER: &str = ".ccdesk-recovery-v1";
const PAYLOAD: &str = "transcript.jsonl";
const MAXIMUM: usize = 128;
fn fail() -> SafeError {
    error("RECYCLE_UNAVAILABLE")
}
fn name(value: &str) -> Result<ComponentName, SafeError> {
    ComponentName::new(OsStr::new(value)).map_err(|_| fail())
}
fn root(scope: &Scope) -> Result<Arc<Directory>, SafeError> {
    scope.current()?;
    let directory = Directory::open_absolute(scope.root.selected_path()).map_err(|_| fail())?;
    scope.current()?;
    Ok(directory)
}
fn folder(root: &Arc<Directory>, value: &str) -> Result<Arc<Directory>, SafeError> {
    root.open_directory(name(value)?).map_err(|_| fail())
}
fn source_parent(
    root: &Arc<Directory>,
    relative: &str,
    id: &str,
) -> Result<(Arc<Directory>, ComponentName), SafeError> {
    let parts: Vec<_> = relative.split('/').collect();
    if parts.len() != 3
        || parts[0] != "projects"
        || parts[2] != format!("{id}.jsonl")
        || uuid::Uuid::parse_str(id).is_err()
    {
        return Err(fail());
    }
    let project = folder(&folder(root, "projects")?, parts[1])?;
    Ok((project, name(parts[2])?))
}
fn verify_transcript(file: &PinnedFile, id: &str, cwd: &str) -> Result<(), SafeError> {
    let bytes = file.read_prefix(2 * 1024 * 1024).map_err(|_| fail())?;
    let mut has_id = false;
    let mut has_cwd = false;
    for line in bytes
        .split_inclusive(|b| *b == b'\n')
        .filter(|line| line.ends_with(b"\n"))
    {
        let value: serde_json::Value = serde_json::from_slice(line).map_err(|_| fail())?;
        if let Some(found) = value.get("sessionId") {
            if found.as_str() != Some(id) {
                return Err(fail());
            }
            has_id = true;
        }
        if let Some(found) = value["cwd"].as_str() {
            // 与历史 / rename 契约一致：首个 cwd 决定归属，后续工作目录变化不改来源。
            if !has_cwd && found != cwd {
                return Err(fail());
            }
            has_cwd = true;
        }
    }
    if !has_id || !has_cwd {
        return Err(fail());
    }
    Ok(())
}
pub(crate) struct Target {
    parent: Arc<Directory>,
    name: ComponentName,
    identity: FileIdentity,
    relative: String,
    id: String,
    cwd: String,
}
pub(crate) fn prepare_target(
    scope: &Scope,
    relative: &str,
    id: &str,
    cwd: &str,
) -> Result<Target, SafeError> {
    if !scope.permits(cwd) {
        return Err(error("FORBIDDEN"));
    }
    let root = root(scope)?;
    let (parent, name) = source_parent(&root, relative, id)?;
    let file = parent
        .open_file(name.clone(), FileAccess::Read)
        .map_err(|_| error("SOURCE_BUSY"))?;
    verify_transcript(&file, id, cwd)?;
    scope.current()?;
    Ok(Target {
        parent,
        name,
        identity: file.identity().clone(),
        relative: relative.into(),
        id: id.into(),
        cwd: cwd.into(),
    })
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    source_key: String,
    relative: String,
    native_id: String,
    cwd: String,
    identity: FileIdentity,
}
fn container(scope: &Scope, create: bool) -> Result<Option<Arc<Directory>>, SafeError> {
    let root = root(scope)?;
    match root.open_directory(name(CONTAINER)?) {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !create => Ok(None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let cap = root.capability().map_err(|_| fail())?;
            cap.create_dir(CONTAINER).map_err(|_| fail())?;
            Ok(Some(folder(&root, CONTAINER)?))
        }
        Err(_) => Err(fail()),
    }
}
fn read_journal(scope: &Scope, operation: &Arc<Directory>) -> Result<Option<Journal>, SafeError> {
    let file = match operation.open_file(name("record.json")?, FileAccess::Read) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(fail()),
    };
    let bytes = file.read_prefix(65537).map_err(|_| fail())?;
    if bytes.len() > 65536 {
        return Err(fail());
    }
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| fail())?;
    if journal.version != 1
        || journal.source_key != scope.root.key()
        || !scope.permits(&journal.cwd)
        || uuid::Uuid::parse_str(&journal.native_id).is_err()
    {
        return Err(error("FORBIDDEN"));
    }
    source_parent(&root(scope)?, &journal.relative, &journal.native_id)?;
    Ok(Some(journal))
}
fn observe(
    parent: &Arc<Directory>,
    name: ComponentName,
    identity: &FileIdentity,
) -> Option<(bool, bool)> {
    match parent.open_file(name, FileAccess::Read) {
        Ok(file) => Some((true, file.identity() == identity)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some((false, false)),
        Err(_) => None,
    }
}
fn inspect(
    scope: &Scope,
    id: &str,
    operation: &Arc<Directory>,
    journal: &Journal,
) -> Result<Record, SafeError> {
    let (parent, original) = source_parent(&root(scope)?, &journal.relative, &journal.native_id)?;
    let original = observe(&parent, original, &journal.identity);
    let payload = observe(operation, name(PAYLOAD)?, &journal.identity);
    let state = match (original, payload) {
        (Some((_, false)), Some((true, true))) => "stored",
        (Some((true, true)), Some((false, false))) => "source",
        _ => "unknown",
    };
    scope.current()?;
    Ok(Record {
        operation_id: id.into(),
        native_session_id: journal.native_id.clone(),
        project_path: journal.cwd.clone(),
        state,
        original_occupied: original.is_none_or(|v| v.0),
        scope: "claude-root-transcript",
    })
}
pub(crate) fn list(scope: &Scope) -> Result<Response, SafeError> {
    let Some(container) = container(scope, false)? else {
        return Ok(Response {
            records: vec![],
            unavailable_count: 0,
        });
    };
    let mut records = Vec::new();
    let (entries, rejected) = container
        .read_recovery_child_names(MAXIMUM)
        .map_err(|_| fail())?;
    let mut unavailable_count = u32::try_from(rejected).map_err(|_| fail())?;
    for entry in entries {
        let id = entry.os_string().into_string().map_err(|_| fail())?;
        if uuid::Uuid::parse_str(&id).is_err() {
            unavailable_count += 1;
            continue;
        }
        let operation = match folder(&container, &id) {
            Ok(value) => value,
            Err(_) => {
                unavailable_count += 1;
                continue;
            }
        };
        let journal = match read_journal(scope, &operation) {
            Ok(Some(journal)) => journal,
            Ok(None) => continue,
            // 同源其他注册项目的有效记录不向本项目 scope 发布。
            Err(e) if e.code == "FORBIDDEN" => continue,
            Err(_) => {
                unavailable_count += 1;
                continue;
            }
        };
        match inspect(scope, &id, &operation, &journal) {
            Ok(record) => records.push(record),
            Err(_) => unavailable_count += 1,
        }
    }
    scope.current()?;
    records.sort_by(|a, b| a.operation_id.cmp(&b.operation_id));
    Ok(Response {
        records,
        unavailable_count,
    })
}
pub(crate) fn remove(scope: &Scope, id: &str, target: Target) -> Result<Response, SafeError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error("RECYCLE_INVALID_REQUEST"));
    }
    scope.current()?;
    let container = container(scope, true)?.ok_or_else(fail)?;
    // 同一 operationId 的不确定结果只检查，不重新移动原路径上的后继对象。
    if let Ok(operation) = container.open_directory(name(id)?) {
        let journal = read_journal(scope, &operation)?.ok_or_else(fail)?;
        if journal.relative != target.relative
            || journal.native_id != target.id
            || journal.cwd != target.cwd
        {
            return Err(error("RECYCLE_OPERATION_CONFLICT"));
        }
        return Ok(Response {
            unavailable_count: 0,
            records: vec![inspect(scope, id, &operation, &journal)
                .map_err(|_| error("RECYCLE_OUTCOME_UNKNOWN"))?],
        });
    }
    let mut file = target
        .parent
        .open_file(target.name, FileAccess::ExclusiveRename)
        .map_err(|_| error("SOURCE_BUSY"))?;
    if file.identity() != &target.identity {
        return Err(error("SOURCE_CHANGED"));
    }
    verify_transcript(&file, &target.id, &target.cwd)?;
    let (entries, rejected) = container
        .read_recovery_child_names(MAXIMUM)
        .map_err(|_| fail())?;
    if entries.len() + rejected >= MAXIMUM {
        return Err(error("RECYCLE_CAPACITY"));
    }
    let cap = container.capability().map_err(|_| fail())?;
    cap.create_dir(id)
        .map_err(|_| error("RECYCLE_OPERATION_CONFLICT"))?;
    let operation = folder(&container, id)?;
    let journal = Journal {
        version: 1,
        source_key: scope.root.key().into(),
        relative: target.relative,
        native_id: target.id,
        cwd: target.cwd,
        identity: target.identity,
    };
    let cap = operation.capability().map_err(|_| fail())?;
    let mut options = cap_std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    // 部分写入只留在临时文件；只有同步完成后才原子发布可读取的日志。
    let mut record = cap.open_with("intent.tmp", &options).map_err(|_| fail())?;
    let bytes = serde_json::to_vec(&journal).map_err(|_| fail())?;
    let split = bytes.len() / 2;
    record.write_all(&bytes[..split]).map_err(|_| fail())?;
    checkpoint("journal-write")?;
    record.write_all(&bytes[split..]).map_err(|_| fail())?;
    record.sync_all().map_err(|_| fail())?;
    drop(record);
    let mut complete = operation
        .open_file(name("intent.tmp")?, FileAccess::ExclusiveRename)
        .map_err(|_| fail())?;
    complete
        .rename_to(operation.clone(), name("record.json")?)
        .map_err(|_| fail())?;
    drop(complete);
    checkpoint("journal")?;
    scope.current()?;
    file.rename_to(operation.clone(), name(PAYLOAD)?)
        .map_err(|_| error("RECYCLE_OUTCOME_UNKNOWN"))?;
    checkpoint("moved")?;
    drop(file);
    Ok(Response {
        unavailable_count: 0,
        records: vec![inspect(scope, id, &operation, &journal)
            .map_err(|_| error("RECYCLE_OUTCOME_UNKNOWN"))?],
    })
}
pub(crate) fn restore(scope: &Scope, id: &str) -> Result<Response, SafeError> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error("RECYCLE_INVALID_REQUEST"));
    }
    let container = container(scope, false)?.ok_or_else(fail)?;
    let operation = folder(&container, id)?;
    let journal = read_journal(scope, &operation)?.ok_or_else(fail)?;
    let before = inspect(scope, id, &operation, &journal)?;
    if before.state == "source" {
        return Ok(Response {
            unavailable_count: 0,
            records: vec![before],
        });
    }
    if before.state != "stored" {
        return Err(error("RECYCLE_OUTCOME_UNKNOWN"));
    }
    if before.original_occupied {
        return Err(error("RECYCLE_RESTORE_CONFLICT"));
    }
    let mut file = operation
        .open_file(name(PAYLOAD)?, FileAccess::ExclusiveRename)
        .map_err(|_| error("SOURCE_BUSY"))?;
    if file.identity() != &journal.identity {
        return Err(error("SOURCE_CHANGED"));
    }
    verify_transcript(&file, &journal.native_id, &journal.cwd)?;
    let (parent, original) = source_parent(&root(scope)?, &journal.relative, &journal.native_id)?;
    scope.current()?;
    checkpoint("restore")?;
    // rename_handle 强制 ReplaceIfExists=false；前面的观察不替代原子无覆盖条件。
    // 已发出的 rename 可能成功后才无法验证；不能把所有错误当成确定未执行的冲突。
    file.rename_to(parent, original)
        .map_err(|_| error("RECYCLE_OUTCOME_UNKNOWN"))?;
    checkpoint("restored")?;
    drop(file);
    Ok(Response {
        unavailable_count: 0,
        records: vec![inspect(scope, id, &operation, &journal)
            .map_err(|_| error("RECYCLE_OUTCOME_UNKNOWN"))?],
    })
}
fn checkpoint(_stage: &str) -> Result<(), SafeError> {
    #[cfg(test)]
    if FAIL_AT.with(|value| value.borrow().as_deref() == Some(_stage)) {
        return Err(error("RECYCLE_OUTCOME_UNKNOWN"));
    }
    Ok(())
}
#[cfg(test)]
thread_local! { pub(super) static FAIL_AT:std::cell::RefCell<Option<String>>=const {std::cell::RefCell::new(None)}; }
