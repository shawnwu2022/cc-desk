//! opencode 历史读取：单一 SQLite 数据源，不进 catalog 的文件遍历路径。
//! 授权不在本模块：Grant.check() 与 root.current() 由 catalog/registry 在观察出口复核。
//! 归属只用文本归一化比对 admitted 项目自身的拼法；绝不对库内路径做文件系统解析——
//! canonicalize 一个库内字符串等于以 Desk 身份触碰未授权路径（对抗审查 #3）。
use super::catalog::ProjectRef;
use super::scoped_fs::{ReadResult, Root};
use super::wire::ResourceItem;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

const DB_FILE: &str = "opencode.db";
const SIDECAR_FILES: &[&str] = &["opencode.db-wal", "opencode.db-shm"];
const SESSION_COLUMNS: &[&str] = &["id", "project_id", "directory", "title", "time_created", "time_updated", "parent_id"];
const PROJECT_COLUMNS: &[&str] = &["id", "worktree"];
const BUSY_TIMEOUT_MS: u64 = 2000;
const TITLE_LIMIT: usize = 512;
const PER_PROJECT_LIMIT: i64 = 2000;
const GLOBAL_PROJECT_ID: &str = "global";

pub(crate) fn read_history(root: &Root, projects: &[ProjectRef<'_>]) -> ReadResult<Vec<ResourceItem>> {
    if projects.is_empty() {
        // Run/root-wide 观察不接 opencode：归属必须落在已 admitted 的项目上。
        return Err("SOURCE_UNSUPPORTED");
    }
    // Windows SQLite VFS 不保证侧车 NOFOLLOW；其路径只指向下面的能力句柄私有副本。
    // Unix VFS 自身对 db/WAL/SHM 使用 NOFOLLOW；两端仍复核源文件类型。
    for name in std::iter::once(DB_FILE).chain(SIDECAR_FILES.iter().copied()) {
        let present = root.sqlite_file_present(Path::new(name))?;
        if name == DB_FILE && !present {
            return Err("SOURCE_UNSUPPORTED");
        }
    }
    #[cfg(windows)]
    let snapshot = private_sqlite_snapshot(root)?;
    #[cfg(windows)]
    let database = snapshot.directory.path().join(DB_FILE);
    #[cfg(not(windows))]
    let database = root.selected_path().join(DB_FILE);
    let mut conn = open_read_only(&database)?;
    let tx = conn.transaction().map_err(|_| "SOURCE_BUSY")?;
    probe_schema(&tx)?;
    let worktrees = read_worktrees(&tx)?;
    let mut items = Vec::new();
    for project in projects {
        append_project(&tx, root, project, &worktrees, &mut items)?;
    }
    tx.commit().map_err(|_| "SOURCE_BUSY")?;
    for name in std::iter::once(DB_FILE).chain(SIDECAR_FILES.iter().copied()) {
        let present = root.sqlite_file_present(Path::new(name))?;
        if name == DB_FILE && !present {
            return Err("SOURCE_CHANGED");
        }
    }
    Ok(items)
}

#[cfg(windows)]
const SNAPSHOT_LIMIT: u64 = 1024 * 1024 * 1024;

#[cfg(windows)]
struct PrivateSqliteSnapshot {
    _database: std::fs::File,
    _wal: std::fs::File,
    _shm: std::fs::File,
    directory: tempfile::TempDir,
}

#[cfg(windows)]
struct SqliteSourceLock<'a> {
    file: &'a std::fs::File,
    offset: u32,
}

#[cfg(windows)]
impl Drop for SqliteSourceLock<'_> {
    fn drop(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::UnlockFileEx;
        use windows::Win32::System::IO::OVERLAPPED;
        let mut overlap = OVERLAPPED::default();
        overlap.Anonymous.Anonymous.Offset = self.offset;
        unsafe {
            let _ = UnlockFileEx(HANDLE(self.file.as_raw_handle()), None, 1, 0, &mut overlap);
        }
    }
}

#[cfg(windows)]
fn lock_sqlite_source(file: &std::fs::File) -> ReadResult<SqliteSourceLock<'_>> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::{HANDLE, ERROR_LOCK_VIOLATION};
    use windows::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_FAIL_IMMEDIATELY};
    use windows::Win32::System::IO::OVERLAPPED;

    // 与 SQLite Windows VFS 的 SHARED_FIRST/SHARED_SIZE 匹配；独占写事务锁住整个区间。
    // 尝试各槽位避免与普通读者恰好占用同一字节时误报 BUSY。
    for offset in 0x4000_0002..0x4000_0002 + 510 {
        let mut overlap = OVERLAPPED::default();
        overlap.Anonymous.Anonymous.Offset = offset;
        match unsafe {
            LockFileEx(
                HANDLE(file.as_raw_handle()),
                LOCKFILE_FAIL_IMMEDIATELY,
                None,
                1,
                0,
                &mut overlap,
            )
        } {
            Ok(()) => return Ok(SqliteSourceLock { file, offset }),
            Err(error) if error.code() == ERROR_LOCK_VIOLATION.to_hresult() => continue,
            Err(_) => return Err("SOURCE_READ_FAILED"),
        }
    }
    Err("SOURCE_BUSY")
}

#[cfg(windows)]
fn copy_sqlite_file(
    source: &std::fs::File,
    destination: &Path,
    remaining: &mut u64,
) -> ReadResult<std::fs::File> {
    use std::io::{Read, Write};
    let before = source.metadata().map_err(|_| "SOURCE_READ_FAILED")?;
    if !before.is_file() || before.len() > *remaining {
        return Err("SOURCE_TOO_LARGE");
    }
    *remaining -= before.len();
    let mut output = create_private_file(destination)?;
    let copied = std::io::copy(&mut source.take(before.len() + 1), &mut output)
        .map_err(|_| "SOURCE_READ_FAILED")?;
    output.flush().map_err(|_| "SOURCE_READ_FAILED")?;
    let after = source.metadata().map_err(|_| "SOURCE_READ_FAILED")?;
    if copied != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err("SOURCE_CHANGED");
    }
    Ok(output)
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> ReadResult<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .open(path)
        .map_err(|_| "SOURCE_READ_FAILED")
}

#[cfg(windows)]
fn private_sqlite_snapshot(root: &Root) -> ReadResult<PrivateSqliteSnapshot> {
    // SQLite 的 Windows VFS 会沿路径打开 -wal/-shm。仅复制能力目录中的 db/WAL，
    // 私有目录中的 SHM 由 SQLite 根据 WAL 重建，避免它触碰原始侧车路径。
    let database = root
        .sqlite_source_file(Path::new(DB_FILE))?
        .ok_or("SOURCE_UNSUPPORTED")?;
    let _lock = lock_sqlite_source(&database)?;
    let snapshot = tempfile::Builder::new()
        .prefix("cc-desk-opencode-")
        .tempdir()
        .map_err(|_| "SOURCE_READ_FAILED")?;
    let mut remaining = SNAPSHOT_LIMIT;
    let database = copy_sqlite_file(&database, &snapshot.path().join(DB_FILE), &mut remaining)?;
    let wal = if let Some(source) = root.sqlite_source_file(Path::new(SIDECAR_FILES[0]))? {
        copy_sqlite_file(&source, &snapshot.path().join(SIDECAR_FILES[0]), &mut remaining)?
    } else {
        create_private_file(&snapshot.path().join(SIDECAR_FILES[0]))?
    };
    let shm = create_private_file(&snapshot.path().join(SIDECAR_FILES[1]))?;
    root.current()?;
    Ok(PrivateSqliteSnapshot {
        _database: database,
        _wal: wal,
        _shm: shm,
        directory: snapshot,
    })
}

fn open_read_only(path: &Path) -> ReadResult<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW)
        .map_err(|_| "SOURCE_UNSUPPORTED")?;
    conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS))
        .map_err(|_| "SOURCE_BUSY")?;
    // 双保险：连接级只读，杜绝任何意外写入（WAL 下读者可安全并发）。
    conn.pragma_update(None, "query_only", true).map_err(|_| "SOURCE_BUSY")?;
    Ok(conn)
}

/// rusqlite 错误 → 既有 safe code。busy/locked 是并发现实，其余读错误不伪装成 schema 不支持。
fn sqlite_code(error: rusqlite::Error) -> &'static str {
    match error {
        rusqlite::Error::SqliteFailure(code, _) => match code.code {
            rusqlite::ErrorCode::DatabaseBusy => "SOURCE_BUSY",
            rusqlite::ErrorCode::DatabaseLocked => "SOURCE_BUSY",
            _ => "SOURCE_READ_FAILED",
        },
        _ => "SOURCE_READ_FAILED",
    }
}

fn probe_schema(tx: &Connection) -> ReadResult<()> {
    for (table, required) in [("session", SESSION_COLUMNS), ("project", PROJECT_COLUMNS)] {
        let mut stmt = tx
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(sqlite_code)?;
        let present: std::collections::BTreeSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(sqlite_code)?
            .collect::<Result<_, _>>()
            .map_err(sqlite_code)?;
        if !required.iter().all(|column| present.contains(*column)) {
            return Err("SOURCE_UNSUPPORTED");
        }
    }
    Ok(())
}

type Worktrees = Vec<(String, String)>;

fn read_worktrees(tx: &Connection) -> ReadResult<Worktrees> {
    let mut stmt = tx.prepare("SELECT id, worktree FROM project").map_err(sqlite_code)?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(sqlite_code)?;
    let mut result = Vec::new();
    for row in rows {
        let (id, worktree) = row.map_err(sqlite_code)?;
        if !worktree.is_empty() && worktree.len() <= 4096 {
            result.push((id, worktree));
        }
        // 过长/空的 worktree 不参与匹配；不做文件系统解析，也不报错。
    }
    Ok(result)
}

/// 仅文本归一化：Windows 下分隔符统一 + ASCII 大小写折叠；其余平台原样。
/// 不做 canonicalize——那会以 Desk 身份触碰库内字符串指向的任意路径。
fn spelling(text: &str) -> String {
    #[cfg(windows)]
    {
        text.replace('/', "\\").to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        text.to_owned()
    }
}

fn append_project(
    tx: &Connection,
    root: &Root,
    project: &ProjectRef<'_>,
    worktrees: &Worktrees,
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    // WHERE 在 ORDER BY/LIMIT 之前：先全局截断再归属会把整个项目挤出窗口、
    // 伪造「项目无历史」（对抗审查实测：3000 条他项目 + 2 条本项目 → 命中 0）。
    let named: Vec<String> = worktrees
        .iter()
        .filter(|(id, worktree)| {
            if id == GLOBAL_PROJECT_ID {
                return false;
            }
            let needle = spelling(worktree);
            project.paths.iter().any(|path| spelling(&path.to_string_lossy()) == needle)
        })
        .map(|(id, _)| id.clone())
        .collect();
    let sql = "SELECT id, title, directory, time_created, time_updated FROM session \
               WHERE parent_id IS NULL AND project_id = ?1 \
               ORDER BY time_updated DESC, id ASC LIMIT ?2";
    for project_id in &named {
        // SQL 多取一行：让 append_rows 能观察到「还有更多」，否则截断分支永不触发。
        append_rows(tx, root, project, sql, &[project_id as &dyn rusqlite::ToSql, &(PER_PROJECT_LIMIT + 1)], out)?;
    }
    // global 是漂移桶：永不按 worktree 归属，只按 directory 拼法比对（spec §5.3）。
    // 故意多取一行：目录过滤后无法证明完整性，超限就在该项目末行披露「可能不完整」，
    // 宁可过度披露也好过静默省略。
    let sql = "SELECT id, title, directory, time_created, time_updated FROM session \
               WHERE parent_id IS NULL AND project_id = ?1 \
               ORDER BY time_updated DESC, id ASC LIMIT ?2";
    let mut stmt = tx.prepare(sql).map_err(sqlite_code)?;
    let rows = stmt
        .query_map([&GLOBAL_PROJECT_ID as &dyn rusqlite::ToSql, &(PER_PROJECT_LIMIT + 1)], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(sqlite_code)?;
    let spellings: Vec<String> = project.paths.iter().map(|path| spelling(&path.to_string_lossy())).collect();
    let mut count = 0i64;
    for row in rows {
        let (id, title, directory, created_ms, updated_ms) = row.map_err(sqlite_code)?;
        count += 1;
        if count > PER_PROJECT_LIMIT {
            if let Some(ResourceItem::Session { truncated, .. }) = out.last_mut() {
                *truncated = true;
            }
            continue;
        }
        if !spellings.iter().any(|candidate| *candidate == spelling(&directory)) {
            continue;
        }
        if !bounded_identity(&id, &directory) {
            continue;
        }
        push_session(root, project, out, id, title, directory, created_ms, updated_ms);
    }
    Ok(())
}

/// 单个具名 project_id 的有界查询；调用方以 LIMIT+1 取行，超出上限的行丢弃
/// 并在该项目最后一行披露截断。计数按取到的行算，病理行不参与披露判定。
fn append_rows(
    tx: &Connection,
    root: &Root,
    project: &ProjectRef<'_>,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
    out: &mut Vec<ResourceItem>,
) -> ReadResult<()> {
    let mut stmt = tx.prepare(sql).map_err(sqlite_code)?;
    let rows = stmt
        .query_map(params, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .map_err(sqlite_code)?;
    let mut count = 0i64;
    for row in rows {
        let (id, title, directory, created_ms, updated_ms) = row.map_err(sqlite_code)?;
        count += 1;
        if count > PER_PROJECT_LIMIT {
            // 超限截断：用既有 truncated 字段披露，不伪造完整空集（对抗审查 #2）。
            if let Some(ResourceItem::Session { truncated, .. }) = out.last_mut() {
                *truncated = true;
            }
            continue;
        }
        if !bounded_identity(&id, &directory) {
            continue;
        }
        push_session(root, project, out, id, title, directory, created_ms, updated_ms);
    }
    Ok(())
}

/// 行身份界与 Claude/Codex 解析路径一致（id ≤256 字节、directory ≤32768 字节）。
/// 病理行选择逐行省略而非整包拒绝：一行脏数据不该让前端整个响应 INVALID_PROJECTION。
fn bounded_identity(id: &str, directory: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !directory.is_empty() && directory.len() <= 32768
}

#[allow(clippy::too_many_arguments)]
fn push_session(
    root: &Root,
    project: &ProjectRef<'_>,
    out: &mut Vec<ResourceItem>,
    id: String,
    title: String,
    directory: String,
    created_ms: i64,
    updated_ms: i64,
) {
    let (title, truncated) = bounded(&title, TITLE_LIMIT);
    out.push(ResourceItem::Session {
        session_key: stable_key(root, &id),
        native_session_id: id,
        project_id: Some(project.id.to_owned()),
        title,
        title_unknown: None,
        // opencode 的 title 由模型或占位模板生成，Claude 的 Prompt/Ai/Custom 三态都不适用；缺省不发。
        title_source: None,
        metadata_incomplete: None,
        read_failure: None,
        truncated,
        // opencode 无 CLI 侧归档容器；Desk 归档是独立元数据。
        archived: false,
        cwd: Some(directory),
        updated_at: rfc3339(updated_ms),
        created_at: rfc3339(created_ms),
    });
}

fn stable_key(root: &Root, native_id: &str) -> String {
    serde_json::to_string(&["local", crate::cli::types::CliKind::Opencode.as_str(), root.key(), native_id])
        .unwrap_or_default()
}

fn rfc3339(millis: i64) -> Option<String> {
    use chrono::TimeZone;
    chrono::Utc
        .timestamp_millis_opt(millis)
        .single()
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn bounded(text: &str, maximum: usize) -> (String, bool) {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), end < text.len())
}

#[cfg(test)]
#[path = "../../tests/native_cli_opencode_db.rs"]
mod tests;
