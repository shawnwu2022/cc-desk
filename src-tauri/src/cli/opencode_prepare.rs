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
    admission: PreparingStart,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    probe_output(spec, admission, current)
}

fn probe_output(
    spec: &ProcessLaunchSpec,
    mut admission: PreparingStart,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    let mut command = spec
        .metadata_command()
        .map_err(|_| error("SCOPE_UNKNOWN"))?;
    let mut output = tempfile::tempfile().map_err(|_| error("SCOPE_UNKNOWN"))?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            output.try_clone().map_err(|_| error("SCOPE_UNKNOWN"))?,
        ))
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
    child
        .job
        .as_ref()
        .unwrap()
        .assign_and_resume(&child.child)
        .map_err(|_| error("SCOPE_UNKNOWN"))?;
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
    output
        .seek(SeekFrom::Start(0))
        .map_err(|_| error("SCOPE_UNKNOWN"))?;
    let mut bytes = Vec::new();
    output
        .take(4097)
        .read_to_end(&mut bytes)
        .map_err(|_| error("SCOPE_UNKNOWN"))?;
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

/// v2 的会话对象包在 `data` 里、目录在 `location.directory`；v1 是裸对象、
/// 顶层 `directory`。按响应形态解析，不做版本探测。
#[derive(Deserialize)]
struct Session {
    id: String,
    #[serde(flatten)]
    origin: SessionOrigin,
}

/// v1/v2 两代目录字段的并集形状；`directory()` 是唯一的目录读取口。
#[derive(Deserialize, Default)]
struct SessionOrigin {
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    location: Option<LocationRef>,
}

#[derive(Deserialize)]
struct LocationRef {
    #[serde(default)]
    directory: Option<String>,
}

impl SessionOrigin {
    fn directory(&self) -> Option<&str> {
        self.location
            .as_ref()
            .and_then(|location| location.directory.as_deref())
            .or(self.directory.as_deref())
    }
}

/// 本地 server 的 API 代次：由 with_server 的健康握手形态决议（v1 的
/// /global/health JSON 或 v2 的 /health 网页 200），同一 server 生命周期内
/// 不会变化，后续调用按它直选端点，不再逐请求嗅探路由。
#[derive(Clone, Copy)]
enum ApiFlavor {
    V1,
    V2,
}

impl ApiFlavor {
    fn session_url(self, base: &str, id: &str) -> String {
        match self {
            ApiFlavor::V1 => format!("{base}/session/{id}"),
            ApiFlavor::V2 => format!("{base}/api/session/{id}"),
        }
    }
}

/// 有界读取会话响应，剥掉 v2 的 `data` 包装（v1 裸对象原样）。
/// 有界读取契约由 bounded_json 单点承担。
fn bounded_session<T: DeserializeOwned>(response: reqwest::blocking::Response) -> Option<T> {
    let value: serde_json::Value = bounded_json(response, 32768)?;
    let inner = value.get("data").cloned().unwrap_or(value);
    serde_json::from_value(inner).ok()
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
    // 目录证明是同目录契约的一部分：v1 顶层 directory、v2 location.directory，
    // 两者都缺失时不能承认这个会话 ID。
    let matches = session
        .origin
        .directory()
        .is_some_and(|directory| same_directory(directory, cwd));
    if !session.id.starts_with("ses_")
        || session.id.len() > 256
        || !session
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || !matches
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
    admission: PreparingStart,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    with_server(
        spec,
        port,
        admission,
        current,
        |client, base, password, flavor| post_session(client, base, password, cwd, flavor, current),
    )
}

fn with_server(
    spec: &ProcessLaunchSpec,
    port: u16,
    mut admission: PreparingStart,
    current: &impl Fn() -> Result<(), SafeError>,
    operation: impl FnOnce(
        &reqwest::blocking::Client,
        &str,
        &str,
        ApiFlavor,
    ) -> Result<String, SafeError>,
) -> Result<String, SafeError> {
    let password = uuid::Uuid::new_v4().simple().to_string();
    let mut command = spec.metadata_command()?;
    command
        // v1 读 OPENCODE_SERVER_PASSWORD/USERNAME；v2 读 OPENCODE_PASSWORD 且
        // 用户名固定 opencode（SERVER_USERNAME 被忽略）。双写保持 v1/v2 同密。
        .env("OPENCODE_PASSWORD", &password)
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
    let mut flavor = None;
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
        // v1 健康是 /global/health 的 JSON {healthy:true}；v2 的 /health 返回
        // 200 网页（无认证体）。先问 v1，命中即短路；否则再问 v2——就绪轮
        // 最多两个请求，同时握手的形态就是本次 server 的 API 代次。
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
            flavor = Some(ApiFlavor::V1);
        } else if client
            .get(format!("{base}/health"))
            .timeout(HEALTH_TIMEOUT.min(deadline.saturating_duration_since(Instant::now())))
            .send()
            .ok()
            .and_then(|response| response.error_for_status().ok())
            .is_some()
        {
            flavor = Some(ApiFlavor::V2);
        }
        if flavor.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(error("OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE"));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // flavor 在健康握手中必然已决议（否则上面已超时返回）。
    let id = operation(&client, &base, &password, flavor.unwrap_or(ApiFlavor::V1))?;
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

pub(crate) fn rename_session(
    spec: &ProcessLaunchSpec,
    port: u16,
    admission: PreparingStart,
    authority: &super::native_projection::registry::RenameAuthority,
    title: &str,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<(), SafeError> {
    with_server(
        spec,
        port,
        admission,
        current,
        |client, base, password, flavor| {
            rename_http(
                client,
                base,
                password,
                &authority.native_id,
                Path::new(&authority.cwd),
                title,
                flavor,
                current,
            )
        },
    )
    .map(|_| ())
    .map_err(|failure| match failure.code.as_str() {
        "OPENCODE_SESSION_PREPARE_UNAVAILABLE_BEFORE_CREATE" => error("NATIVE_RENAME_UNSUPPORTED"),
        "OPENCODE_SESSION_OUTCOME_UNKNOWN" => error("RENAME_OUTCOME_UNKNOWN"),
        _ => failure,
    })
}

#[allow(clippy::too_many_arguments)]
fn rename_http(
    client: &reqwest::blocking::Client,
    base: &str,
    password: &str,
    id: &str,
    cwd: &Path,
    title: &str,
    flavor: ApiFlavor,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    #[derive(Deserialize)]
    struct Metadata {
        id: String,
        #[serde(flatten)]
        origin: SessionOrigin,
        title: String,
    }
    let check = |value: &Metadata| {
        let matches = value
            .origin
            .directory()
            .is_some_and(|directory| same_directory(directory, cwd));
        if value.id != id || !matches {
            return Err(error("SESSION_ORIGIN_AMBIGUOUS"));
        }
        current()
    };
    // 端点由 with_server 的健康握手一次性决议，整链一致；不存在「会话缺失
    // 被误判为代次切换」的歧义（那是路由嗅探的问题）。
    current()?;
    let url = flavor.session_url(base, id);
    let before: Metadata = bounded_session(
        client
            .get(&url)
            .basic_auth("opencode", Some(password))
            .timeout(CREATE_TIMEOUT)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| error("NATIVE_RENAME_UNSUPPORTED"))?,
    )
    .ok_or_else(|| error("NATIVE_RENAME_UNSUPPORTED"))?;
    check(&before)?;
    let read_back = |value: reqwest::blocking::Response| -> Result<Metadata, SafeError> {
        let metadata: Metadata =
            bounded_session(value).ok_or_else(|| error("RENAME_OUTCOME_UNKNOWN"))?;
        check(&metadata)?;
        Ok(metadata)
    };
    // PATCH 仅发一次。未知写入结果不自动重放，也不退回 SQLite 或显示别名。
    let changed = client
        .patch(&url)
        .basic_auth("opencode", Some(password))
        .timeout(CREATE_TIMEOUT)
        .json(&serde_json::json!({"title":title}))
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    let changed = read_back(changed)?;
    if changed.title != title {
        return Err(error("RENAME_OUTCOME_UNKNOWN"));
    }
    let after = client
        .get(&url)
        .basic_auth("opencode", Some(password))
        .timeout(CREATE_TIMEOUT)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|_| error("RENAME_OUTCOME_UNKNOWN"))?;
    let after = read_back(after)?;
    if after.title != title {
        return Err(error("RENAME_OUTCOME_UNKNOWN"));
    }
    Ok(id.to_owned())
}

fn post_session(
    client: &reqwest::blocking::Client,
    base: &str,
    password: &str,
    cwd: &Path,
    flavor: ApiFlavor,
    current: &impl Fn() -> Result<(), SafeError>,
) -> Result<String, SafeError> {
    current()?;
    // 端点由健康握手决议。v2：POST /api/session 显式携带 location 目录，响应
    // 包 data；v1：POST /session 从 serve cwd 推断目录。创建请求只发一次——
    // 任何错误（重定向/服务错误/连接失败）都按 OUTCOME_UNKNOWN 结束，绝不重放。
    let (url, body) = match flavor {
        ApiFlavor::V2 => (
            format!("{base}/api/session"),
            serde_json::json!({"location": {"directory": cwd.to_string_lossy()}}),
        ),
        ApiFlavor::V1 => (format!("{base}/session"), serde_json::json!({})),
    };
    let response = client
        .post(url)
        .timeout(CREATE_TIMEOUT)
        .basic_auth("opencode", Some(password))
        .json(&body)
        .send()
        .map_err(|_| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?;
    let response = response
        .error_for_status()
        .map_err(|_| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?;
    let session =
        bounded_session(response).ok_or_else(|| error("OPENCODE_SESSION_OUTCOME_UNKNOWN"))?;
    validated_session_id(session, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(non_snake_case)]
    fn Opencode_RenameHttpChecksIdentityAndNeverReplaysPatch_096() {
        use std::io::Write;
        for mode in [
            "success",
            "wrong-id",
            "wrong-directory",
            "wrong-patch-title",
            "wrong-readback",
            "redirect",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let count = if mode == "wrong-id" || mode == "wrong-directory" {
                1
            } else if mode == "wrong-patch-title" || mode == "redirect" {
                2
            } else {
                3
            };
            let worker = std::thread::spawn(move || {
                let mut methods = Vec::new();
                let deadline = Instant::now() + Duration::from_secs(3);
                while methods.len() < count && Instant::now() < deadline {
                    let Ok((mut stream, _)) = listener.accept() else {
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    };
                    // Winsock 继承 listener 的非阻塞模式，读取超时不会恢复阻塞。
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 4096];
                    loop {
                        let n = stream.read(&mut buffer).unwrap();
                        if n == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buffer[..n]);
                        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                            let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                            let length = header
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("content-length: ")
                                        .and_then(|x| x.parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            if bytes.len() >= end + 4 + length {
                                break;
                            }
                        }
                    }
                    let request = String::from_utf8(bytes).unwrap();
                    methods.push(request.split_whitespace().next().unwrap().to_owned());
                    assert!(request.contains("/session/ses_fixture HTTP/1.1"));
                    assert!(request.to_lowercase().contains("authorization: basic "));
                    if methods.len() == 2 {
                        assert!(request.ends_with("{\"title\":\"New title\"}"));
                    }
                    let mut body = serde_json::json!({"id":"ses_fixture","directory":"/fixture","title":if methods.len()==1 {"Old"} else {"New title"},"future":true});
                    if mode == "wrong-id" {
                        body["id"] = "ses_foreign".into();
                    }
                    if mode == "wrong-directory" {
                        body["directory"] = "/foreign".into();
                    }
                    if (mode == "wrong-patch-title" && methods.len() == 2)
                        || (mode == "wrong-readback" && methods.len() == 3)
                    {
                        body["title"] = "Old".into();
                    }
                    let status = if mode == "redirect" && methods.len() == 2 {
                        "307 Temporary Redirect\r\nLocation: /session/ses_fixture"
                    } else {
                        "200 OK"
                    };
                    let body = body.to_string();
                    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
                }
                methods
            });
            let result = rename_http(
                &http_client().unwrap(),
                &format!("http://{address}"),
                "fixture-secret",
                "ses_fixture",
                Path::new("/fixture"),
                "New title",
                ApiFlavor::V1,
                &|| Ok(()),
            );
            let methods = worker.join().unwrap();
            assert_eq!(methods.len(), count, "{mode}");
            assert_eq!(
                methods.iter().filter(|m| m.as_str() == "PATCH").count(),
                usize::from(count > 1)
            );
            match mode {
                "success" => assert_eq!(result.unwrap(), "ses_fixture"),
                "wrong-id" | "wrong-directory" => {
                    assert_eq!(result.unwrap_err().code, "SESSION_ORIGIN_AMBIGUOUS")
                }
                _ => assert_eq!(result.unwrap_err().code, "RENAME_OUTCOME_UNKNOWN"),
            }
        }
    }
    use std::io::Write;

    fn mock_creation_server(
        delay: Duration,
        redirect: bool,
    ) -> (String, std::thread::JoinHandle<usize>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let mut count = 0;
            let mut redirect_deadline = None;
            loop {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if redirect_deadline.is_some_and(|deadline| Instant::now() < deadline) {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        break;
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                };
                // redirect 的后续 accept 也必须保留有界的阻塞读取。
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let headers = String::from_utf8(request).unwrap();
                let request_line = headers.lines().next().unwrap_or("").to_owned();
                assert!(
                    request_line == "POST /api/session HTTP/1.1"
                        || request_line == "POST /session HTTP/1.1",
                    "unexpected request line: {request_line}"
                );
                let is_v2 = request_line.starts_with("POST /api/session");
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
                    // v2 形态包 data、目录在 location；v1 是裸对象、顶层 directory。
                    let body = if is_v2 {
                        serde_json::json!({"data":{"id":"ses_fixture_1","location":{"directory":"/fixture"},"future":{"ignored":true}}}).to_string()
                    } else {
                        serde_json::json!({"id":"ses_fixture_1","directory":"/fixture","future":{"ignored":true}}).to_string()
                    };
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
                // 同时覆盖迟到的正向对照和禁止重放的负向观察，不只探测一次。
                redirect_deadline = Some(Instant::now() + Duration::from_secs(2));
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
            ApiFlavor::V2,
            &|| Ok(()),
        );
        assert_eq!(worker.join().unwrap(), 1);
        assert_eq!(result.unwrap(), "ses_fixture_1");
    }

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_CreationFixtureAcceptsDelayedRedirect_098() {
        let (base, worker) = mock_creation_server(Duration::ZERO, true);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                // 故意晚于旧 fixture 的单次 500ms accept，仍在请求期限内。
                std::thread::sleep(Duration::from_millis(650));
                attempt.follow()
            }))
            .build()
            .unwrap();
        let response = client
            .post(format!("{base}/session"))
            .json(&serde_json::json!({}))
            .send();
        let count = worker.join().unwrap();
        assert_eq!(count, 2, "fixture closed before the permitted second POST");
        assert!(response.unwrap().status().is_success());
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
            ApiFlavor::V2,
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
            ApiFlavor::V2,
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
                    origin: SessionOrigin {
                        directory: Some(cwd.to_string_lossy().into()),
                        location: None
                    },
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
                    origin: SessionOrigin {
                        directory: Some("/other".into()),
                        location: None
                    },
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
                    origin: SessionOrigin {
                        directory: Some(cwd.to_string_lossy().into()),
                        location: None
                    },
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
        path_request.action = LaunchAction::Raw {
            argv: vec!["db".into(), "path".into()],
        };
        let path_snapshot = freeze_launch(
            &path_request,
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
        let path_spec =
            resolve_process(&build_invocation(&path_request, &path_snapshot).unwrap()).unwrap();
        let path_admission = AdmissionGate::new()
            .begin_start(RuntimeKind::Native)
            .unwrap()
            .preparing();
        let path = probe_database_path(&path_spec, path_admission, &|| Ok(())).unwrap();
        assert!(same_directory(
            path.trim(),
            &root.path().join("data/opencode/opencode.db")
        ));
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
