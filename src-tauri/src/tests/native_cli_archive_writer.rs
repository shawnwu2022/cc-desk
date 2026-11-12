//! 只加载合成会话的真实 writer；不发送模型 turn、不触碰真实会话。
use super::*;
use serde_json::{json, Value};

fn host_skills_absent() -> io::Result<()> {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::UI::Shell::FOLDERID_Profile;
    // Windows dirs::home_dir 不遵从 USERPROFILE，必须独立核对 KnownFolder。
    let allocation = unsafe { SHGetKnownFolderPath(&FOLDERID_Profile, KF_FLAG_DEFAULT, None) }
        .map_err(io::Error::other)?;
    let folder = unsafe { allocation.to_string() };
    unsafe { CoTaskMemFree(Some(allocation.0.cast())) };
    let parent = PathBuf::from(folder.map_err(io::Error::other)?).join(".agents");
    match fs::symlink_metadata(&parent) {
        Ok(meta) if meta.is_dir() && meta.file_attributes() & 0x400 == 0 => {
            namespace_absent(&parent.join("skills"))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(io::Error::other("HOST_SKILLS_NOT_PROVEN_ABSENT")),
    }
}

fn writer_contended(fixture: &Fixture, id: &str) -> io::Result<bool> {
    let file = NativeWriterLock::open(
        &fixture
            .root
            .join(format!("codex-home/thread-writer-locks/{id}.lock")),
        false,
    )?;
    Ok(matches!(
        file.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ))
}

struct LiveWriter {
    release: Option<mpsc::SyncSender<()>>,
    finished: mpsc::Receiver<io::Result<()>>,
    fixture: Arc<Fixture>,
    id: String,
}
impl LiveWriter {
    fn start(fixture: Arc<Fixture>, id: String) -> io::Result<Self> {
        system_configuration_absent()?;
        host_skills_absent()?;
        let executable = verified_executable()?;
        let configuration = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(fixture.root.join("codex-home/config.toml"))?;
        let (ready, readiness) = mpsc::sync_channel(1);
        let (release, released) = mpsc::sync_channel(1);
        let (done, finished) = mpsc::sync_channel(1);
        let owned_fixture = fixture.clone();
        let owned_id = id.clone();
        std::thread::Builder::new()
            .name("isolated-native-writer".into())
            .spawn(move || {
                let (_executable, _configuration) = (executable, configuration);
                let result = run_writer(&owned_fixture, &owned_id, ready, released);
                let result = result
                    .and_then(|()| system_configuration_absent())
                    .and_then(|()| host_skills_absent());
                let _ = done.send(result);
            })?;
        let writer = Self {
            release: Some(release),
            finished,
            fixture,
            id,
        };
        readiness
            .recv_timeout(Duration::from_secs(35))
            .map_err(|_| io::Error::other("NATIVE_WRITER_NOT_READY"))??;
        writer.assert_live()?;
        Ok(writer)
    }
    fn assert_live(&self) -> io::Result<()> {
        match self.finished.try_recv() {
            Err(mpsc::TryRecvError::Empty) => {}
            _ => return Err(io::Error::other("NATIVE_WRITER_EXITED_EARLY")),
        }
        if writer_contended(&self.fixture, &self.id)? {
            Ok(())
        } else {
            Err(io::Error::other("NATIVE_WRITER_LEASE_NOT_HELD"))
        }
    }
    fn finish(mut self) -> io::Result<()> {
        self.release.take().map(|sender| sender.try_send(()));
        self.finished
            .recv()
            .map_err(|_| io::Error::other("NATIVE_WRITER_OWNER_LOST"))?
    }
}
impl Drop for LiveWriter {
    fn drop(&mut self) {
        if let Some(sender) = self.release.take() {
            let _ = sender.try_send(());
            // 即使测试 panic，也等自己创建的 root 与 Job 回收后才释放隔离资源。
            while let Err(mpsc::RecvTimeoutError::Timeout) =
                self.finished.recv_timeout(Duration::from_secs(5))
            {
                eprintln!("NATIVE_ARCHIVE_WRITER_CLEANUP_PENDING");
            }
        }
    }
}

type FrameSender = mpsc::SyncSender<(Vec<u8>, mpsc::SyncSender<bool>)>;
fn send(sender: &FrameSender, frame: Value) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(&frame)?;
    if bytes.len() >= 4096 {
        return Err(io::Error::other("WRITER_REQUEST_LIMIT"));
    }
    bytes.push(b'\n');
    let (ack, received) = mpsc::sync_channel(1);
    sender
        .try_send((bytes, ack))
        .map_err(|_| io::Error::other("WRITER_PIPE_UNAVAILABLE"))?;
    match received.recv_timeout(Duration::from_secs(2)) {
        Ok(true) => Ok(()),
        _ => Err(io::Error::other("WRITER_PIPE_SEND_UNCONFIRMED")),
    }
}

fn run_writer(
    fixture: &Fixture,
    id: &str,
    ready: mpsc::SyncSender<io::Result<()>>,
    released: mpsc::Receiver<()>,
) -> io::Result<()> {
    let stdout = fixture.root.join("reports/live-writer.stdout");
    let stderr = fixture.root.join("reports/live-writer.stderr");
    let job = ProbeJob::new()?;
    let mut command = Command::new(CODEX_EXE);
    command
        .args(["app-server", "--strict-config"])
        .env_clear()
        .envs(&fixture.environment)
        .current_dir(&fixture.project)
        .stdin(Stdio::piped())
        .stdout(File::create(&stdout)?)
        .stderr(File::create(&stderr)?)
        .creation_flags(0x08000000 | 0x4);
    let mut child = command.spawn()?;
    let assigned = job.assign_and_resume(&child);
    if assigned.is_err() {
        let _ = child.kill();
    }
    let mut frame_sender = None;
    let result = (|| -> io::Result<()> {
        assigned?;
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("WRITER_STDIN_MISSING"))?;
        let (sender, receiver) = mpsc::sync_channel::<(Vec<u8>, mpsc::SyncSender<bool>)>(1);
        std::thread::Builder::new()
            .name("isolated-native-writer-input".into())
            .spawn(move || {
                while let Ok((bytes, ack)) = receiver.recv() {
                    let ok = input.write_all(&bytes).and_then(|_| input.flush()).is_ok();
                    let _ = ack.try_send(ok);
                    if !ok {
                        break;
                    }
                }
            })?;
        frame_sender = Some(sender);
        let sender = frame_sender.as_ref().unwrap();
        send(
            sender,
            json!({"id":1,"method":"initialize","params":{
            "clientInfo":{"name":"cc_desk_archive_fixture","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"explicitGatewayOauth":true,"experimentalApi":false}}}),
        )?;
        let deadline = Instant::now() + Duration::from_secs(150);
        let ready_deadline = Instant::now() + Duration::from_secs(30);
        let home_key = Root::open(&fixture.root.join("codex-home"))
            .map_err(io::Error::other)?
            .key()
            .to_owned();
        let mut offset = 0;
        let mut initialized = false;
        let mut resumed = false;
        let mut notified = false;
        loop {
            if child.try_wait()?.is_some() || job.empty()? {
                return Err(io::Error::other("NATIVE_WRITER_EARLY_EXIT"));
            }
            if Instant::now() >= deadline || (!notified && Instant::now() >= ready_deadline) {
                return Err(io::Error::other("NATIVE_WRITER_DEADLINE"));
            }
            if fixture
                .leases
                .iter()
                .any(|root| root.source_current().is_err())
            {
                return Err(io::Error::other("WRITER_FIXTURE_CHANGED"));
            }
            for path in [&stdout, &stderr] {
                if fs::metadata(path)?.len() > OUTPUT_LIMIT {
                    return Err(io::Error::other("WRITER_OUTPUT_LIMIT"));
                }
            }
            let bytes = fs::read(&stdout)?;
            while let Some(end) = bytes[offset..].iter().position(|byte| *byte == b'\n') {
                if end > 65536 {
                    return Err(io::Error::other("WRITER_FRAME_LIMIT"));
                }
                let frame: Value = serde_json::from_slice(&bytes[offset..offset + end])?;
                offset += end + 1;
                if frame.get("method").is_some() {
                    if frame.get("id").is_some() {
                        return Err(io::Error::other("WRITER_SERVER_REQUEST"));
                    }
                    continue;
                }
                if frame.get("error").is_some() {
                    return Err(io::Error::other("NATIVE_WRITER_RPC_ERROR"));
                }
                match frame["id"].as_u64() {
                    Some(1) if !initialized => {
                        let home = frame["result"]["codexHome"]
                            .as_str()
                            .and_then(|path| Root::open(Path::new(path)).ok());
                        if home.is_none_or(|home| home.key() != home_key) {
                            return Err(io::Error::other("WRITER_HOME_MISMATCH"));
                        }
                        initialized = true;
                        send(sender, json!({"method":"initialized"}))?;
                        send(
                            sender,
                            json!({"id":2,"method":"thread/resume","params":{"threadId":id,"cwd":fixture.project}}),
                        )?;
                    }
                    Some(2) if initialized && !resumed => {
                        if frame["result"]["thread"]["id"] != id {
                            return Err(io::Error::other("WRITER_THREAD_MISMATCH"));
                        }
                        resumed = true;
                    }
                    _ => return Err(io::Error::other("WRITER_UNEXPECTED_RESPONSE")),
                }
            }
            if bytes.len() - offset > 65536 {
                return Err(io::Error::other("WRITER_FRAME_LIMIT"));
            }
            if resumed && !notified && writer_contended(fixture, id)? {
                ready
                    .try_send(Ok(()))
                    .map_err(|_| io::Error::other("WRITER_CALLER_GONE"))?;
                notified = true;
            }
            match released.try_recv() {
                Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {}
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    })();
    if let Err(error) = &result {
        let _ = ready.try_send(Err(io::Error::other(error.to_string())));
    }
    // 不发 Shutdown。EOF 后只终止本测试创建且精确持有的 Job。
    drop(frame_sender);
    child.stdin.take();
    let grace = Instant::now() + Duration::from_secs(2);
    let mut root_reaped = false;
    loop {
        if !root_reaped {
            root_reaped = matches!(child.try_wait(), Ok(Some(_)));
        }
        if root_reaped && matches!(job.empty(), Ok(true)) {
            break;
        }
        if Instant::now() >= grace {
            let _ = job.terminate();
            let _ = child.kill();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    result
}

#[test]
#[ignore = "explicit isolated installed Codex loaded-writer experiment only"]
fn NativeArchiveLive_LoadedWriterScopedMatrix_002() {
    use crate::version_history::{
        codex_archive_admission::CodexOwnerIdentity, maintenance::RuntimeKind,
    };
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_ARCHIVE_LIVE_GO").as_deref(),
        Ok("archive-loaded-writer-matrix")
    );
    for held in [
        "known-target",
        "known-descendant",
        "known-unrelated",
        "external-target",
        "external-descendant",
    ] {
        system_configuration_absent().unwrap();
        host_skills_absent().unwrap();
        let fixture = Arc::new(Fixture::new().unwrap());
        let backend = BackendFixture::new(fixture.clone(), BackendProgram::InstalledPinned);
        let home = fixture.root.join("codex-home");
        let child_id = uuid::Uuid::new_v4().to_string();
        let (child, _) = synthetic_rollout(&fixture, &home, &child_id, false);
        // 原生 read 的父身份来自持久化 header；不能只造 DB 边却声明独立 cli。
        let mut header: Value = serde_json::from_slice(&fs::read(&child).unwrap()).unwrap();
        header["payload"]["parent_thread_id"] = json!(backend.active_id);
        header["payload"]["session_id"] = json!(backend.active_id);
        header["payload"]["source"] = json!({"subagent":{"thread_spawn":{
            "parent_thread_id":backend.active_id,"depth":1}}});
        fs::write(&child, format!("{header}\n")).unwrap();
        let unrelated_id = uuid::Uuid::new_v4().to_string();
        let (unrelated, _) = synthetic_rollout(&fixture, &home, &unrelated_id, false);
        initialize_fixture(&fixture, &backend.active_id);
        let database =
            rusqlite::Connection::open(fixture.root.join("sqlite/state_5.sqlite")).unwrap();
        database.execute("INSERT OR REPLACE INTO thread_spawn_edges(parent_thread_id,child_thread_id,status) VALUES (?1,?2,'open')", rusqlite::params![backend.active_id,child_id]).unwrap();
        drop(database);
        let held_id = if held.ends_with("target") {
            &backend.active_id
        } else if held.ends_with("descendant") {
            &child_id
        } else {
            &unrelated_id
        };
        let writer = LiveWriter::start(fixture.clone(), held_id.clone()).unwrap();
        let owner = held.starts_with("known-").then(|| {
            backend
                .repository
                .admission()
                .begin_start(RuntimeKind::Codex({
                    let id = uuid::Uuid::parse_str(held_id).unwrap();
                    let identity = CodexOwnerIdentity::new(Some(id));
                    identity.observe(id);
                    identity
                }))
                .unwrap()
                .child_created()
        });
        let root_bytes = fs::read(&backend.active_rollout).unwrap();
        let child_bytes = fs::read(&child).unwrap();
        let unrelated_bytes = fs::read(&unrelated).unwrap();
        let result = backend.service.archive(
            &backend.caller,
            ArchiveRequest {
                operation_id: uuid::Uuid::new_v4().to_string(),
                source: backend.source.clone(),
                session_key: json!([
                    "local",
                    "codex",
                    backend.source.source_root_key,
                    backend.active_id
                ])
                .to_string(),
            },
        );
        backend.launch.wait_for_commands();
        writer.assert_live().unwrap();
        let code = result.as_ref().err().map(|error| error.code.as_str());
        let archived_root = home
            .join("archived_sessions")
            .join(backend.active_rollout.file_name().unwrap());
        let archived_child = home
            .join("archived_sessions")
            .join(child.file_name().unwrap());
        if held == "known-unrelated" {
            assert!(result.is_ok(), "{held}: {code:?}");
            assert!(!backend.active_rollout.exists() && !child.exists());
            assert_eq!(fs::read(archived_root).unwrap(), root_bytes);
            assert_eq!(fs::read(archived_child).unwrap(), child_bytes);
        } else {
            assert_eq!(
                code,
                Some(if held.starts_with("known-") {
                    "ARCHIVE_BUSY"
                } else {
                    "ARCHIVE_OUTCOME_UNKNOWN"
                }),
                "{held}"
            );
            assert_eq!(fs::read(&backend.active_rollout).unwrap(), root_bytes);
            assert_eq!(fs::read(&child).unwrap(), child_bytes);
            assert!(!archived_root.exists() && !archived_child.exists());
        }
        assert_eq!(fs::read(unrelated).unwrap(), unrelated_bytes);
        assert_eq!(fs::read(&backend.rollout).unwrap(), backend.original);
        writer.finish().unwrap();
        if let Some(owner) = owner {
            owner.reaped();
        }
        assert!(backend.repository.admission().check_metadata_idle().is_ok());
        let report = json!({"version":CODEX_VERSION,"case":held,"safeCode":code,"archiveSucceeded":result.is_ok(),
            "loadedNativeWriter":true,"modelTurnSent":false,"syntheticDeskAdmission":held.starts_with("known-"),
            "writerLeaseHeldAfterArchive":true,"rootAndChildChecked":true,"rootReaped":true,"jobEmpty":true,
            "hostSkillsAbsentBeforeAndAfter":true,"allSessionDataSynthetic":true});
        fs::write(
            fixture
                .root
                .join("reports/archive-loaded-writer-result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("NATIVE_ARCHIVE_LOADED_WRITER {report}");
    }
}
