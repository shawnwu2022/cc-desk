//! Actual isolated registry/source process probes; never changes installed product state.
use super::fixture_process::{await_fixture_release, FixtureChild};
use crate::version_history::windows::{
    process::ExactProcess, scope::RegisteredInstallation, security::CurrentUser,
};
use windows::Win32::System::Registry::*;
use windows_core::PCWSTR;

struct RegistryFixture {
    path: Vec<u16>,
    user: HKEY,
    machine: HKEY,
}
impl RegistryFixture {
    fn new() -> Self {
        let path = format!(
            "Software\\CCDeskRegistrationTests\\{}",
            uuid::Uuid::new_v4()
        );
        Self {
            user: Self::key(HKEY_CURRENT_USER, &format!("{path}\\User")),
            machine: Self::key(HKEY_CURRENT_USER, &format!("{path}\\Machine")),
            path: path.encode_utf16().chain(Some(0)).collect(),
        }
    }
    fn key(root: HKEY, path: &str) -> HKEY {
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                None,
            )
            .ok()
            .unwrap();
        }
        key
    }
    fn value(&self, path: &str, name: &str, kind: REG_VALUE_TYPE, value: &[u8]) {
        let key = Self::key(self.user, path);
        let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            RegSetValueExW(key, PCWSTR(name.as_ptr()), None, kind, Some(value))
                .ok()
                .unwrap();
            RegCloseKey(key).ok().unwrap();
        }
    }
    fn string(&self, path: &str, name: &str, value: &str) {
        self.value(
            path,
            name,
            REG_SZ,
            &value
                .encode_utf16()
                .chain(Some(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        );
    }
    fn install(&self, path: &std::path::Path) {
        let root = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk";
        for (name, value) in [
            ("DisplayName", "CC Desk".to_owned()),
            ("Publisher", "shawnwu2022".into()),
            ("MainBinaryName", "cc-desk.exe".into()),
            ("DisplayVersion", env!("CARGO_PKG_VERSION").into()),
            ("InstallLocation", format!("\"{}\"", path.display())),
            (
                "DisplayIcon",
                format!("\"{}\"", path.join("cc-desk.exe").display()),
            ),
            (
                "UninstallString",
                format!("\"{}\"", path.join("uninstall.exe").display()),
            ),
        ] {
            self.string(root, name, &value);
        }
        self.string("Software\\shawnwu2022\\CC Desk", "", path.to_str().unwrap());
    }
}
impl Drop for RegistryFixture {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.user);
            let _ = RegCloseKey(self.machine);
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.path.as_ptr()));
        }
    }
}

// 检查拷贝manager按真实source进程重新准入，实际wait后消耗image guards才允许原对象fence。
#[test]
fn HistoryRegistration_SourceReadmission_001() {
    let user = CurrentUser::capture().unwrap();
    if user.require_unelevated().is_err() {
        let process = ExactProcess::capture_observed(std::process::id()).unwrap();
        assert!(process.verify_current_user(&user).is_err());
        // The copied-source positive branch needs the actual Medium worker.
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let image = temporary.path().join("cc-desk.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &image).unwrap();
    let marker = temporary.path().join("source-marker");
    let release = temporary.path().join("release-source");
    let mut child = FixtureChild::new(
        std::process::Command::new(&image)
            .args([
                "--exact",
                "tests::version_history_registration_windows::HistoryRegistration_SourceWorker_004",
                "--ignored",
            ])
            .env("CC_DESK_HISTORY_PROBE_MARKER", &marker)
            .env("CC_DESK_HISTORY_PROBE_RELEASE", &release)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let registry = RegistryFixture::new();
    registry.install(temporary.path());
    let capture = || {
        RegisteredInstallation::fixture_for_source(
            ExactProcess::capture_observed(child.id()).unwrap(),
            &image,
            registry.user,
            registry.machine,
        )
    };
    let installed = capture().unwrap();
    installed.recheck().unwrap();
    assert!(capture().unwrap().release_after_exit().is_err());
    let wrong = ExactProcess::capture_observed(std::process::id()).unwrap();
    assert!(RegisteredInstallation::fixture_for_source(
        wrong,
        &image,
        registry.user,
        registry.machine
    )
    .is_err());
    std::fs::write(&release, b"release").unwrap();
    child.release(b'R').unwrap();
    assert!(child
        .wait_bounded(std::time::Duration::from_secs(5))
        .unwrap()
        .success());
    let exited = installed.release_after_exit().unwrap();
    exited.verify().unwrap();
    let fence = std::sync::Arc::new(parking_lot::Mutex::new(
        exited.acquire_source_fence().unwrap(),
    ));
    let fenced = exited.into_fenced(fence.clone()).unwrap();
    fenced.verify().unwrap();
    fenced.verify_fence(&fence.lock()).unwrap();
    // Registry readers have been consumed; replacing source registration no
    // longer invalidates the historical scope/terminal proof or retains keys.
    let product: Vec<_> = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        RegDeleteTreeW(registry.user, PCWSTR(product.as_ptr()))
            .ok()
            .unwrap();
    }
    registry.install(temporary.path());
    fenced.verify().unwrap();
    let other_path = temporary.path().join("other.exe");
    std::fs::write(&other_path, b"foreign image").unwrap();
    use crate::version_history::windows::{
        fence::ImageFence,
        files::{ComponentName, FileAccess},
    };
    let other_name = ComponentName::new(std::ffi::OsStr::new("other.exe")).unwrap();
    let other = fenced
        .directory()
        .open_file(other_name.clone(), FileAccess::Read)
        .unwrap();
    let identity = other.identity().clone();
    let digest = other.digest().unwrap();
    drop(other);
    let other =
        ImageFence::acquire(fenced.directory().clone(), other_name, &identity, &digest).unwrap();
    assert!(fenced.verify_fence(&other).is_err());
    fence
        .lock()
        .rename_to(
            fenced.directory().clone(),
            ComponentName::new(std::ffi::OsStr::new("source-fenced.exe")).unwrap(),
        )
        .unwrap();
    fenced.verify_fence(&fence.lock()).unwrap();
}

// 检查实际retained process token被查询，当前未提升用户来源可验证，不从PID或路径猜SID。
#[test]
fn HistoryRegistration_SourceToken_002() {
    let user = CurrentUser::capture().unwrap();
    let process = ExactProcess::capture_observed(std::process::id()).unwrap();
    if user.require_unelevated().is_ok() {
        process.verify_current_user(&user).unwrap();
    } else {
        assert!(process.verify_current_user(&user).is_err());
    }
}

// 检查source selector仅来自实际retained image对象，不依赖前端或旧存储路径。
#[test]
fn HistoryRegistration_ObservedImagePath_003() {
    let process = ExactProcess::capture_observed(std::process::id()).unwrap();
    let path = process.observed_image_path().unwrap();
    use crate::version_history::windows::files::{ComponentName, Directory, FileAccess};
    let parent = Directory::open_absolute(path.parent().unwrap()).unwrap();
    let image = parent
        .open_file(
            ComponentName::new(path.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    process.verify_held_image(&image).unwrap();
    let foreign = tempfile::tempdir().unwrap();
    std::fs::copy(&path, foreign.path().join("copied.exe")).unwrap();
    let parent = Directory::open_absolute(foreign.path()).unwrap();
    let image = parent
        .open_file(
            ComponentName::new(std::ffi::OsStr::new("copied.exe")).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    assert!(process.verify_held_image(&image).is_err());
}

// Source lifetime is authorized only by its exact owning supervisor.
#[test]
#[ignore = "explicitly supervised by the copied-source registration fixture"]
fn HistoryRegistration_SourceWorker_004() {
    let marker =
        std::path::PathBuf::from(std::env::var_os("CC_DESK_HISTORY_PROBE_MARKER").unwrap());
    std::fs::write(&marker, b"executed").unwrap();
    await_fixture_release(b'R');
    let release = std::env::var_os("CC_DESK_HISTORY_PROBE_RELEASE").unwrap();
    assert_eq!(std::fs::read(release).unwrap(), b"release");
    std::fs::write(marker.with_extension("completed"), b"completed").unwrap();
}
