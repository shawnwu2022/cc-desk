//! opencode_db 读取器行为：归属、parent 过滤、schema 探测、预算。fixture 用真实 DDL。
use crate::cli::native_projection::catalog::ProjectRef;
use crate::cli::native_projection::opencode_db::read_history;
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
        // 真实 opencode 库是 WAL；不显式设置则 fixture 落在 rollback journal，
        // 并发探针声称的「WAL 快照隔离」就不成立于其真实机制。
        conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
            .unwrap();
        drop(conn);
        let project = root.join("proj");
        std::fs::create_dir_all(&project).unwrap();
        let project_root = Root::open(&project).unwrap();
        let canonical = std::fs::canonicalize(&project).unwrap();
        let paths = if canonical == project {
            vec![project.clone()]
        } else {
            vec![project.clone(), canonical]
        };
        Self {
            _guard: guard,
            root,
            project,
            project_root,
            paths,
        }
    }

    fn conn(&self) -> Connection {
        Connection::open(self.root.join("opencode.db")).unwrap()
    }

    fn project_ref(&self) -> ProjectRef<'_> {
        ProjectRef {
            id: "proj-1",
            root: &self.project_root,
            paths: &self.paths,
        }
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
fn seed(
    conn: &Connection,
    id: &str,
    project_id: &str,
    directory: &str,
    title: &str,
    updated: i64,
    parent: Option<&str>,
) {
    conn.execute(
        "INSERT INTO session (id, project_id, parent_id, slug, directory, title, version, cost, \
         tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, \
         time_created, time_updated) \
         VALUES (?1,?2,?3,?4,?5,?6,'1.19.0',0,0,0,0,0,0,?7,?7)",
        rusqlite::params![id, project_id, parent, id, directory, title, updated],
    )
    .unwrap();
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
    seed(
        &conn,
        "ses_a",
        "p1",
        &sub.to_string_lossy(),
        "子目录会话",
        200,
        None,
    );
    seed(
        &conn,
        "ses_b",
        "p1",
        &f.project.to_string_lossy(),
        "根目录会话",
        100,
        None,
    );
    seed(
        &conn,
        "ses_x",
        "other",
        &f.project.to_string_lossy(),
        "别家项目",
        300,
        None,
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    let ids: Vec<&str> = items
        .iter()
        .map(|i| match i {
            ResourceItem::Session {
                native_session_id, ..
            } => native_session_id.as_str(),
            _ => panic!("session expected"),
        })
        .collect();
    assert_eq!(ids, vec!["ses_a", "ses_b"]); // updated DESC；ses_x 不归属
}

#[test]
#[allow(non_snake_case)]
fn Opencode_CustomDbReadsOnlySelectedFile_25() {
    let f = Fixture::new();
    let custom = f.root.join("custom.db");
    let conn = Connection::open(&custom).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        "ses_custom",
        "p1",
        &f.project.to_string_lossy(),
        "custom",
        1,
        None,
    );
    drop(conn);
    let root = Root::open_sqlite(&f.root, "custom.db").unwrap();
    let items = read_history(&root, &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        matches!(&items[0], ResourceItem::Session { native_session_id, .. } if native_session_id == "ses_custom")
    );
    assert_ne!(root.key(), Root::open(&f.root).unwrap().key());
}

// global 行永不按 worktree 匹配，只按 directory canonical 比对（spec §5.3）。
#[test]
#[allow(non_snake_case)]
fn Opencode_GlobalProjectMatchesByDirectory_02() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.execute("INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('global', '/', 1, 1, '[]')", []).unwrap();
    seed(
        &conn,
        "ses_g",
        "global",
        &f.project.to_string_lossy(),
        "global 归属",
        100,
        None,
    );
    seed(
        &conn,
        "ses_o",
        "global",
        "C:/nowhere/else",
        "不归属",
        200,
        None,
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session {
            native_session_id,
            cwd,
            ..
        } => {
            assert_eq!(native_session_id, "ses_g");
            assert!(cwd.is_some());
        }
        _ => panic!("session expected"),
    }
}

#[test]
#[allow(non_snake_case)]
fn Opencode_GlobalWorktreeNeverClaimsForeignSessions_11() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.execute(
        "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('global', ?1, 1, 1, '[]')",
        [&f.project.to_string_lossy().as_ref()],
    ).unwrap();
    seed(
        &conn,
        "ses_here",
        "global",
        &f.project.to_string_lossy(),
        "here",
        100,
        None,
    );
    seed(
        &conn,
        "ses_elsewhere",
        "global",
        "C:/somewhere/else",
        "elsewhere",
        200,
        None,
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    let ids: Vec<&str> = items
        .iter()
        .map(|item| match item {
            ResourceItem::Session {
                native_session_id, ..
            } => native_session_id.as_str(),
            _ => panic!("session expected"),
        })
        .collect();
    assert_eq!(ids, ["ses_here"]);
}

#[test]
#[allow(non_snake_case)]
fn Opencode_LinkedWalFailsClosed_12() {
    let f = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    for sidecar in ["opencode.db-wal", "opencode.db-shm"] {
        let target = outside.path().join(sidecar);
        std::fs::write(&target, b"outside").unwrap();
        let link = f.root.join(sidecar);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_file(&target, &link) {
            eprintln!("[SKIP] Opencode_LinkedWalFailsClosed_12: symlink unavailable: {error}");
            return;
        }
        let error = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap_err();
        assert_eq!(error, "SOURCE_NOT_REGULAR", "{sidecar}");
        std::fs::remove_file(link).unwrap();
    }
}

#[cfg(unix)]
#[test]
#[allow(non_snake_case)]
fn Opencode_AuthorizedAncestorSymlinkReadsHistory_26() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        "ses_alias",
        "p1",
        &f.project.to_string_lossy(),
        "alias",
        1,
        None,
    );
    drop(conn);
    let aliases = tempfile::tempdir().unwrap();
    let selected = aliases.path().join("authorized-alias");
    std::os::unix::fs::symlink(&f.root, &selected).unwrap();
    let items = read_history(&Root::open(&selected).unwrap(), &[f.project_ref()]).unwrap();
    assert!(
        matches!(&items[..], [ResourceItem::Session { native_session_id, .. }] if native_session_id == "ses_alias")
    );
}

#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_SourceHandlePinsWalAgainstReplacement_14() {
    let f = Fixture::new();
    let wal = f.root.join("opencode.db-wal");
    let moved = f.root.join("moved-wal");
    std::fs::write(&wal, b"inside").unwrap();
    let root = Root::open(&f.root).unwrap();
    let held = root
        .sqlite_source_file(std::path::Path::new("opencode.db-wal"))
        .unwrap()
        .unwrap();
    assert!(std::fs::rename(&wal, &moved).is_err());
    drop(held);
    std::fs::rename(&wal, &moved).unwrap();
}

#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_PrivateSnapshotPinsEverySqlitePath_15() {
    let f = Fixture::new();
    let root = Root::open(&f.root).unwrap();
    let snapshot = super::private_sqlite_snapshot(&root).unwrap();
    for name in ["opencode.db", "opencode.db-wal", "opencode.db-shm"] {
        assert!(
            std::fs::rename(
                snapshot.directory.path().join(name),
                snapshot.directory.path().join(format!("moved-{name}")),
            )
            .is_err(),
            "{name}"
        );
    }
}

// parent_id 非空是 subagent 子会话，不进顶层列表（spec §5.5）。
#[test]
#[allow(non_snake_case)]
fn Opencode_SubagentSessionsAreExcluded_03() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        "ses_top",
        "p1",
        &f.project.to_string_lossy(),
        "顶层",
        200,
        None,
    );
    seed(
        &conn,
        "ses_kid",
        "p1",
        &f.project.to_string_lossy(),
        "子代理",
        300,
        Some("ses_top"),
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session {
            native_session_id, ..
        } => assert_eq!(native_session_id, "ses_top"),
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
    let projects = vec![ProjectRef {
        id: "p",
        root: &projects_root,
        paths: &paths,
    }];
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
    seed(
        &conn,
        "ses_a",
        "p1",
        &f.project.to_string_lossy(),
        "T",
        100,
        None,
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    match &items[0] {
        ResourceItem::Session {
            session_key,
            updated_at,
            created_at,
            project_id,
            ..
        } => {
            assert!(
                session_key.contains("\"local\"")
                    && session_key.contains("\"opencode\"")
                    && session_key.contains("ses_a")
            );
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
        seed(
            &conn,
            &format!("ses_{index:05}"),
            "p1",
            &f.project.to_string_lossy(),
            "T",
            1000 + index,
            None,
        );
    }
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 2000);
    match &items[items.len() - 1] {
        ResourceItem::Session {
            truncated,
            native_session_id,
            ..
        } => {
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
        seed(
            &conn,
            &format!("glo_{index:05}"),
            "global",
            &f.project.to_string_lossy(),
            "T",
            1000 + index,
            None,
        );
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
    seed(
        &conn,
        &"a".repeat(300),
        "p1",
        &f.project.to_string_lossy(),
        "bad",
        300,
        None,
    );
    seed(
        &conn,
        "ses_ok",
        "p1",
        &f.project.to_string_lossy(),
        "ok",
        100,
        None,
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session {
            native_session_id, ..
        } => assert_eq!(native_session_id, "ses_ok"),
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
        seed(
            &conn,
            "ses_a",
            "p1",
            &f.project.to_string_lossy(),
            "T",
            100,
            None,
        );
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
        // 环境无可用解释器时跳过并显式告知：[SKIP] 标记可被 CI 日志检索，
        // 不因宿主缺 Python 让 CI 变红，也不冒充「已验证」。
        eprintln!("[SKIP] Opencode_ConcurrentWriterRemainsReadable_01: no usable python for concurrent-writer probe");
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

// 不依赖宿主 Python：写者在 WAL 中留下未提交事务时，能力快照仍须读到旧提交行。
#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_UncommittedWalWriterKeepsCommittedHistory_13() {
    let f = Fixture::new();
    {
        let conn = f.conn();
        f.register_project(&conn, "p1");
        seed(
            &conn,
            "ses_committed",
            "p1",
            &f.project.to_string_lossy(),
            "kept",
            100,
            None,
        );
    }
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
    let database = f.root.join("opencode.db");
    let directory = f.project.to_string_lossy().into_owned();
    let writer = std::thread::spawn(move || {
        let conn = Connection::open(database).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE").unwrap();
        seed(
            &conn,
            "ses_uncommitted",
            "p1",
            &directory,
            "pending",
            200,
            None,
        );
        ready_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        conn.execute_batch("ROLLBACK").unwrap();
    });
    ready_rx.recv().unwrap();
    let result = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]);
    release_tx.send(()).unwrap();
    writer.join().unwrap();
    let items = result.unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session {
            native_session_id, ..
        } => assert_eq!(native_session_id, "ses_committed"),
        _ => panic!("session expected"),
    }
}

// 确定性 BUSY：另一连接以 locking_mode=EXCLUSIVE 独占打开（备份/同步工具、另一
// 实例迁移等真实场景）时，读者的读锁在 busy_timeout(2s) 重试耗尽后必须映射
// SOURCE_BUSY，不得伪装成 schema 不支持或读取失败。WAL 快照隔离的「可读」半边
// 由上面的并发探针覆盖，两者互补。
#[test]
#[allow(non_snake_case)]
fn Opencode_ExclusiveWriterMapsSourceBusy_10() {
    let f = Fixture::new();
    {
        let conn = f.conn();
        f.register_project(&conn, "p1");
        seed(
            &conn,
            "ses_held",
            "p1",
            &f.project.to_string_lossy(),
            "T",
            100,
            None,
        );
    }
    let writer = f.conn();
    writer
        .query_row("PRAGMA locking_mode=EXCLUSIVE", [], |row| {
            row.get::<_, String>(0)
        })
        .unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let error = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap_err();
    writer.execute_batch("ROLLBACK").unwrap();
    assert_eq!(error, "SOURCE_BUSY");
}

#[test]
#[allow(non_snake_case)]
fn Opencode_GlobalDirectoryIsFilteredBeforeLimit_16() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.execute("INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('global', '/', 1, 1, '[]')", []).unwrap();
    seed(
        &conn,
        "ses_target",
        "global",
        &f.project.to_string_lossy(),
        "target",
        1,
        None,
    );
    for index in 0..2100 {
        seed(
            &conn,
            &format!("foreign_{index}"),
            "global",
            "C:/foreign",
            "foreign",
            index + 100,
            None,
        );
    }
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    assert!(
        matches!(&items[0], ResourceItem::Session { native_session_id, .. } if native_session_id == "ses_target")
    );
}

#[test]
#[allow(non_snake_case)]
fn Opencode_NulTextCannotPoisonProjection_17() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        "ses_title",
        "p1",
        &f.project.to_string_lossy(),
        "before\0after",
        2,
        None,
    );
    seed(
        &conn,
        "invalid\0identity",
        "p1",
        &f.project.to_string_lossy(),
        "bad",
        1,
        None,
    );
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0] {
        ResourceItem::Session {
            title,
            title_unknown,
            metadata_incomplete,
            ..
        } => {
            assert!(!title.contains('\0'));
            assert_eq!(*title_unknown, Some(true));
            assert_eq!(*metadata_incomplete, Some(true));
        }
        _ => panic!("session expected"),
    }
}

#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_CheckpointCannotSplitPrivateSnapshot_18() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    f.register_project(&conn, "p1");
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    seed(
        &conn,
        "ses_wal",
        "p1",
        &f.project.to_string_lossy(),
        "WAL",
        1,
        None,
    );
    let root = Root::open(&f.root).unwrap();
    let snapshot = super::private_sqlite_snapshot_during(&root, || {
        let busy: i64 = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            busy, 1,
            "checkpoint must not rewrite DB between DB and WAL copies"
        );
    })
    .unwrap();
    let copy = super::open_read_only(&snapshot.directory.path().join("opencode.db")).unwrap();
    let count: i64 = copy
        .query_row("SELECT count(*) FROM session", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
#[allow(non_snake_case)]
fn Opencode_EmptyOmittedHistoryReportsIncomplete_19() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        &"x".repeat(300),
        "p1",
        &f.project.to_string_lossy(),
        "bad",
        1,
        None,
    );
    let mut budget = crate::cli::native_projection::scoped_fs::Budget::new(Default::default());
    let items = super::read_history_with_budget(
        &Root::open(&f.root).unwrap(),
        &[f.project_ref()],
        &mut budget,
    )
    .unwrap();
    assert!(items.is_empty());
    assert!(budget.history_metadata_incomplete());
}

#[test]
#[allow(non_snake_case)]
fn Opencode_CatalogRetainsEmptyPartialAndRechecksGrant_20() {
    use crate::cli::native_projection::catalog::{read, Catalog, Options};
    use crate::cli::native_projection::scoped_fs::Budget;
    use crate::cli::native_projection::wire::ResourceKind;
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        &"x".repeat(300),
        "p1",
        &f.project.to_string_lossy(),
        "bad",
        1,
        None,
    );
    let root = Root::open(&f.root).unwrap();
    let projects = [f.project_ref()];
    let options = Options {
        kind: ResourceKind::History,
        query: None,
        session_id: None,
    };
    let mut budget = Budget::new(Default::default());
    let items = read(
        &Catalog {
            cli: crate::cli::types::CliKind::Opencode,
            root: &root,
            projects: &projects,
            user_config: None,
            check: &|| Ok(()),
        },
        &options,
        &mut budget,
    )
    .unwrap();
    assert!(items.is_empty());
    assert!(budget.history_metadata_incomplete());
    let checks = std::cell::Cell::new(0);
    let check = || {
        checks.set(checks.get() + 1);
        if checks.get() > 1 {
            Err("SCOPE_REVOKED")
        } else {
            Ok(())
        }
    };
    let error = read(
        &Catalog {
            cli: crate::cli::types::CliKind::Opencode,
            root: &root,
            projects: &projects,
            user_config: None,
            check: &check,
        },
        &options,
        &mut Budget::new(Default::default()),
    )
    .unwrap_err();
    assert_eq!(error, "SCOPE_REVOKED");
}

#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_GlobalDirectorySpellingMatchesExistingWindowsRules_21() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.execute("INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('global', '/', 1, 1, '[]')", []).unwrap();
    let directory = f
        .project
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_uppercase();
    seed(&conn, "ses_alias", "global", &directory, "alias", 1, None);
    let items = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(items.len(), 1);
}

#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_WalResetCannotReplaceFramesDuringSnapshot_22() {
    let f = Fixture::new();
    let conn = f.conn();
    conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        "ses_before",
        "p1",
        &f.project.to_string_lossy(),
        "before",
        1,
        None,
    );
    let busy: i64 = conn
        .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| row.get(0))
        .unwrap();
    assert_eq!(busy, 0);
    let wal = f.root.join("opencode.db-wal");
    let salts = std::fs::read(&wal).unwrap()[16..24].to_vec();
    let root = Root::open(&f.root).unwrap();
    let snapshot = super::private_sqlite_snapshot_during(&root, || {
        seed(
            &conn,
            "ses_after",
            "p1",
            &f.project.to_string_lossy(),
            "after",
            2,
            None,
        );
        assert_eq!(
            &std::fs::read(&wal).unwrap()[16..24],
            salts.as_slice(),
            "READ(1) must prevent resetting the existing WAL"
        );
    })
    .unwrap();
    let copy = super::open_read_only(&snapshot.directory.path().join("opencode.db")).unwrap();
    let count: i64 = copy
        .query_row("SELECT count(*) FROM session", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
}

#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn Opencode_WithoutShmBlocksNewConnectionOnlyDuringCopy_23() {
    let f = Fixture::new();
    assert!(!f.root.join("opencode.db-shm").exists());
    let root = Root::open(&f.root).unwrap();
    let snapshot = super::private_sqlite_snapshot_during(&root, || {
        let other = f.conn();
        other.busy_timeout(std::time::Duration::ZERO).unwrap();
        let error = other
            .query_row("SELECT count(*) FROM session", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap_err();
        assert_eq!(super::sqlite_code(error), "SOURCE_BUSY");
    })
    .unwrap();
    let other = f.conn();
    f.register_project(&other, "p1");
    drop(other);
    let copy = super::open_read_only(&snapshot.directory.path().join("opencode.db")).unwrap();
    let count: i64 = copy
        .query_row("SELECT count(*) FROM session", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
#[allow(non_snake_case)]
fn Opencode_BundledSqliteMatchesQualifiedHost_27() {
    assert_eq!(
        rusqlite::version_number(),
        3_053_002,
        "application and Host must use the same qualified bundled engine"
    );
    let conn = Connection::open_in_memory().unwrap();
    let source: String = conn
        .query_row("SELECT sqlite_source_id()", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        source,
        "2026-06-03 19:12:13 d6e03d8c777cfa2d35e3b60d8ec3e0187f3e9f99d8e2ee9cac695fd6fcdf1a24"
    );
    for (option, expected) in [
        ("THREADSAFE=1", 1),
        ("NO_SYNC", 0),
        ("OMIT_WAL", 0),
        ("OMIT_DISKIO", 0),
        ("OMIT_COMPILEOPTION_DIAGS", 0),
    ] {
        let actual: i32 = conn
            .query_row("SELECT sqlite_compileoption_used(?1)", [option], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(actual, expected, "qualified engine compile option {option}");
    }
}

#[test]
#[allow(non_snake_case)]
fn Opencode_PrivateWalReadLeavesOriginalDatabaseBytesUnchanged_28() {
    let f = Fixture::new();
    let conn = f.conn();
    f.register_project(&conn, "p1");
    seed(
        &conn,
        "ses_readonly",
        "p1",
        &f.project.to_string_lossy(),
        "kept",
        100,
        None,
    );
    let db = f.root.join("opencode.db");
    let wal = f.root.join("opencode.db-wal");
    let before_db = std::fs::read(&db).unwrap();
    let before_wal = std::fs::read(&wal).unwrap();
    let read_only = super::open_read_only(&db).unwrap();
    let query_only: i32 = read_only
        .query_row("PRAGMA query_only", [], |row| row.get(0))
        .unwrap();
    assert_eq!(query_only, 1);
    let denied = read_only.execute_batch("DELETE FROM session").unwrap_err();
    assert!(
        matches!(denied, rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::ReadOnly)
    );
    drop(read_only);
    assert!(
        before_wal.len() > 32,
        "committed rows must actually be present in WAL"
    );
    let result = read_history(&Root::open(&f.root).unwrap(), &[f.project_ref()]).unwrap();
    assert_eq!(result.len(), 1);
    match &result[0] {
        ResourceItem::Session {
            native_session_id, ..
        } => assert_eq!(native_session_id, "ses_readonly"),
        _ => panic!("session expected"),
    }
    assert_eq!(std::fs::read(db).unwrap(), before_db);
    assert_eq!(std::fs::read(wal).unwrap(), before_wal);
    drop(conn);
}
