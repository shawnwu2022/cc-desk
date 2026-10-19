//! opencode_db 读取器行为：归属、parent 过滤、schema 探测、预算。fixture 用真实 DDL。
use crate::cli::native_projection::opencode_db::read_history;
use crate::cli::native_projection::catalog::ProjectRef;
use crate::cli::native_projection::scoped_fs::Root;
use crate::cli::native_projection::wire::ResourceItem;
use rusqlite::Connection;
use std::path::PathBuf;

const SCHEMA: &str = include_str!("fixtures/opencode-schema-1.19.0.sql");

struct Fixture {
    _guard: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
    // ProjectRef.root 是 &Root、paths 是 &[PathBuf]，两者必须活得比 Fixture 借出的引用久。
    project_root: Root,
    paths: Vec<PathBuf>,
}

impl Fixture {
    fn new() -> Self {
        let guard = tempfile::tempdir().unwrap();
        let root = guard.path().to_path_buf();
        let conn = Connection::open(root.join("opencode.db")).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        drop(conn);
        let project = root.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let project_root = Root::open(&project).unwrap();
        let canonical = std::fs::canonicalize(&project).unwrap();
        let paths = if canonical == project { vec![project.clone()] } else { vec![project.clone(), canonical] };
        Self { _guard: guard, root, project, project_root, paths }
    }

    fn conn(&self) -> Connection {
        Connection::open(self.root.join("opencode.db")).unwrap()
    }

    fn project_ref(&self) -> ProjectRef<'_> {
        ProjectRef { id: "proj-1", root: &self.project_root, paths: &self.paths }
    }

    fn register_project(&self, conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?1, ?2, 1, 1, '[]')",
            rusqlite::params![id, self.project.to_string_lossy()],
        ).unwrap();
    }
}

// 真实 session 表有多个 NOT NULL 列（slug/version/cost/tokens_*）。insert 必须逐列填齐——
// 以入库的 opencode-schema-1.19.0.sql 为准；若真实表还有别的 NOT NULL 列，按 DDL 补齐。
// 这是 harness 契约，不是产品逻辑：填不齐说明 fixture 与真实 schema 不一致，应当先修 fixture。
fn seed(conn: &Connection, id: &str, project_id: &str, directory: &str, title: &str, updated: i64, parent: Option<&str>) {
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, cost, \
         tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, \
         time_created, time_updated) \
         VALUES (?1,?2,?3,?4,?5,?6,'1.19.0',0,0,0,0,0,0,?7,?7)",
        rusqlite::params![id, project_id, parent, id, directory, title, updated],
    ).unwrap();
}

// 具名项目按 project_id 归属：子目录创建的会话也归到项目根。
#[test]
#[allow(non_snake_case)]
fn Opencode_DbHistoryAttributesByProjectId_01() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    // 真实 schema 对 session.project_id 有外键约束（rusqlite bundled 默认开启 FK）：
    // 「别家项目」也必须有 project 行；worktree 指向别处以保持不归属语义。
    conn.execute(
        "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('other', 'C:/somewhere/else', 1, 1, '[]')",
        [],
    ).unwrap();
    let sub = f.project.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    seed(&conn, "ses_a", "p1", &sub.to_string_lossy(), "子目录会话", 200, None);
    seed(&conn, "ses_b", "p1", &f.project.to_string_lossy(), "根目录会话", 100, None);
    seed(&conn, "ses_x", "other", &f.project.to_string_lossy(), "别家项目", 300, None);
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    let ids: Vec<&str> = items.iter().map(|i| match i {
        ResourceItem::Session { native_session_id, .. } => native_session_id.as_str(),
        _ => panic!("session expected"),
    }).collect();
    assert_eq!(ids, vec!["ses_a", "ses_b"]); // updated DESC；ses_x 不归属
}

// global 行永不按 worktree 匹配，只按 directory canonical 比对（spec §5.3）。
#[test]
#[allow(non_snake_case)]
fn Opencode_GlobalProjectMatchesByDirectory_02() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.execute("INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('global', '/', 1, 1, '[]')", []).unwrap();
    seed(&conn, "ses_g", "global", &f.project.to_string_lossy(), "global 归属", 100, None);
    seed(&conn, "ses_o", "global", "C:/nowhere/else", "不归属", 200, None);
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session { native_session_id, cwd, .. } => {
            assert_eq!(native_session_id, "ses_g");
            assert!(cwd.is_some());
        }
        _ => panic!("session expected"),
    }
}

// parent_id 非空是 subagent 子会话，不进顶层列表（spec §5.5）。
#[test]
#[allow(non_snake_case)]
fn Opencode_SubagentSessionsAreExcluded_03() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(&conn, "ses_top", "p1", &f.project.to_string_lossy(), "顶层", 200, None);
    seed(&conn, "ses_kid", "p1", &f.project.to_string_lossy(), "子代理", 300, Some("ses_top"));
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session { native_session_id, .. } => assert_eq!(native_session_id, "ses_top"),
        _ => panic!("session expected"),
    }
}

// 缺表 = schema 漂移，fail-closed，不猜（spec §4.4）。
#[test]
#[allow(non_snake_case)]
fn Opencode_SchemaDriftFailsClosed_04() {
    let f = Fixture::new();
    f.conn().execute_batch("DROP TABLE session").unwrap();
    let err = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap_err();
    assert_eq!(err, "SOURCE_UNSUPPORTED");
}

// db 不存在（未安装/从未运行过）同样 fail-closed，不 panic。
#[test]
#[allow(non_snake_case)]
fn Opencode_MissingDatabaseFailsClosed_05() {
    let guard = tempfile::tempdir().unwrap();
    let project = guard.path().join("proj");
    std::fs::create_dir_all(&project).unwrap();
    let paths = vec![project];
    let projects_root = Root::open(guard.path()).unwrap();
    let projects = vec![ProjectRef { id: "p", root: &projects_root, paths: &paths }];
    let err = read_history(&Root::open(guard.path()).unwrap(), &projects).unwrap_err();
    assert_eq!(err, "SOURCE_UNSUPPORTED");
}

// session_key 契约与 history.rs:222 一致（["local", cli, rootKey, nativeId]）。
#[test]
#[allow(non_snake_case)]
fn Opencode_SessionKeyUsesStableIdentityContract_06() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(&conn, "ses_a", "p1", &f.project.to_string_lossy(), "T", 100, None);
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    match &items[0] {
        ResourceItem::Session { session_key, updated_at, created_at, project_id, .. } => {
            assert!(session_key.contains("\"local\"") && session_key.contains("\"opencode\"") && session_key.contains("ses_a"));
            assert_eq!(project_id.as_deref(), Some("proj-1"));
            assert!(updated_at.is_some() && created_at.is_some());
        }
        _ => panic!("session expected"),
    }
}

// 具名项目超限：保留最新的 PER_PROJECT_LIMIT 行并在末行披露 truncated，不伪造完整列表。
#[test]
#[allow(non_snake_case)]
fn Opencode_NamedOverflowDisclosesTruncation_07() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    for index in 0..(2000 + 3) {
        seed(&conn, &format!("ses_{index:05}"), "p1", &f.project.to_string_lossy(), "T", 1000 + index, None);
    }
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 2000);
    match &items[items.len() - 1] {
        ResourceItem::Session { truncated, native_session_id, .. } => {
            assert!(truncated);
            // ORDER BY time_updated DESC：丢弃的是最旧的 3 行，末行是保留窗口里最旧的。
            assert_eq!(native_session_id, "ses_00003");
        }
        _ => panic!("session expected"),
    }
}

// global 桶同样多取一行：目录过滤后无法证明完整性，超限也必须披露「可能不完整」。
#[test]
#[allow(non_snake_case)]
fn Opencode_GlobalOverflowDisclosesTruncation_08() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.execute("INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('global', '/', 1, 1, '[]')", []).unwrap();
    for index in 0..(2000 + 1) {
        seed(&conn, &format!("glo_{index:05}"), "global", &f.project.to_string_lossy(), "T", 1000 + index, None);
    }
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 2000);
    match &items[items.len() - 1] {
        ResourceItem::Session { truncated, .. } => assert!(truncated),
        _ => panic!("session expected"),
    }
}

// 病理行（超长 id）逐行省略而非整包拒绝；同页正常行不受影响。
#[test]
#[allow(non_snake_case)]
fn Opencode_OversizedIdentityRowIsOmitted_09() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(&conn, &"a".repeat(300), "p1", &f.project.to_string_lossy(), "bad", 300, None);
    seed(&conn, "ses_ok", "p1", &f.project.to_string_lossy(), "ok", 100, None);
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session { native_session_id, .. } => assert_eq!(native_session_id, "ses_ok"),
        _ => panic!("session expected"),
    }
}

// 独立进程持未提交写事务时，WAL 只读读者必须读到已提交行（对抗修订 #5）：
// "成功或 BUSY 都算过"会掩盖错误实现；确定性 BUSY 是另一条用例，不在这里混测。
#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_ConcurrentWriterRemainsReadable_01() {
    let f = Fixture::new();
    {
        let conn = f.conn();
        f.register_project(&conn, "p1");
        seed(&conn, "ses_a", "p1", &f.project.to_string_lossy(), "T", 100, None);
    }
    let db = f.root.join("opencode.db");
    let script = format!(
        "import sqlite3,time\nc=sqlite3.connect(r'{db}')\nc.execute('BEGIN IMMEDIATE')\n\
         c.execute(\"INSERT INTO session (id,project_id,slug,directory,title,version,cost,\
         tokens_input,tokens_output,tokens_reasoning,tokens_cache_read,tokens_cache_write,\
         time_created,time_updated) VALUES ('x','p1','x','{dir}','x','1.19.0',0,0,0,0,0,0,1,1)\")\n\
         time.sleep(20)\n",
        db = db.to_string_lossy(),
        dir = f.project.to_string_lossy().replace('\\', "/"),
    );
    let python = std::env::var("PYTHON").unwrap_or_else(|_| "python".into());
    let Ok(mut child) = std::process::Command::new(python)
        .args(["-I", "-c", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        // 环境无可用解释器时跳过并显式告知：不因宿主缺 Python 让 CI 变红。
        eprintln!("skipped: no usable python for concurrent-writer probe");
        return;
    };
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]);
    let _ = child.kill();
    let _ = child.wait();
    // 必须读到已提交的 1 行；未提交的 x 行不得出现（WAL 快照隔离）。
    let items = items.unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        crate::cli::native_projection::wire::ResourceItem::Session {
            native_session_id, ..
        } => assert_eq!(native_session_id, "ses_a"),
        _ => panic!("session expected"),
    }
}
