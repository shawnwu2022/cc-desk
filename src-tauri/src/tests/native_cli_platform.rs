use super::native_cli_harness::ProbeReport;
use crate::cli::environment::EnvMap;
use crate::cli::invocation::build_invocation;
use crate::cli::profiles::{Dialect, Launcher, Override, Profile};
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, WireU64};
use crate::platform::launch::{resolve_process, spawn_process, ProcessLaunchSpec};
use portable_pty::PtySize;
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../conpty_runtime.rs"]
mod bundled_runtime;

const PAYLOAD: &[&str] = &[
    "a b", "", "中文", "quote\"inside", "single'inside", "$HOME", "%TEMP%", "!x!",
    "^", "&", "|", "C:\\tail\\", "line\nbreak", "$(echo should-not-run)", "/opaque/path",
    "--future", "--", "",
];

struct Fixture {
    temp: tempfile::TempDir,
    cwd: PathBuf,
    report: PathBuf,
    node: String,
    profile: Profile,
    request: LaunchRequest,
    inherited: EnvMap,
}

impl Fixture {
    fn new(payload: &[&str]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("工作 目录");
        fs::create_dir(&cwd).unwrap();
        let report = temp.path().join("report.json");
        let node = crate::platform::find_executable("node").expect("Node.js is required");
        let probe = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent().unwrap().join("tests/fixtures/native-cli/probe.mjs");
        let mut argv = vec![
            probe.to_str().unwrap().into(), "--report".into(),
            report.to_str().unwrap().into(), "--".into(),
        ];
        argv.extend(payload.iter().map(|s| (*s).to_string()));
        let mut profile = Profile::new("platform-test", CliKind::Codex);
        profile.program_path = Override::Set(node.clone());
        let request = LaunchRequest {
            request_id: "request".into(), tab_id: "tab".into(), run_id: "run".into(),
            generation: 1, profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision, cli: CliKind::Codex,
            launch_cwd: cwd.to_str().unwrap().into(), action: LaunchAction::Raw { argv },
            extra_args: vec![], cols: 100, rows: 30,
        };
        let mut inherited: EnvMap = std::env::vars_os().collect();
        inherited.insert("CC_DESK_TEST_ROOT".into(), temp.path().as_os_str().to_owned());
        Self { temp, cwd, report, node, profile, request, inherited }
    }

    fn resolve(&self) -> Result<ProcessLaunchSpec, crate::cli::types::SafeError> {
        let empty = EnvMap::new();
        let owner = CallerIdentity {
            instance_id: "test-instance".into(), window_label: "main".into(),
            webview_epoch: WireU64::parse("1").unwrap(),
        };
        let snapshot = freeze_launch(&self.request, &self.profile, &owner, &FreezeContext {
            inherited: &self.inherited, terminal: &empty, legacy: None, observer: None,
        })?;
        resolve_process(&build_invocation(&self.request, &snapshot)?)
    }

    fn runner(&mut self, dialect: Dialect, shim: bool) {
        let runner = runner_path(&dialect);
        self.profile.launcher = if shim {
            Launcher::Shim { runner: runner.clone(), dialect: dialect.clone() }
        } else {
            Launcher::Shell { program: runner, dialect: dialect.clone() }
        };
        if shim {
            let (name, source) = match dialect {
                Dialect::Bash => ("probe.sh", format!("exec '{}' \"$@\"\n", self.node.replace('\\', "/").replace('\'', "'\\''"))),
                Dialect::PowerShell => ("probe.ps1", format!("& '{}' @args\nexit $LASTEXITCODE\n", self.node.replace('\'', "''"))),
                Dialect::Cmd => ("probe.cmd", format!("@echo off\r\n\"{}\" %*\r\n", self.node)),
            };
            let path = self.temp.path().join(name);
            fs::write(&path, source).unwrap();
            self.profile.program_path = Override::Set(path.to_str().unwrap().into());
        }
    }
}

fn runner_path(dialect: &Dialect) -> String {
    match dialect {
        Dialect::Bash => {
            #[cfg(windows)]
            {
                let git = crate::platform::find_executable("git.exe").expect("Git for Windows");
                let root = Path::new(&git).parent().unwrap().parent().unwrap();
                let path = root.join("bin/bash.exe");
                assert!(path.is_file(), "Git Bash is a required test dependency");
                path.to_str().unwrap().into()
            }
            #[cfg(unix)]
            { "/bin/bash".into() }
        }
        Dialect::PowerShell => crate::platform::find_executable("pwsh").expect("PowerShell 7 is required"),
        Dialect::Cmd => std::env::var("COMSPEC").expect("Windows cmd is required"),
    }
}

fn size() -> PtySize {
    PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 }
}

// Uses the production resolver, command builder and PTY spawn. The receiving
// program is D03's existing probe; no alternate argv or environment assembler.
fn execute(spec: &ProcessLaunchSpec) -> u32 {
    #[cfg(windows)]
    bundled_runtime::initialize().unwrap();
    let mut process = spawn_process(spec, size()).unwrap_or_else(|e| panic!("spawn: {e}"));
    let mut reader = process.master.try_clone_reader().unwrap();
    let writer = process.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut total = 0;
        let mut buffer = [0u8; 4096];
        let result = loop {
            match reader.read(&mut buffer) {
                Ok(0) => break true,
                Ok(n) => { total += n; if total > 4 * 1024 * 1024 { break false; } }
                Err(e) if crate::pty::is_pty_stream_end(&e) => break true,
                Err(_) => break false,
            }
        };
        let _ = tx.send(result);
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = process.child.try_wait().unwrap() { break status; }
        if started.elapsed() > Duration::from_secs(25) {
            let _ = process.child.kill();
            let _ = process.child.wait();
            panic!("probe timed out; no output contents are logged");
        }
        thread::sleep(Duration::from_millis(10));
    };
    drop(writer);
    drop(process.master);
    assert!(rx.recv_timeout(Duration::from_secs(5)).expect("PTY reader drain"));
    status.exit_code()
}

fn roundtrip(fixture: &Fixture, expected: &[&str]) -> ProbeReport {
    assert_eq!(execute(&fixture.resolve().unwrap()), 0);
    let report: ProbeReport = serde_json::from_slice(&fs::read(&fixture.report).unwrap()).unwrap();
    assert_eq!(report.argv, expected);
    assert!(report.stdin_is_tty && report.stdout_is_tty);
    assert_eq!(fs::canonicalize(&report.cwd).unwrap(), fs::canonicalize(&fixture.cwd).unwrap());
    report
}

#[test]
fn D10_Argv_NativeRoundTrip_01() {
    roundtrip(&Fixture::new(PAYLOAD), PAYLOAD);
}

#[test]
fn D10_Environment_EmptyMapClearsBase_02() {
    let mut fixture = Fixture::new(&[]);
    fixture.inherited.clear();
    let command = fixture.resolve().unwrap().command().unwrap();
    assert_eq!(command.iter_full_env_as_str().count(), 0);
    assert!(command.get_env("PATH").is_none());
    assert!(command.get_env("HOME").is_none());
}

#[test]
fn D10_SelectedProgram_DisappearsWithoutFallback_03() {
    let mut fixture = Fixture::new(&[]);
    let target = fixture.temp.path().join("selected.exe");
    fs::copy(&fixture.node, &target).unwrap();
    fixture.profile.program_path = Override::Set(target.to_str().unwrap().into());
    let spec = fixture.resolve().unwrap();
    fs::remove_file(target).unwrap();
    let error = spec.command().unwrap_err();
    assert_eq!(error.code, "PROGRAM_UNAVAILABLE");
    assert!(!error.to_string().contains("selected.exe"));
}

#[test]
fn D10_Cwd_DisappearsWithoutHomeFallback_04() {
    let fixture = Fixture::new(&[]);
    let spec = fixture.resolve().unwrap();
    fs::remove_dir(&fixture.cwd).unwrap();
    assert_eq!(spec.command().unwrap_err().code, "WORKING_DIRECTORY_UNAVAILABLE");
}

#[test]
fn D10_Debug_RedactsProcessValues_05() {
    let fixture = Fixture::new(&["private-test-token"]);
    assert_eq!(format!("{:?}", fixture.resolve().unwrap()), "ProcessLaunchSpec(<redacted>)");
}

#[test]
fn D10_Size_RejectsZeroBeforeAllocation_06() {
    let fixture = Fixture::new(&[]);
    let spec = fixture.resolve().unwrap();
    let mut bad_size = size();
    bad_size.rows = 0;
    assert_eq!(spawn_process(&spec, bad_size).err().unwrap().code, "INVALID_REQUEST");
}

#[test]
fn D10_Bash_ShellAndShimRoundTrip_07() {
    for shim in [false, true] {
        let mut fixture = Fixture::new(PAYLOAD);
        fixture.runner(Dialect::Bash, shim);
        roundtrip(&fixture, PAYLOAD);
    }
}

#[cfg(windows)]
#[test]
fn D10_PowerShell_ShellAndShimRoundTrip_08() {
    for shim in [false, true] {
        let mut fixture = Fixture::new(PAYLOAD);
        fixture.runner(Dialect::PowerShell, shim);
        roundtrip(&fixture, PAYLOAD);
    }
}

#[cfg(windows)]
#[test]
fn D10_Cmd_SafeShellAndShimRoundTrip_09() {
    let args = &["a b", "", "中文", "single'inside", "C:\\tail\\", "--future", "--", ""];
    for shim in [false, true] {
        let mut fixture = Fixture::new(args);
        fixture.runner(Dialect::Cmd, shim);
        roundtrip(&fixture, args);
    }
}

#[cfg(windows)]
#[test]
fn D10_Cmd_UnsafeInputIsNotRewritten_10() {
    for value in ["%TEMP%", "!x!", "^", "&", "|", "<", ">", "\"", "\n", "(echo x)"] {
        let mut fixture = Fixture::new(&[value]);
        fixture.runner(Dialect::Cmd, false);
        let error = fixture.resolve().unwrap_err();
        assert_eq!(error.code, "ARG_NOT_REPRESENTABLE");
        assert!(!fixture.report.exists());
    }
}

#[cfg(windows)]
#[test]
fn D10_Native_DoesNotImplicitlyExecuteBatchFiles_11() {
    let mut fixture = Fixture::new(&[]);
    fixture.runner(Dialect::Cmd, true);
    fixture.profile.launcher = Launcher::Native;
    assert_eq!(fixture.resolve().unwrap_err().code, "ARG_NOT_REPRESENTABLE");
}

#[cfg(windows)]
#[test]
fn D10_PowerShell_OldParserFailsBeforeAgent_12() {
    let mut fixture = Fixture::new(PAYLOAD);
    let runner = crate::platform::find_executable("powershell.exe").expect("Windows PowerShell 5.1");
    fixture.profile.launcher = Launcher::Shell { program: runner, dialect: Dialect::PowerShell };
    assert_eq!(execute(&fixture.resolve().unwrap()), 125);
    assert!(!fixture.report.exists());
}

#[test]
fn D10_Environment_RemovalReachesRealChild_13() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "tests::native_cli_platform::D10_Environment_Worker_99"])
        .env("CC_DESK_FIXTURE_VALUE", "parent-only-fixture")
        .env("CC_DESK_D10_WORKER", "1")
        .status().unwrap();
    assert!(status.success());
}

#[test]
#[ignore = "subprocess worker explicitly invoked by D10_Environment_RemovalReachesRealChild_13"]
fn D10_Environment_Worker_99() {
    assert_eq!(std::env::var("CC_DESK_D10_WORKER").unwrap(), "1");
    let mut fixture = Fixture::new(&[]);
    assert_eq!(fixture.inherited.get(OsStr::new("CC_DESK_FIXTURE_VALUE")).unwrap(), "parent-only-fixture");
    fixture.profile.env.insert("CC_DESK_FIXTURE_VALUE".into(), Override::Unset);
    let report = roundtrip(&fixture, &[]);
    assert!(report.env.cc_desk_fixture_value.is_none());
}

#[cfg(unix)]
#[test]
fn D10_Environment_NonUnicodePreserved_14() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let mut fixture = Fixture::new(&[]);
    let bytes = OsString::from_vec(vec![0xff, 0xfe]);
    fixture.inherited.insert("BYTES".into(), bytes.clone());
    let command = fixture.resolve().unwrap().command().unwrap();
    assert_eq!(command.get_env("BYTES"), Some(bytes.as_os_str()));
}
