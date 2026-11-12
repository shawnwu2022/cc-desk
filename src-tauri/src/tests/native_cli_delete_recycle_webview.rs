use super::*;

pub(super) struct RecycleFixture {
    pub(super) id: String,
    pub(super) context: serde_json::Value,
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
        Self {
            id,
            context,
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
