use super::*;

pub(super) struct RecycleFixture {
    pub(super) id: String,
    pub(super) context: serde_json::Value,
    source_root_key: String,
    transcript: PathBuf,
    original: Vec<u8>,
    controls: Vec<(PathBuf, Vec<u8>)>,
}
impl RecycleFixture {
    pub(super) fn new(backend: &BackendFixture) -> Self {
        let home = backend.fixture.root.join("claude-home");
        let directory = home.join("projects/fixture");
        fs::create_dir_all(&directory).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let transcript = directory.join(format!("{id}.jsonl"));
        let original = format!("{}\n", serde_json::json!({"type":"user","sessionId":id,"cwd":backend.fixture.project,"uuid":uuid::Uuid::new_v4().to_string(),"timestamp":"2026-11-12T00:00:00Z","message":{"role":"user","content":"Synthetic recycle acceptance"}})).into_bytes();
        fs::write(&transcript, &original).unwrap();
        let children = directory.join(&id);
        fs::create_dir(&children).unwrap();
        let controls = vec![
            (
                children.join("child.keep"),
                b"synthetic child stays in place".to_vec(),
            ),
            (
                directory.join("sessions-index.json"),
                b"{\"version\":1,\"entries\":[]}".to_vec(),
            ),
        ];
        for (path, bytes) in &controls {
            fs::write(path, bytes).unwrap();
        }
        let mut profile = Profile::new("native-recycle-fixture", CliKind::Claude);
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
        profile.env.insert(
            "CLAUDE_CONFIG_DIR".into(),
            Override::Set(EnvValue::Literal {
                value: home.to_str().unwrap().into(),
                non_secret: true,
            }),
        );
        backend
            .repository
            .apply(
                backend.repository.read().unwrap().revision,
                Patch::Create { profile },
            )
            .unwrap();
        let context = serde_json::json!({"cli":"claude","profileId":"native-recycle-fixture","profileRevision":backend.repository.get_profile("native-recycle-fixture").unwrap().revision,"projectId":backend.project_id,"projectPath":backend.fixture.project});
        let source_root_key = BackendFixture::scope(
            &backend.service,
            &backend.repository,
            &backend.caller,
            "native-recycle-fixture",
            &backend.project_id,
        )
        .source_root_key;
        Self {
            id,
            context,
            source_root_key,
            transcript,
            original,
            controls,
        }
    }
    pub(super) fn verify(&self, removed: bool) -> Result<(), String> {
        if removed {
            if !matches!(fs::symlink_metadata(&self.transcript), Err(ref error) if error.kind()==io::ErrorKind::NotFound)
            {
                return Err("RECYCLE_ORIGINAL_PRESENT".into());
            }
        } else if fs::read(&self.transcript).ok().as_ref() != Some(&self.original) {
            return Err("RECYCLE_ORIGINAL_CHANGED".into());
        }
        for (path, bytes) in &self.controls {
            if fs::read(path).ok().as_ref() != Some(bytes) {
                return Err("RECYCLE_CONTROL_CHANGED".into());
            }
        }
        Ok(())
    }
}
#[tauri::command]
pub(super) async fn s11_recycle_checkpoint(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    state.admit(&webview, &request)?;
    let q: Named = query(&request)?;
    let mut checks = state.recycle_checks.lock();
    let expected = [("review", 0, 0), ("stored", 1, 0), ("restored", 1, 1)];
    if expected.get(checks.len()).copied()
        != Some((
            q.name.as_str(),
            state.recycle_removes.load(Ordering::Acquire),
            state.recycle_restores.load(Ordering::Acquire),
        ))
    {
        return Err("RECYCLE_CHECKPOINT_INVALID".into());
    }
    state.recycle.verify(q.name == "stored")?;
    state
        .backend
        .lock()
        .repository
        .admission()
        .check_metadata_idle()
        .map_err(|_| "RECYCLE_OWNER_PENDING")?;
    println!("STAGE11_RECYCLE_CHECKPOINT {}", q.name);
    checks.push(q.name);
    Ok(())
}
#[test]
fn NativeDeleteWebView_RecycleFixtureUsesOnlyContainedSyntheticFiles_014() {
    let fixture = Arc::new(Fixture::new().unwrap());
    let backend = BackendFixture::new(fixture.clone(), BackendProgram::UnavailableFixture);
    let recycle = RecycleFixture::new(&backend);
    assert!(recycle.transcript.starts_with(&fixture.root));
    assert!(recycle
        .controls
        .iter()
        .all(|(path, _)| path.starts_with(&fixture.root)));
    recycle.verify(false).unwrap();
    assert_eq!(
        recycle.verify(true).unwrap_err(),
        "RECYCLE_ORIGINAL_PRESENT"
    );
    assert_eq!(recycle.context["cli"], "claude");
    assert_eq!(
        backend
            .repository
            .get_profile("native-recycle-fixture")
            .unwrap()
            .program_path,
        Override::Set(
            fixture
                .project
                .join("must-not-exist.exe")
                .to_str()
                .unwrap()
                .into()
        )
    );
}

impl TaskBackendFixture {
    fn verify_desk_state(&self, archived: bool, opened: bool) -> Result<(), String> {
        let (data, lock) =
            crate::store::data_and_lock_paths().map_err(|_| "DESK_PATH_UNAVAILABLE")?;
        let state = crate::store::read_projects_state_locked(&data, &lock)
            .map_err(|_| "DESK_READ_FAILED")?;
        crate::version_history::maintenance::process_admissions()
            .check_metadata_idle()
            .map_err(|_| "DESK_OWNER_PENDING")?;
        let project = self.recycle.context["projectPath"]
            .as_str()
            .ok_or("DESK_CONTEXT_INVALID")?;
        let normalized = crate::store::normalize_path_str(project);
        let context = serde_json::json!([
            "native-history-v1",
            "claude",
            self.recycle.context["profileId"],
            self.recycle.context["profileRevision"],
            self.recycle.context["projectId"],
            normalized
        ])
        .to_string();
        let adapter = serde_json::json!([
            "local",
            "claude",
            self.recycle.source_root_key,
            self.recycle.id
        ])
        .to_string();
        let expected_key = format!(
            "native-history:{}",
            serde_json::json!(["native-history-v2", context, adapter, self.recycle.id])
        );
        if !state.pinned_projects.is_empty()
            || !state.display_names.is_empty()
            || !state.launch_preferences.is_empty()
        {
            return Err("DESK_UNEXPECTED_METADATA".into());
        }
        if archived {
            if state.archived_sessions.len() != 1
                || state.archived_sessions.get(&normalized) != Some(&vec![expected_key.clone()])
            {
                return Err("DESK_ARCHIVE_INVALID".into());
            }
            let mut known = self.desk_key.lock();
            if known.as_ref().is_some_and(|key| key != &expected_key) {
                return Err("DESK_KEY_CHANGED".into());
            }
            *known = Some(expected_key.clone());
        } else if state
            .archived_sessions
            .values()
            .any(|keys| !keys.is_empty())
        {
            return Err("DESK_RESTORE_INVALID".into());
        }
        if opened {
            let record = state
                .session_records
                .get(&expected_key)
                .ok_or("DESK_RECORD_MISSING")?;
            if state.session_records.len() != 1
                || record.runtime != "native-cli"
                || record.cli != "claude"
                || record.project_path != normalized
                || record.adapter_session_id != adapter
                || record.native_session_id.as_deref() != Some(self.recycle.id.as_str())
                || !record.title.is_empty()
                || !record.last_opened_at.is_some_and(|value| value > 0)
            {
                return Err("DESK_OPENED_AT_MISSING".into());
            }
        } else if !state.session_records.is_empty() {
            return Err("DESK_UNEXPECTED_METADATA".into());
        }
        Ok(())
    }
    pub(super) fn verify_desk_final(&self) -> Result<(), String> {
        if self.desk_checks.lock().len() != 3
            || [
                self.desk_archives.load(Ordering::Acquire),
                self.desk_restores.load(Ordering::Acquire),
                self.desk_upserts.load(Ordering::Acquire),
            ] != [3, 2, 1]
        {
            return Err("DESK_FINAL_DISPATCH_CHANGED".into());
        }
        self.verify_desk_state(true, true)
    }
}
#[tauri::command]
pub(super) async fn s11_desk_checkpoint(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    state.admit(&webview, &request)?;
    let q: Named = query(&request)?;
    let mut checks = state.desk_checks.lock();
    let expected = [
        ("archived", [1, 0, 0]),
        ("restored", [1, 1, 1]),
        ("rearchived", [2, 1, 1]),
    ];
    if expected.get(checks.len()).copied()
        != Some((
            q.name.as_str(),
            [
                state.desk_archives.load(Ordering::Acquire),
                state.desk_restores.load(Ordering::Acquire),
                state.desk_upserts.load(Ordering::Acquire),
            ],
        ))
    {
        return Err("DESK_CHECKPOINT_INVALID".into());
    }
    state.verify_desk_state(q.name != "restored", q.name != "archived")?;
    state.recycle.verify(false)?;
    state.verify_files(false)?;
    println!("STAGE11_DESK_CHECKPOINT {}", q.name);
    checks.push(q.name);
    Ok(())
}
