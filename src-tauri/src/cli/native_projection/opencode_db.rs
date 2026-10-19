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

pub(super) fn read_history(root: &Root, projects: &[ProjectRef<'_>]) -> ReadResult<Vec<ResourceItem>> {
    if projects.is_empty() {
        // Run/root-wide 观察不接 opencode：归属必须落在已 admitted 的项目上。
        return Err("SOURCE_UNSUPPORTED");
    }
    // SQLite 只认路径、拿不到 cap-std 句柄，因此用 Root 自身的守卫预检 db 与
    // -wal/-shm（拒 symlink/非 regular/相对越界），出口再 current() + stat 复核。
    // 残余 TOCTOU 窗口与全库既有姿态一致：「检出替换，不是 race-free 读沙箱」。
    for name in std::iter::once(DB_FILE).chain(SIDECAR_FILES.iter().copied()) {
        let present = root.stat(Path::new(name))?;
        if name == DB_FILE && present.is_none() {
            return Err("SOURCE_UNSUPPORTED");
        }
    }
    let mut conn = open_read_only(&root.selected_path().join(DB_FILE))?;
    let tx = conn.transaction().map_err(|_| "SOURCE_BUSY")?;
    probe_schema(&tx)?;
    let worktrees = read_worktrees(&tx)?;
    let mut items = Vec::new();
    for project in projects {
        append_project(&tx, root, project, &worktrees, &mut items)?;
    }
    tx.commit().map_err(|_| "SOURCE_BUSY")?;
    Ok(items)
}

fn open_read_only(path: &Path) -> ReadResult<Connection> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
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
        .filter(|(_, worktree)| {
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
