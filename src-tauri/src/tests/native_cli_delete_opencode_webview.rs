use super::*;
use crate::version_history::windows::files::{ComponentName, FileAccess, PinnedFile};
use rusqlite::{Connection, OpenFlags};

pub(super) struct OpencodeFixture {
    pub(super) id: String,
    pub(super) context: serde_json::Value,
    database: PathBuf,
    controls: Vec<String>,
    events: Vec<serde_json::Value>,
    pub(super) proofs: serde_json::Value,
    _directory: Arc<Directory>,
    _node: PinnedFile,
}
fn snapshot(db: &Connection, id: &str) -> Vec<String> {
    let mut result = vec![];
    for table in [
        "migration",
        "job",
        "schedule",
        "blob",
        "blob_ref",
        "session_message",
        "session_inbox",
        "session_part",
        "credential",
        "project",
        "session_v2",
        "event",
    ] {
        let mut statement = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        let names: Vec<_> = statement
            .column_names()
            .into_iter()
            .map(str::to_owned)
            .collect();
        let identity = names.iter().position(|name| {
            name == if table == "event" {
                "aggregate_id"
            } else {
                "id"
            }
        });
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            let target = identity
                .is_some_and(|column| row.get::<_, String>(column).ok().as_deref() == Some(id));
            if table == "event" && target {
                continue;
            }
            let values: Vec<_> = (0..columns)
                .filter(|column| {
                    !(table == "session_v2"
                        && target
                        && matches!(names[*column].as_str(), "time_archived" | "time_updated"))
                })
                .map(|column| row.get::<_, rusqlite::types::Value>(column).unwrap())
                .collect();
            result.push(format!("{table}:{values:?}"));
        }
    }
    result
}
fn target_events(db: &Connection, id: &str) -> Vec<serde_json::Value> {
    let mut statement = db.prepare("SELECT id,aggregate_id,seq,created,type,data FROM event WHERE aggregate_id=?1 ORDER BY seq").unwrap();
    statement.query_map([id], |r| Ok(serde_json::json!({"id":r.get::<_,String>(0)?,"aggregate":r.get::<_,String>(1)?,"seq":r.get::<_,i64>(2)?,"created":r.get::<_,i64>(3)?,"type":r.get::<_,String>(4)?,"data":r.get::<_,String>(5)?}))).unwrap().map(Result::unwrap).collect()
}
impl OpencodeFixture {
    pub(super) fn new(backend: &BackendFixture) -> Self {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(workspace.join(".audit/opencode-webview-seed.json")).unwrap(),
        )
        .unwrap();
        let seed = manifest["seed"].as_str().unwrap();
        assert!(seed
            .strip_prefix("opencode-sdk-recovery-")
            .is_some_and(|suffix| !suffix.is_empty()
                && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())));
        let source = Directory::open_absolute(&workspace.join(".audit").join(seed)).unwrap();
        let source = source
            .open_file(
                ComponentName::new(std::ffi::OsStr::new("fixture.db")).unwrap(),
                FileAccess::Read,
            )
            .unwrap();
        let bytes = source.read_prefix(16 * 1024 * 1024 + 1).unwrap();
        assert!(bytes.len() <= 16 * 1024 * 1024);
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            manifest["seedSha256"].as_str().unwrap()
        );
        let node_path = PathBuf::from(manifest["node"].as_str().unwrap());
        assert_eq!(node_path.file_name().unwrap(), "node.exe");
        let node_root = Directory::open_absolute(node_path.parent().unwrap()).unwrap();
        let node = node_root
            .open_file(
                ComponentName::new(node_path.file_name().unwrap()).unwrap(),
                FileAccess::Read,
            )
            .unwrap();
        assert_eq!(
            node.digest().unwrap(),
            manifest["nodeSha256"].as_str().unwrap()
        );
        let directory = backend.fixture.root.join("opencode-data").join("opencode");
        fs::create_dir_all(&directory).unwrap();
        let held = Directory::open_absolute(&directory).unwrap();
        let database = directory.join("opencode.db");
        fs::write(&database, bytes).unwrap();
        let db = Connection::open(&database).unwrap();
        let id: String = db
            .query_row(
                "SELECT id FROM session_v2 WHERE title='late target'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        db.execute(
            "UPDATE session_v2 SET directory=?1",
            [backend.fixture.project.to_str().unwrap()],
        )
        .unwrap();
        db.execute(
            "UPDATE project SET worktree=?1",
            [backend.fixture.project.to_str().unwrap()],
        )
        .unwrap();
        let controls = snapshot(&db, &id);
        let events = target_events(&db, &id);
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        drop(db);
        let mut profile = Profile::new("opencode-webview-fixture", CliKind::Opencode);
        profile.program_path = Override::Set(
            backend
                .fixture
                .project
                .join("must-not-exist.exe")
                .to_str()
                .unwrap()
                .into(),
        );
        profile.default_args = Override::Set(vec![]);
        for (key, value) in [
            ("OPENCODE_DB", database.clone()),
            ("XDG_DATA_HOME", backend.fixture.root.join("opencode-data")),
            ("PATH", node_path.parent().unwrap().to_owned()),
        ] {
            profile.env.insert(
                key.into(),
                Override::Set(EnvValue::Literal {
                    value: value.to_str().unwrap().into(),
                    non_secret: true,
                }),
            );
        }
        backend
            .repository
            .apply(
                backend.repository.read().unwrap().revision,
                Patch::Create { profile },
            )
            .unwrap();
        let context = serde_json::json!({"cli":"opencode","profileId":"opencode-webview-fixture","profileRevision":backend.repository.get_profile("opencode-webview-fixture").unwrap().revision,"projectId":backend.project_id,"projectPath":backend.fixture.project});
        let fixture = Self {
            id,
            context,
            database,
            controls,
            events,
            proofs: manifest,
            _directory: held,
            _node: node,
        };
        fixture.verify(false, 0).unwrap();
        fixture
    }
    pub(super) fn verify(&self, archived: bool, count: usize) -> Result<(), String> {
        let db = Connection::open_with_flags(&self.database, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|_| "OPENCODE_VERIFY_FAILED")?;
        let state: (Option<i64>, String) = db
            .query_row(
                "SELECT time_archived,title FROM session_v2 WHERE id=?1",
                [&self.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| "OPENCODE_VERIFY_FAILED")?;
        if state.0.is_some_and(|value| value > 0) != archived
            || state.1 != "late target"
            || snapshot(&db, &self.id) != self.controls
        {
            return Err("OPENCODE_STATE_CHANGED".into());
        }
        let events = target_events(&db, &self.id);
        if events.len() != self.events.len() + count || events[..self.events.len()] != self.events {
            return Err("OPENCODE_EVENTS_CHANGED".into());
        }
        for (index, event) in events[self.events.len()..].iter().enumerate() {
            let data: serde_json::Value =
                serde_json::from_str(event["data"].as_str().ok_or("OPENCODE_EVENT_INVALID")?)
                    .map_err(|_| "OPENCODE_EVENT_INVALID")?;
            if event["type"]
                != if index == 0 {
                    "session.archived.1"
                } else {
                    "session.unarchived.1"
                }
                || data != serde_json::json!({"sessionID":self.id})
                || event["seq"].as_i64()
                    != self
                        .events
                        .last()
                        .and_then(|value| value["seq"].as_i64())
                        .map(|seq| seq + 1 + index as i64)
            {
                return Err("OPENCODE_EVENT_INVALID".into());
            }
        }
        Ok(())
    }
}
#[tauri::command]
pub(super) async fn s11_opencode_checkpoint(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    state.admit(&webview, &request)?;
    let q: Named = query(&request)?;
    let mut checks = state.opencode_checks.lock();
    let expected = [("review", 0), ("archived", 1), ("restored", 2)];
    if expected.get(checks.len()).copied()
        != Some((
            q.name.as_str(),
            state.opencode_calls.load(Ordering::Acquire),
        ))
    {
        return Err("OPENCODE_DISPATCH_INVALID".into());
    }
    state.opencode.verify(
        q.name == "archived",
        state.opencode_calls.load(Ordering::Acquire),
    )?;
    state.verify_files(false)?;
    println!("STAGE11_OPENCODE_CHECKPOINT {}", q.name);
    checks.push(q.name);
    Ok(())
}
