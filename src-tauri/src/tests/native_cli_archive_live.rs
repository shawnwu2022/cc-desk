//! 固定已安装 CLI 的隔离并发锁实验，不建立真实用户会话或推断其他版本能力。
use super::*;
use crate::cli::native_archive::ArchiveRequest;

fn initialize_fixture(fixture: &Arc<Fixture>, id: &str) {
    let mut command = Command::new(CODEX_EXE);
    command.args(["app-server", "--strict-config"]);
    let task = start_owned(
        fixture.clone(),
        "archive-initialize".into(),
        command,
        Some(verified_executable().unwrap()),
        false,
        true,
        Some(ReadOnlyProtocol::new(fixture, id.into())),
    )
    .unwrap();
    let report = task.wait_until_settled().unwrap();
    assert!(report.status.success() && !report.forced);
    let trace = report.protocol.unwrap();
    assert!(
        trace.initialize_acknowledged
            && trace.home_verified
            && trace.read_acknowledged
            && trace.thread_id_matched
    );
}

#[test]
#[ignore = "explicit isolated installed Codex archive lock experiment only"]
fn NativeArchiveLive_InstalledCodexLockMatrix_001() {
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_ARCHIVE_LIVE_GO").as_deref(),
        Ok("archive-lock-matrix")
    );
    for held in ["unrelated", "root", "descendant"] {
        system_configuration_absent().unwrap();
        let fixture = Arc::new(Fixture::new().unwrap());
        let _executable_lease = verified_executable().unwrap();
        let _configuration_lease = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0)
            .open(fixture.root.join("codex-home/config.toml"))
            .unwrap();
        let backend = BackendFixture::new(fixture.clone(), BackendProgram::InstalledPinned);
        let home = fixture.root.join("codex-home");
        let child_id = uuid::Uuid::new_v4().to_string();
        let (child, child_original) = synthetic_rollout(&fixture, &home, &child_id, false);
        let unrelated_id = uuid::Uuid::new_v4().to_string();
        let (unrelated, unrelated_original) =
            synthetic_rollout(&fixture, &home, &unrelated_id, false);
        let root_original = fs::read(&backend.active_rollout).unwrap();
        initialize_fixture(&fixture, &backend.active_id);
        // 只修改本次新建隔离库，关系格式来自固定版本的原生迁移与官方归档用例。
        let database = fixture.root.join("sqlite/state_5.sqlite");
        let connection = rusqlite::Connection::open_with_flags(
            &database,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .unwrap();
        connection
            .execute(
                "INSERT INTO thread_spawn_edges (parent_thread_id, child_thread_id, status) VALUES (?1, ?2, 'open')",
                rusqlite::params![backend.active_id, child_id],
            )
            .unwrap();
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM thread_spawn_edges WHERE parent_thread_id = ?1 AND child_thread_id = ?2",
                    rusqlite::params![backend.active_id, child_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );
        drop(connection);
        let held_id = match held {
            "root" => &backend.active_id,
            "descendant" => &child_id,
            _ => &unrelated_id,
        };
        let writer = NativeWriterLock::acquire(&fixture, held_id).unwrap();
        assert!(writer.contended().unwrap());
        let request = ArchiveRequest {
            operation_id: uuid::Uuid::new_v4().to_string(),
            source: backend.source.clone(),
            session_key: serde_json::json!([
                "local",
                "codex",
                backend.source.source_root_key,
                backend.active_id
            ])
            .to_string(),
        };
        let result = backend.service.archive(&backend.caller, request);
        backend.launch.wait_for_commands();
        system_configuration_absent().unwrap();
        assert!(backend.repository.admission().check_metadata_idle().is_ok());
        assert!(writer.contended().unwrap());
        let archived_root = home
            .join("archived_sessions")
            .join(backend.active_rollout.file_name().unwrap());
        let archived_child = home
            .join("archived_sessions")
            .join(child.file_name().unwrap());
        let success = result.is_ok();
        let safe_code = result.as_ref().err().map(|failure| failure.code.clone());
        if held == "unrelated" {
            assert!(success, "unrelated native writer lock: {safe_code:?}");
            assert!(!backend.active_rollout.exists() && !child.exists());
            assert_eq!(fs::read(&archived_root).unwrap(), root_original);
            assert_eq!(fs::read(&archived_child).unwrap(), child_original);
        } else {
            assert_eq!(safe_code.as_deref(), Some("ARCHIVE_OUTCOME_UNKNOWN"));
            assert_eq!(fs::read(&backend.active_rollout).unwrap(), root_original);
            assert_eq!(fs::read(&child).unwrap(), child_original);
            assert!(!archived_root.exists() && !archived_child.exists());
        }
        assert_eq!(fs::read(&unrelated).unwrap(), unrelated_original);
        assert_eq!(fs::read(&backend.rollout).unwrap(), backend.original);
        drop(writer);
        if !success {
            // 只在隔离实验重建服务作释放锁正对照；生产未知写屏障不得这样绕过。
            let service = ProjectionService::new(backend.launch.clone());
            let source = BackendFixture::scope(
                &service,
                &backend.repository,
                &backend.caller,
                "native-delete-fixture",
                &backend.project_id,
            );
            let released = service.archive(
                &backend.caller,
                ArchiveRequest {
                    operation_id: uuid::Uuid::new_v4().to_string(),
                    session_key: serde_json::json!([
                        "local",
                        "codex",
                        source.source_root_key,
                        backend.active_id
                    ])
                    .to_string(),
                    source,
                },
            );
            backend.launch.wait_for_commands();
            assert!(
                released.is_ok(),
                "released {held} lock: {:?}",
                released.as_ref().err()
            );
            assert!(backend.repository.admission().check_metadata_idle().is_ok());
            assert!(!backend.active_rollout.exists() && !child.exists());
            assert_eq!(fs::read(&archived_root).unwrap(), root_original);
            assert_eq!(fs::read(&archived_child).unwrap(), child_original);
            assert_eq!(fs::read(&unrelated).unwrap(), unrelated_original);
            assert_eq!(fs::read(&backend.rollout).unwrap(), backend.original);
            system_configuration_absent().unwrap();
        }
        let report = serde_json::json!({
            "version":CODEX_VERSION,"heldLock":held,"archiveSucceeded":success,
            "safeCode":safe_code,"nativeWriterLockContended":true,
            "syntheticWriterLockOnly":true,"runningModelWriter":false,
            "releasePositiveControl":!success,"freshTestServiceAfterRelease":!success,
            "rootAndChildChecked":true,"unrelatedOriginalUnchanged":true,
            "rootReaped":true,"jobEmpty":true,"allDataIsolated":true
        });
        fs::write(
            fixture.root.join("reports/archive-lock-result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("NATIVE_ARCHIVE_LIVE_LOCK {report}");
    }
}
