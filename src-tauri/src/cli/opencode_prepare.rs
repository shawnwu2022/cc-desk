//! 通过 opencode 官方本机 API 创建可续接会话；临时 server 不接管 TUI。
use super::profiles::error;
use super::types::SafeError;
use crate::platform::launch::ProcessLaunchSpec;
use crate::version_history::maintenance::{OwnedChildTicket, PreparingStart};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::io::{Read, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

const PREPARE_TIMEOUT: Duration = Duration::from_secs(5);
const HEALTH_TIMEOUT: Duration = Duration::from_millis(700);
const CREATE_TIMEOUT: Duration = Duration::from_secs(5);
const DB_PATH_TIMEOUT: Duration = Duration::from_secs(5);

/// The official `db path` command has no session creation action. Its bounded
/// stdout is used only to narrow the already selected XDG data directory.
pub(crate) fn probe_database_path(
    spec: &ProcessLaunchSpec,
    mut admission: PreparingStart,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    let mut command = spec.metadata_command().map_err(|_| error("SCOPE_UNKNOWN"))?;
    let mut output = tempfile::tempfile().map_err(|_| error("SCOPE_UNKNOWN"))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(output.try_clone().map_err(|_| error("SCOPE_UNKNOWN"))?))
        .stderr(Stdio::null());
    #[cfg(windows)]
    let job = server_job::Job::new().map_err(|_| error("SCOPE_UNKNOWN"))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000 | 0x0000_0004);
    }
    current()?;
    let ticket = admission.begin_creation();
    let spawned = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            ticket.no_child_created();
            return Err(error("SCOPE_UNKNOWN"));
        }
    };
    let child = ServerChild {
        child: spawned,
        ticket: Some(ticket.child_created()),
        #[cfg(windows)]
        job: Some(job),
    };
    let mut child = child;
    #[cfg(windows)]
    child.job.as_ref().unwrap().assign_and_resume(&child.child).map_err(|_| error("SCOPE_UNKNOWN"))?;
    let deadline = Instant::now() + DB_PATH_TIMEOUT;
    loop {
        current()?;
        if let Some(status) = child.child.try_wait().map_err(|_| error("SCOPE_UNKNOWN"))? {
            if !status.success() {
                return Err(error("SCOPE_UNKNOWN"));
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err(error("SCOPE_UNKNOWN"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if output.metadata().map_err(|_| error("SCOPE_UNKNOWN"))?.len() > 4096 {
        return Err(error("SCOPE_UNKNOWN"));
    }
    output.seek(SeekFrom::Start(0)).map_err(|_| error("SCOPE_UNKNOWN"))?;
    let mut bytes = Vec::new();
    output.take(4097).read_to_end(&mut bytes).map_err(|_| error("SCOPE_UNKNOWN"))?;
    if bytes.len() > 4096 {
        return Err(error("SCOPE_UNKNOWN"));
    }
    current()?;
    String::from_utf8(bytes).map_err(|_| error("SCOPE_UNKNOWN"))
}

fn http_client() -> Result<reqwest::blocking::Client, SafeError> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(HEALTH_TIMEOUT)
        .build()
        .map_err(|_| error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"))
}

#[derive(Deserialize)]
struct Health {
    healthy: bool,
}

#[derive(Deserialize)]
struct Session {
    id: String,
    directory: String,
}

struct ServerChild {
    child: Child,
    ticket: Option<OwnedChildTicket>,
    #[cfg(windows)]
    job: Option<server_job::Job>,
}

impl Drop for ServerChild {
    fn drop(&mut self) {
        #[cfg(windows)]
        drop(self.job.take());
        let _ = self.child.kill();
        if self.child.wait().is_ok() {
            if let Some(ticket) = self.ticket.take() {
                ticket.reaped();
            }
        }
    }
}

#[cfg(windows)]
mod server_job {
    use super::{error, SafeError};
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    };
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    pub(super) struct Job(HANDLE);
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    impl Job {
        pub(super) fn new() -> Result<Self, SafeError> {
            let job = Self(
                unsafe { CreateJobObjectW(None, PCWSTR::null()) }
                    .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?,
            );
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            }
            .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?;
            Ok(job)
        }
        pub(super) fn assign_and_resume(&self, child: &Child) -> Result<(), SafeError> {
            let process = HANDLE(child.as_raw_handle());
            unsafe { AssignProcessToJobObject(self.0, process) }
                .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?;
            let snapshot = Self(
                unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }
                    .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?,
            );
            let mut entry = THREADENTRY32 {
                dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            unsafe { Thread32First(snapshot.0, &mut entry) }
                .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?;
            loop {
                if entry.th32OwnerProcessID == child.id() {
                    let thread = Self(
                        unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                            .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?,
                    );
                    if unsafe { ResumeThread(thread.0) } == u32::MAX {
                        return Err(error("OPENCODE_SESSION_PREPARE_FAILED"));
                    }
                    return Ok(());
                }
                if unsafe { Thread32Next(snapshot.0, &mut entry) }.is_err() {
                    break;
                }
            }
            Err(error("OPENCODE_SESSION_PREPARE_FAILED"))
        }
    }
}

fn same_directory(actual: &str, expected: &Path) -> bool {
    let Some(expected) = expected.to_str() else {
        return false;
    };
    #[cfg(windows)]
    {
        actual
            .replace('/', "\\")
            .eq_ignore_ascii_case(&expected.replace('/', "\\"))
    }
    #[cfg(not(windows))]
    {
        actual == expected
    }
}

fn validated_session_id(session: Session, cwd: &Path) -> Result<String, SafeError> {
    if !session.id.starts_with("ses_")
        || session.id.len() > 256
        || !session
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !same_directory(&session.directory, cwd)
    {
        return Err(error("OPENCODE_SESSION_OUTCOME_UNKNOWN"));
    }
    Ok(session.id)
}

fn bounded_json<T: DeserializeOwned>(
    response: reqwest::blocking::Response,
    limit: u64,
) -> Option<T> {
    let mut bytes = Vec::new();
    response.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// 为本机临时服务选择回环端口。
pub(crate) fn select_port() -> Result<u16, SafeError> {
    let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?;
    let port = listener
        .local_addr()
        .map_err(|_| error("OPENCODE_SESSION_PREPARE_FAILED"))?
        .port();
    Ok(port)
}

/// 从已冻结的 CLI profile 启动短命服务，仅接受同目录的官方会话 ID。
pub(crate) fn create_session(
    spec: &ProcessLaunchSpec,
    cwd: &Path,
    port: u16,
    mut admission: PreparingStart,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    let password = uuid::Uuid::new_v4().simple().to_string();
    let mut command = spec.metadata_command()?;
    command
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .env("OPENCODE_SERVER_USERNAME", "opencode")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    let job = server_job::Job::new()
        .map_err(|_| error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // 子进程在进入私有 kill-on-close Job 前不能执行包装脚本。
        command.creation_flags(0x0800_0000 | 0x0000_0004);
    }
    current()?;
    let ticket = admission.begin_creation();
    let spawned = match command.spawn() {
        Ok(child) => child,
        Err(_) => {
            ticket.no_child_created();
            return Err(error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"));
        }
    };
    let mut child = ServerChild {
        child: spawned,
        ticket: Some(ticket.child_created()),
        #[cfg(windows)]
        job: Some(job),
    };
    #[cfg(windows)]
    child
        .job
        .as_ref()
        .unwrap()
        .assign_and_resume(&child.child)
        .map_err(|_| error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"))?;
    let client = http_client()?;
    let base = format!("http://127.0.0.1:{port}");
    let deadline = Instant::now() + PREPARE_TIMEOUT;
    loop {
        current()?;
        if child
            .child
            .try_wait()
            .map_err(|_| error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"))?
            .is_some()
        {
            return Err(error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"));
        }
        if client
            .get(format!("{base}/global/health"))
            .timeout(HEALTH_TIMEOUT.min(deadline.saturating_duration_since(Instant::now())))
            .basic_auth("opencode", Some(&password))
            .send()
            .ok()
            .and_then(|response| response.error_for_status().ok())
            .and_then(|response| bounded_json::<Health>(response, 4096))
            .is_some_and(|health| health.healthy)
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err(error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // POST 只发送一次：响应丢失时不能盲重放并制造第二个会话。
    let id = post_session(&client, &base, &password, cwd, current)?;
    drop(child);
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    for _ in 0..10 {
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() {
            current()?;
            return Ok(id);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))
}

fn post_session(
    client: &reqwest::blocking::Client,
    base: &str,
    password: &str,
    cwd: &Path,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    current()?;
    let response = client
        .post(format!("{base}/session"))
        .timeout(CREATE_TIMEOUT)
        .basic_auth("opencode", Some(password))
        .json(&serde_json::json!({}))
        .send()
        .map_err(|_| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?;
    let response = response
        .error_for_status()
        .map_err(|_| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?;
    let session: Session =
        bounded_json(response, 32768).ok_or_else(|| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?;
    validated_session_id(session, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn mock_creation_server(
        delay: Duration,
        redirect: bool,
    ) -> (String, std::thread::JoinHandle<usize>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let mut count = 0;
            loop {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(error) => panic!("fixture accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                assert!(request.starts_with(b"POST /session HTTP/1.1\r\n"));
                let headers = String::from_utf8(request).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                assert!(length <= 8192);
                stream.read_exact(&mut vec![0; length]).unwrap();
                count += 1;
                std::thread::sleep(delay);
                if redirect && count == 1 {
                    write!(stream, "HTTP/1.1 307 Temporary Redirect\r\nLocation: /session\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else {
                    let body = serde_json::json!({"id":"ses_fixture_1","directory":"/fixture","future":{"ignored":true}}).to_string();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                }
                drop(stream);
                if !redirect || count > 1 {
                    break;
                }
                listener.set_nonblocking(true).unwrap();
                // 给意外重定向第二次 POST 留出观察窗口。
                std::thread::sleep(Duration::from_millis(500));
            }
            count
        });
        (format!("http://{address}"), worker)
    }

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_PreparationCreationOutlivesHealthTimeout_093() {
        // 等价 HTTP 层对照：旧逻辑把健康探测的 700ms 同样用于 POST。
        let (legacy_base, legacy_worker) = mock_creation_server(Duration::from_millis(1100), false);
        let legacy = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(HEALTH_TIMEOUT)
            .build()
            .unwrap();
        assert!(legacy
            .post(format!("{legacy_base}/session"))
            .basic_auth("opencode", Some("fixture"))
            .json(&serde_json::json!({}))
            .send()
            .is_err());
        assert_eq!(legacy_worker.join().unwrap(), 1);
        eprintln!("HTTP-layer legacy control: 700ms timed out after exactly one accepted POST");
        let (base, worker) = mock_creation_server(Duration::from_millis(1100), false);
        let result = post_session(
            &http_client().unwrap(),
            &base,
            "fixture",
            Path::new("/fixture"),
            &|| Ok(()),
        );
        assert_eq!(worker.join().unwrap(), 1);
        assert_eq!(result.unwrap(), "ses_fixture_1");
    }

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_PreparationRedirectNeverReposts_094() {
        // 等价旧 reqwest 默认策略会遵循 307 并实际发出第二次 POST。
        let (legacy_base, legacy_worker) = mock_creation_server(Duration::ZERO, true);
        let legacy = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(HEALTH_TIMEOUT)
            .build()
            .unwrap();
        assert!(legacy
            .post(format!("{legacy_base}/session"))
            .basic_auth("opencode", Some("fixture"))
            .json(&serde_json::json!({}))
            .send()
            .unwrap()
            .status()
            .is_success());
        assert_eq!(legacy_worker.join().unwrap(), 2);
        eprintln!("HTTP-layer legacy control: default 307 policy sent two POST requests");
        let (base, worker) = mock_creation_server(Duration::ZERO, true);
        let result = post_session(
            &http_client().unwrap(),
            &base,
            "fixture",
            Path::new("/fixture"),
            &|| Ok(()),
        );
        assert_eq!(worker.join().unwrap(), 1);
        assert_eq!(result.unwrap_err().code, "OPENCODE_SESSION_OUTCOME_UNKNOWN");
    }

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_PreparationRevocationStopsBeforePost_095() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let result = post_session(
            &http_client().unwrap(),
            &format!("http://{}", listener.local_addr().unwrap()),
            "fixture",
            Path::new("/fixture"),
            &|| Err(error("FORBIDDEN")),
        );
        assert_eq!(result.unwrap_err().code, "FORBIDDEN");
        assert!(listener.accept().is_err());
    }

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_PreparedIdRequiresExactDirectory_01() {
        let cwd = Path::new(if cfg!(windows) { r"C:\repo" } else { "/repo" });
        assert_eq!(
            validated_session_id(
                Session {
                    id: "ses_valid_1".into(),
                    directory: cwd.to_string_lossy().into()
                },
                cwd
            )
            .unwrap(),
            "ses_valid_1"
        );
        assert_eq!(
            validated_session_id(
                Session {
                    id: "ses_valid_1".into(),
                    directory: "/other".into()
                },
                cwd
            )
            .unwrap_err()
            .code,
            "OPENCODE_SESSION_OUTCOME_UNKNOWN"
        );
        assert_eq!(
            validated_session_id(
                Session {
                    id: "-bad".into(),
                    directory: cwd.to_string_lossy().into()
                },
                cwd
            )
            .unwrap_err()
            .code,
            "OPENCODE_SESSION_OUTCOME_UNKNOWN"
        );
    }

    #[cfg(windows)]
    #[test]
    #[allow(non_snake_case)]
    fn Opencode_OfficialApiCreatesSessionThroughSelectedShim_03() {
        use crate::cli::environment::EnvMap;
        use crate::cli::invocation::build_invocation;
        use crate::cli::profiles::{Dialect, Launcher, Override, Profile};
        use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext};
        use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, WireU64};
        use crate::platform::launch::resolve_process;
        use crate::version_history::maintenance::{AdmissionGate, RuntimeKind};
        let Some(shim) = std::env::var_os("CC_DESK_OPENCODE_TEST_SHIM") else {
            eprintln!("[SKIP] Opencode_OfficialApiCreatesSessionThroughSelectedShim_03: no installed test shim");
            return;
        };
        let root = tempfile::tempdir().unwrap();
        let shim = Path::new(&shim).to_str().unwrap().to_owned();
        let runner = std::env::var("ComSpec").unwrap();
        let mut profile = Profile::new("opencode-test", CliKind::Opencode);
        profile.program_path = Override::Set(shim);
        profile.launcher = Launcher::Shim {
            runner,
            dialect: Dialect::Cmd,
        };
        let port = select_port().unwrap();
        let request = LaunchRequest {
            request_id: "request-opencode-test".into(),
            tab_id: "tab-opencode-test".into(),
            run_id: "run-opencode-test".into(),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision,
            cli: CliKind::Opencode,
            launch_cwd: root.path().to_string_lossy().into(),
            action: LaunchAction::Raw {
                argv: vec![
                    "serve".into(),
                    "--hostname".into(),
                    "127.0.0.1".into(),
                    "--port".into(),
                    port.to_string(),
                ],
            },
            extra_args: Vec::new(),
            cols: 80,
            rows: 24,
        };
        // 只继承启动 wrapper 必需的 OS 变量，排除所有用户覆盖、模型凭证和 NODE_OPTIONS。
        let mut inherited = EnvMap::new();
        for name in [
            "PATH",
            "PATHEXT",
            "SystemRoot",
            "ComSpec",
            "TEMP",
            "TMP",
            "WINDIR",
        ] {
            if let Some(value) = std::env::var_os(name) {
                inherited.insert(name.into(), value);
            }
        }
        for name in ["HOME", "USERPROFILE", "OPENCODE_TEST_HOME"] {
            inherited.insert(name.into(), root.path().into());
        }
        for name in [
            "OPENCODE_DISABLE_AUTOUPDATE",
            "OPENCODE_DISABLE_MODELS_FETCH",
            "OPENCODE_DISABLE_DEFAULT_PLUGINS",
            "OPENCODE_DISABLE_EXTERNAL_SKILLS",
            "OPENCODE_DISABLE_LSP_DOWNLOAD",
            "OPENCODE_DISABLE_CLAUDE_CODE",
            "OPENCODE_DISABLE_PROJECT_CONFIG",
            "OPENCODE_DISABLE_CHANNEL_DB",
        ] {
            inherited.insert(name.into(), "1".into());
        }
        for (name, directory) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
        ] {
            inherited.insert(name.into(), root.path().join(directory).into_os_string());
        }
        let snapshot = freeze_launch(
            &request,
            &profile,
            &CallerIdentity {
                instance_id: "opencode-test-instance".into(),
                window_label: "main".into(),
                webview_epoch: WireU64::parse("1").unwrap(),
            },
            &FreezeContext {
                inherited: &inherited,
                terminal: &EnvMap::new(),
                legacy: None,
                observer: None,
            },
        )
        .unwrap();
        let invocation = build_invocation(&request, &snapshot).unwrap();
        let spec = resolve_process(&invocation).unwrap();
        let admission = AdmissionGate::new()
            .begin_start(RuntimeKind::Native)
            .unwrap()
            .preparing();
        let id = create_session(&spec, root.path(), port, admission, &|| Ok(())).unwrap();
        assert!(id.starts_with("ses_"));
        let mut path_request = request.clone();
        path_request.action = LaunchAction::Raw { argv: vec!["db".into(), "path".into()] };
        let path_snapshot = freeze_launch(
            &path_request, &profile,
            &CallerIdentity {
                instance_id: "opencode-test-instance".into(),
                window_label: "main".into(),
                webview_epoch: WireU64::parse("1").unwrap(),
            },
            &FreezeContext { inherited: &inherited, terminal: &EnvMap::new(), legacy: None, observer: None },
        ).unwrap();
        let path_spec = resolve_process(&build_invocation(&path_request, &path_snapshot).unwrap()).unwrap();
        let path_admission = AdmissionGate::new().begin_start(RuntimeKind::Native).unwrap().preparing();
        let path = probe_database_path(&path_spec, path_admission, &|| Ok(())).unwrap();
        assert!(same_directory(path.trim(), &root.path().join("data/opencode/opencode.db")));
        assert!(root.path().join("data/opencode/opencode.db").is_file());
        let data_root = crate::cli::native_projection::scoped_fs::Root::open(
            &root.path().join("data/opencode"),
        )
        .unwrap();
        let project_root =
            crate::cli::native_projection::scoped_fs::Root::open(root.path()).unwrap();
        let paths = [root.path().to_path_buf()];
        let project = crate::cli::native_projection::catalog::ProjectRef {
            id: "opencode-test",
            root: &project_root,
            paths: &paths,
        };
        let rows = crate::cli::native_projection::opencode_db::read_history(&data_root, &[project])
            .unwrap();
        assert!(rows.iter().any(|row| matches!(
            row,
            crate::cli::native_projection::wire::ResourceItem::Session { native_session_id, .. }
                if native_session_id == &id
        )));
    }
}
