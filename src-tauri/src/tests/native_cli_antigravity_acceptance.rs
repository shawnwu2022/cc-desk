//! 显式 opt-in 的真实 CLI 冒烟；没有可执行文件时失败，不把夹具成功冒充实机结果。
use super::native_cli_routed_launch::{Fixture, Lease};
use crate::cli::environment::EnvMap;
use crate::cli::profiles::{Launcher, Override, Profile};
use crate::cli::program_discovery::{discover_programs, DiscoveryRequest};
use crate::cli::routed_launch::RoutedResource;
use crate::cli::run_registry::LaunchPhase;
use crate::cli::snapshot::{freeze_launch, FreezeContext};
use crate::cli::storage::{Patch, WorkspaceRepository};
use crate::cli::types::{CliKind, LaunchAction};
use crate::platform::owned_pty::OwnedPty;
use portable_pty::PtySize;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

struct ChildGuard(Arc<RoutedResource<OwnedPty, Lease>>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.0.process.try_wait().ok().flatten().is_none() {
            let _ = self.0.process.terminate_root();
            if Instant::now() >= deadline {
                eprintln!("AGY_REAL_PTY cleanup_unconfirmed: owned child did not report exit");
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
#[allow(non_snake_case)]
fn Antigravity_InstalledDiscoveryAndOwnedTui_001() {
    bundled_runtime::initialize().unwrap();
    let installed = PathBuf::from(
        std::env::var_os("CC_DESK_AGY_SMOKE_EXE").expect("explicit installed agy path required"),
    );
    assert!(installed.is_absolute() && installed.is_file());
    let expected = std::fs::canonicalize(&installed).unwrap();
    let mut f = Fixture::<OwnedPty>::new();
    let project = f.root().join("empty-project");
    let home = f.root().join("isolated-home");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    // 不把真实凭据环境或用户配置传给 CLI；只保留启动和终端必需的宿主值。
    let mut inherited: EnvMap = std::env::vars_os()
        .filter(|(key, _)| {
            matches!(
                key.to_string_lossy().to_ascii_uppercase().as_str(),
                "PATH" | "SYSTEMROOT" | "WINDIR" | "COMSPEC" | "PATHEXT" | "TEMP" | "TMP"
            )
        })
        .collect();
    for key in ["HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA"] {
        inherited.insert(key.into(), home.as_os_str().into());
    }
    let repo = WorkspaceRepository::open(f.root().join("desk/workspace.json")).unwrap();
    let registered = crate::cli::workspace::register_project(&repo, &project).unwrap();
    let document = repo
        .apply(
            repo.read().unwrap().revision,
            Patch::Create {
                profile: Profile::new("agy-smoke", CliKind::Antigravity),
            },
        )
        .unwrap();
    let discovery = discover_programs(
        &repo,
        "main",
        &DiscoveryRequest {
            profile_id: "agy-smoke".into(),
            expected_revision: document.profiles["agy-smoke"].revision,
            project_id: registered.project_id,
        },
        &inherited,
    )
    .unwrap();
    let selected = discovery
        .candidates
        .iter()
        .find(|candidate| std::fs::canonicalize(&candidate.program_path).unwrap() == expected)
        .expect("actual installed agy must be discovered from host PATH");
    assert_eq!(selected.launcher, Launcher::Native);
    assert_eq!(
        repo.get_profile("agy-smoke").unwrap().program_path,
        Override::Inherit
    );
    let changes = serde_json::json!({"programPath":{"mode":"set","value":selected.program_path},"observer":{"mode":"set","value":false}}).as_object().unwrap().clone();
    let selected_document = repo
        .apply(
            document.revision,
            Patch::Update {
                id: "agy-smoke".into(),
                changes,
            },
        )
        .unwrap();
    f.profile = selected_document.profiles["agy-smoke"].clone();
    f.request.profile_id = f.profile.id.clone();
    f.request.expected_profile_revision = f.profile.revision;
    f.request.cli = CliKind::Antigravity;
    f.request.launch_cwd = project.to_str().unwrap().into();
    f.request.action = LaunchAction::New;
    let empty = EnvMap::new();
    let snapshot = freeze_launch(
        &f.request,
        &f.profile,
        &f.caller,
        &FreezeContext {
            inherited: &inherited,
            terminal: &empty,
            legacy: None,
            observer: None,
        },
    )
    .unwrap();
    assert_eq!(std::fs::canonicalize(snapshot.program()).unwrap(), expected);
    assert!(!snapshot.observer_requested());
    let status = f
        .driver
        .start_pty(&f.caller, &f.request, || Ok(snapshot), |_| Ok(f.lease()))
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    let child = ChildGuard(
        f.driver
            .registry()
            .resource(&f.caller, &status.run)
            .unwrap(),
    );
    let mut reader = child.0.process.take_reader().unwrap();
    let (tx, rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let reading = std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    if tx.send(buffer[..size].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
        let _ = done_tx.send(());
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut output = Vec::new();
    while !String::from_utf8_lossy(&output).contains("Antigravity CLI") {
        assert!(Instant::now() < deadline, "real TUI banner not observed");
        if let Ok(bytes) = rx.recv_timeout(Duration::from_millis(100)) {
            output.extend(bytes);
        }
        assert!(
            output.len() <= 512 * 1024,
            "bounded startup output exceeded"
        );
        assert!(
            child.0.process.try_wait().unwrap().is_none(),
            "CLI exited before interactive banner"
        );
    }
    child
        .0
        .process
        .resize(PtySize {
            cols: 100,
            rows: 30,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    // 只发送取消键，绝不确认登录、工作区信任、偏好或任何任务。
    for _ in 0..2 {
        if child.0.process.try_wait().unwrap().is_some() {
            break;
        }
        child
            .0
            .process
            .with_writer(|writer| {
                writer.write_all(b"\x03")?;
                writer.flush()
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));
    }
    // 未认证启动阶段不保证 Ctrl+C 生效；单独核验 Desk 的 owned stop，不伪称优雅退出。
    let grace_deadline = Instant::now() + Duration::from_secs(3);
    while child.0.process.try_wait().unwrap().is_none() && Instant::now() < grace_deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let keyboard_exited = child.0.process.try_wait().unwrap().is_some();
    if !keyboard_exited {
        child.0.process.terminate_root().unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    let exit = loop {
        if let Some(exit) = child.0.process.try_wait().unwrap() {
            break exit;
        }
        assert!(
            Instant::now() < deadline,
            "owned stop did not produce confirmed exit"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(exit.exit_code(), if keyboard_exited { 0 } else { 1 });
    f.driver.registry().mark_exited(&status.run).unwrap();
    f.driver.registry().retire(&status.run).unwrap();
    drop(child);
    done_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("reader close unconfirmed");
    reading.join().unwrap();
    println!("AGY_REAL_PTY discovery=selected-exact-path new=tui-banner resize-call=ok keyboard-exited={keyboard_exited} owned-stop={} confirmed-exit={} reader=closed login=not-confirmed task=not-sent", !keyboard_exited, exit.exit_code());
}
