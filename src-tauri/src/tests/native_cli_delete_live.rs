//! 仅显式执行的已安装 Codex 隔离验收；不调用应用生产启动入口。
use crate::cli::{
    launch_service::LaunchService,
    native_delete::{ConfirmRequest, InspectRequest, PreviewRequest, Receipt},
    native_projection::{
        scoped_fs::Root,
        service::ProjectionService,
        wire::{ScopeTarget, SourceRef},
    },
    profiles::{EnvValue, Override, Profile},
    snapshot::CallerIdentity,
    storage::{Patch, WorkspaceRepository},
    types::CliKind,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, HANDLE},
        Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_SHARE_DELETE, FILE_SHARE_READ,
        },
        System::{
            Com::CoTaskMemFree,
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
                JobObjectExtendedLimitInformation, QueryInformationJobObject,
                SetInformationJobObject, TerminateJobObject,
                JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Registry::{
                RegCloseKey, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY,
                REG_OPTION_OPEN_LINK,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
        UI::Shell::{FOLDERID_ProgramData, SHGetKnownFolderPath, KF_FLAG_DEFAULT},
    },
};

const CODEX_EXE: &str = r"D:\nodejs\node_global\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe";
const CODEX_SHA256: &str = "edfdd48f8b458af0906e507b4cb05217b326c3d6e515e9f024e05e3b8d650f70";
const CODEX_BYTES: u64 = 369_062_704;
const CODEX_VERSION: &str = "0.167.0";
const OUTPUT_LIMIT: u64 = 1024 * 1024;

#[path = "native_cli_delete_webview.rs"]
mod webview;

struct NativeWriterLock {
    file: File,
    directory: Root,
    path: PathBuf,
}
impl NativeWriterLock {
    fn acquire(fixture: &Fixture, id: &str) -> io::Result<Self> {
        // 固定 086 的 rollout/src/writer_lock.rs 使用此命名空间及 std::File 排他锁。
        use std::os::windows::fs::MetadataExt;
        let id = uuid::Uuid::parse_str(id)
            .map_err(|_| io::Error::other("FIXTURE_LOCK_ID_INVALID"))?
            .to_string();
        let path = fixture.root.join("codex-home/thread-writer-locks");
        fs::create_dir_all(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("FIXTURE_LOCK_DIRECTORY_UNSAFE"));
        }
        let directory = Root::open(&path).map_err(io::Error::other)?;
        let path = path.join(format!("{id}.lock"));
        let file = Self::open(&path, true)?;
        file.try_lock()
            .map_err(|_| io::Error::other("FIXTURE_NATIVE_LOCK_UNAVAILABLE"))?;
        Ok(Self {
            file,
            directory,
            path,
        })
    }
    fn open(path: &Path, create: bool) -> io::Result<File> {
        use std::os::windows::fs::MetadataExt;
        use windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .truncate(false)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("FIXTURE_LOCK_FILE_UNSAFE"));
        }
        Ok(file)
    }
    fn contended(&self) -> io::Result<bool> {
        fn identity(file: &File) -> io::Result<[u32; 3]> {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
                .map_err(io::Error::other)?;
            Ok([
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            ])
        }
        self.directory.source_current().map_err(io::Error::other)?;
        let independent = Self::open(&self.path, false)?;
        if identity(&self.file)? != identity(&independent)? {
            return Err(io::Error::other("FIXTURE_LOCK_FILE_CHANGED"));
        }
        Ok(matches!(
            independent.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ))
    }
}

#[test]
fn NativeDeleteLive_NativeWriterLockLifetimeAndIndependentContention_007() {
    let fixture = Fixture::new().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let guard = NativeWriterLock::acquire(&fixture, &id).unwrap();
    assert!(guard.directory.source_current().is_ok());
    let independent = NativeWriterLock::open(&guard.path, false).unwrap();
    assert!(matches!(
        independent.try_lock(),
        Err(std::fs::TryLockError::WouldBlock)
    ));
    assert!(guard.contended().unwrap());
    drop(guard);
    independent.try_lock().unwrap();
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ReadOnlyTrace {
    frames: Vec<serde_json::Value>,
    outbound: Vec<&'static str>,
    initialize_acknowledged: bool,
    home_verified: bool,
    read_acknowledged: bool,
    thread_id_matched: bool,
    failure: Option<&'static str>,
}
struct ReadOnlyProtocol {
    id: String,
    home_key: String,
    offset: usize,
    done: bool,
    trace: ReadOnlyTrace,
}
impl ReadOnlyProtocol {
    fn new(fixture: &Fixture, id: String) -> Self {
        Self {
            id,
            home_key: Root::open(&fixture.root.join("codex-home"))
                .unwrap()
                .key()
                .to_owned(),
            offset: 0,
            done: false,
            trace: ReadOnlyTrace::default(),
        }
    }
    fn initialize(&self) -> serde_json::Value {
        serde_json::json!({"id":1,"method":"initialize","params":{
            "clientInfo":{"name":"cc_desk","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"explicitGatewayOauth":true,"experimentalApi":false}
        }})
    }
    fn fail(&mut self, code: &'static str) {
        if self.trace.failure.is_none() {
            self.trace.failure = Some(code);
        }
        self.done = true;
    }
    fn observe(&mut self, frame: &serde_json::Value) -> Vec<serde_json::Value> {
        if self.done {
            return vec![];
        }
        if frame.get("method").is_some() {
            if frame.get("id").is_some() {
                self.fail("UNSUPPORTED_SERVER_REQUEST");
            }
            return vec![];
        }
        let expected = if self.trace.initialize_acknowledged {
            2
        } else {
            1
        };
        if frame["id"].as_u64() != Some(expected) {
            self.fail("UNEXPECTED_RESPONSE_ID");
            return vec![];
        }
        if frame.get("error").is_some() {
            self.fail(if expected == 1 {
                "INITIALIZE_RPC_ERROR"
            } else {
                "READ_RPC_ERROR"
            });
            return vec![];
        }
        if !frame["result"].is_object() {
            self.fail("RESULT_OBJECT_MISSING");
            return vec![];
        }
        if expected == 1 {
            self.trace.initialize_acknowledged = true;
            self.trace.home_verified = frame["result"]["codexHome"]
                .as_str()
                .and_then(|path| Root::open(Path::new(path)).ok())
                .is_some_and(|root| root.key() == self.home_key);
            if !self.trace.home_verified {
                self.fail("INITIALIZE_HOME_UNVERIFIED");
                return vec![];
            }
            vec![
                serde_json::json!({"method":"initialized"}),
                serde_json::json!({"id":2,"method":"thread/read","params":{"threadId":self.id,"includeTurns":false}}),
            ]
        } else {
            self.trace.read_acknowledged = true;
            self.trace.thread_id_matched = frame["result"]["thread"]["id"] == self.id;
            self.done = true;
            vec![]
        }
    }
    fn record_frame(&mut self, frame: &serde_json::Value, line: &[u8]) -> io::Result<()> {
        fn token(value: &str) -> String {
            if !value.is_empty()
                && value.len() <= 80
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_/- .".contains(&b) && b != b' ')
            {
                value.into()
            } else {
                "redacted-token".into()
            }
        }
        if self.trace.frames.len() >= 256 {
            return Err(io::Error::other("READ_ONLY_FRAME_LIMIT"));
        }
        let id_type = match frame.get("id") {
            None => "missing",
            Some(serde_json::Value::Number(_)) => "number",
            Some(serde_json::Value::String(_)) => "string",
            _ => "other",
        };
        let keys: Vec<_> = frame["result"]
            .as_object()
            .into_iter()
            .flat_map(|o| o.keys())
            .take(64)
            .map(|key| token(key))
            .collect();
        self.trace.frames.push(serde_json::json!({
            "idType":id_type,"expectedNumericId":frame["id"].as_u64().filter(|id| *id==1 || *id==2),
            "method":frame["method"].as_str().map(token),"resultKeys":keys,
            "errorCode":frame["error"]["code"].as_i64(),"lineSha256":format!("{:x}",Sha256::digest(line))
        }));
        Ok(())
    }
    fn send(
        &mut self,
        input: &mut std::process::ChildStdin,
        frame: &serde_json::Value,
    ) -> io::Result<()> {
        write_read_only_frame(input, frame)?;
        self.trace.outbound.push(match frame["method"].as_str() {
            Some("initialize") => "initialize",
            Some("initialized") => "initialized",
            Some("thread/read") => "thread/read",
            _ => unreachable!(),
        });
        Ok(())
    }
    fn pump(
        &mut self,
        stdout: &Path,
        input: &mut Option<std::process::ChildStdin>,
    ) -> io::Result<()> {
        let mut bytes = vec![];
        File::open(stdout)?
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > OUTPUT_LIMIT || bytes.len() < self.offset {
            return Err(io::Error::other("READ_ONLY_OUTPUT_LIMIT"));
        }
        while let Some(length) = bytes[self.offset..].iter().position(|b| *b == b'\n') {
            if length > 65536 {
                return Err(io::Error::other("READ_ONLY_LINE_LIMIT"));
            }
            let end = self.offset + length + 1;
            let line = &bytes[self.offset..end];
            let frame: serde_json::Value = serde_json::from_slice(line)
                .map_err(|_| io::Error::other("READ_ONLY_FRAME_INVALID"))?;
            self.offset = end;
            self.record_frame(&frame, line)?;
            for next in self.observe(&frame) {
                self.send(
                    input
                        .as_mut()
                        .ok_or_else(|| io::Error::other("READ_ONLY_STDIN_CLOSED"))?,
                    &next,
                )?;
            }
            if self.done {
                input.take();
            }
        }
        if bytes.len() - self.offset > 65536 {
            return Err(io::Error::other("READ_ONLY_LINE_LIMIT"));
        }
        Ok(())
    }
}
fn write_read_only_frame(output: &mut impl Write, frame: &serde_json::Value) -> io::Result<()> {
    let allowed = match frame["method"].as_str() {
        Some("initialize") => {
            frame["id"] == 1
                && frame["params"]["clientInfo"]["name"] == "cc_desk"
                && frame["params"]["clientInfo"]["version"] == env!("CARGO_PKG_VERSION")
                && frame["params"]["capabilities"]["explicitGatewayOauth"] == true
                && frame["params"]["capabilities"]["experimentalApi"] == false
        }
        Some("initialized") => frame.get("id").is_none() && frame.get("params").is_none(),
        Some("thread/read") => {
            frame["id"] == 2
                && frame["params"]["includeTurns"] == false
                && frame["params"]["threadId"]
                    .as_str()
                    .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok())
        }
        _ => false,
    };
    if !allowed {
        return Err(io::Error::other("READ_ONLY_PROTOCOL_METHOD_FORBIDDEN"));
    }
    serde_json::to_writer(&mut *output, frame).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

#[test]
fn NativeDeleteLive_ReadOnlyProtocolWaitsForInitializeAndForbidsMutation_006() {
    let fixture = Fixture::new().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let mut protocol = ReadOnlyProtocol::new(&fixture, id.clone());
    let mut output = vec![];
    write_read_only_frame(&mut output, &protocol.initialize()).unwrap();
    assert!(protocol
        .observe(&serde_json::json!({"method":"configWarning","params":{}}))
        .is_empty());
    let next = protocol.observe(
        &serde_json::json!({"id":1,"result":{"codexHome":fixture.root.join("codex-home")}}),
    );
    assert_eq!(next.len(), 2);
    assert_eq!(next[0]["method"], "initialized");
    assert_eq!(next[1]["method"], "thread/read");
    assert_eq!(next[1]["params"]["threadId"], id);
    assert_eq!(next[1]["params"]["includeTurns"], false);
    for frame in next {
        write_read_only_frame(&mut output, &frame).unwrap();
    }
    let before = output.len();
    assert!(write_read_only_frame(
        &mut output,
        &serde_json::json!({"id":3,"method":"thread/delete","params":{"threadId":id}})
    )
    .is_err());
    assert_eq!(before, output.len());
    assert!(protocol
        .observe(&serde_json::json!({"id":2,"error":{"code":-32000,"message":"fixture"}}))
        .is_empty());
    assert!(protocol.done);
    assert_eq!(protocol.trace.failure, Some("READ_RPC_ERROR"));
    let mut wrong_home = ReadOnlyProtocol::new(&fixture, uuid::Uuid::new_v4().to_string());
    assert!(wrong_home
        .observe(&serde_json::json!({"id":1,"result":{"codexHome":fixture.project}}))
        .is_empty());
    assert_eq!(wrong_home.trace.failure, Some("INITIALIZE_HOME_UNVERIFIED"));
}

fn namespace_absent(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(io::Error::other("SYSTEM_CONFIGURATION_NOT_PROVEN_ABSENT")),
    }
}

#[test]
fn NativeDeleteLive_PreflightRejectsPresentNamespace_001() {
    let root = tempfile::tempdir().unwrap();
    let namespace = root.path().join("Codex");
    assert!(namespace_absent(&namespace).is_ok());
    fs::create_dir(&namespace).unwrap();
    assert!(namespace_absent(&namespace).is_err());
    fs::remove_dir(&namespace).unwrap();
    fs::write(&namespace, b"synthetic namespace").unwrap();
    assert!(namespace_absent(&namespace).is_err());
}

#[test]
fn NativeDeleteLive_FixtureDisablesPluginsAndInheritedAuthentication_002() {
    let fixture = Fixture::new().unwrap();
    let config: toml::Value =
        toml::from_str(&fs::read_to_string(fixture.root.join("codex-home/config.toml")).unwrap())
            .unwrap();
    for feature in [
        "plugins",
        "remote_plugin",
        "plugin_discovery_prefetch",
        "skill_mcp_dependency_install",
        "hooks",
    ] {
        assert_eq!(
            config
                .get("features")
                .and_then(|v| v.get(feature))
                .and_then(toml::Value::as_bool),
            Some(false)
        );
    }
    assert!(!fixture
        .environment
        .contains_key(&OsString::from("OPENAI_API_KEY")));
    assert!(!fixture
        .environment
        .contains_key(&OsString::from("ANTHROPIC_API_KEY")));
    assert_eq!(
        fs::read_to_string(fixture.project.join(".git/HEAD")).unwrap(),
        "ref: refs/heads/fixture\n"
    );
    assert!(!fixture.project.join(".git/commondir").exists());
    assert!(!fixture.project.join(".git/gitdir").exists());
    for key in [
        "CODEX_HOME",
        "CODEX_SQLITE_HOME",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "TEMP",
        "TMP",
    ] {
        let value = Path::new(fixture.environment.get(&OsString::from(key)).unwrap());
        assert!(value.is_absolute() && value.starts_with(&fixture.root));
    }
}

// 只打开系统配置名称的元数据；存在、无权访问或未知均拒绝，不读取值或配置正文。
fn system_configuration_absent() -> io::Result<()> {
    let allocation = unsafe { SHGetKnownFolderPath(&FOLDERID_ProgramData, KF_FLAG_DEFAULT, None) }
        .map_err(|_| io::Error::other("PROGRAM_DATA_UNAVAILABLE"))?;
    let folder = unsafe { allocation.to_string() };
    unsafe { CoTaskMemFree(Some(allocation.0.cast())) };
    let folder = PathBuf::from(folder.map_err(|_| io::Error::other("PROGRAM_DATA_UNAVAILABLE"))?);
    let parent = folder.join("OpenAI");
    match fs::symlink_metadata(&parent) {
        Ok(meta) => {
            use std::os::windows::fs::MetadataExt;
            if !meta.is_dir() || meta.file_attributes() & 0x400 != 0 {
                return Err(io::Error::other("PROGRAM_DATA_PARENT_UNSAFE"));
            }
            namespace_absent(&parent.join("Codex"))?;
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => return Err(io::Error::other("PROGRAM_DATA_UNAVAILABLE")),
    }
    let name: Vec<_> = r"SOFTWARE\Policies\OpenAI\Codex"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(name.as_ptr()),
            Some(REG_OPTION_OPEN_LINK.0),
            KEY_READ | KEY_WOW64_64KEY,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    if status.is_ok() {
        let _ = unsafe { RegCloseKey(key) };
    }
    Err(io::Error::other("POLICY_CONFIGURATION_NOT_PROVEN_ABSENT"))
}

struct Fixture {
    root: PathBuf,
    project: PathBuf,
    environment: BTreeMap<OsString, OsString>,
    leases: Vec<Root>,
}
impl Fixture {
    fn new() -> io::Result<Self> {
        let root = tempfile::Builder::new()
            .prefix("v027-codex-delete-live-")
            .tempdir()?
            .keep();
        println!("NATIVE_DELETE_LIVE_FIXTURE {}", root.display());
        let project = root.join("project");
        for relative in [
            "project/.git/refs/heads",
            "home",
            "roaming",
            "local",
            "xdg/config",
            "xdg/data",
            "xdg/cache",
            "xdg/state",
            "temporary",
            "codex-home/sessions",
            "codex-home/archived_sessions",
            "sqlite",
            "logs",
            "reports",
        ] {
            fs::create_dir_all(root.join(relative))?;
        }
        fs::write(project.join(".git/HEAD"), b"ref: refs/heads/fixture\n")?;
        fs::write(project.join(".git/config"), b"[core]\n\tbare = false\n")?;
        fs::write(root.join("empty-gitconfig"), b"")?;
        let config = serde_json::json!({
            "sqlite_home":root.join("sqlite"), "log_dir":root.join("logs"),
            "cli_auth_credentials_store":"file", "mcp_oauth_credentials_store":"file",
            "check_for_update_on_startup":false,
            "analytics":{"enabled":false}, "feedback":{"enabled":false},
            "features":{"plugins":false,"remote_plugin":false,"plugin_discovery_prefetch":false,"skill_mcp_dependency_install":false,"hooks":false},
            "otel":{"exporter":"none","trace_exporter":"none","metrics_exporter":"none"},
            "model_provider":"fixture", "model":"fixture-no-model-call",
            "model_providers":{"fixture":{"name":"fixture","base_url":"http://127.0.0.1:9/v1","wire_api":"responses","requires_openai_auth":false}},
            "mcp_servers":{}
        });
        fs::write(
            root.join("codex-home/config.toml"),
            toml::to_string(&config).map_err(io::Error::other)?,
        )?;
        let system = std::env::var_os("SystemRoot")
            .filter(|v| Path::new(v).is_absolute())
            .ok_or_else(|| io::Error::other("SYSTEM_ROOT_UNAVAILABLE"))?;
        let mut environment = BTreeMap::new();
        for key in ["SystemRoot", "WINDIR"] {
            environment.insert(key.into(), system.clone());
        }
        environment.insert(
            "PATH".into(),
            Path::new(&system).join("System32").into_os_string(),
        );
        for (key, relative) in [
            ("HOME", "home"),
            ("USERPROFILE", "home"),
            ("APPDATA", "roaming"),
            ("LOCALAPPDATA", "local"),
            ("XDG_CONFIG_HOME", "xdg/config"),
            ("XDG_DATA_HOME", "xdg/data"),
            ("XDG_CACHE_HOME", "xdg/cache"),
            ("XDG_STATE_HOME", "xdg/state"),
            ("TEMP", "temporary"),
            ("TMP", "temporary"),
            ("CODEX_HOME", "codex-home"),
            ("CODEX_SQLITE_HOME", "sqlite"),
            ("GIT_CONFIG_GLOBAL", "empty-gitconfig"),
            ("GIT_CEILING_DIRECTORIES", ""),
        ] {
            environment.insert(key.into(), root.join(relative).into_os_string());
        }
        let home = root.join("home");
        let home = home
            .to_str()
            .ok_or_else(|| io::Error::other("FIXTURE_PATH_UNSUPPORTED"))?;
        if home.as_bytes().get(1) != Some(&b':') {
            return Err(io::Error::other("FIXTURE_DRIVE_UNSUPPORTED"));
        }
        environment.insert("HOMEDRIVE".into(), home[..2].into());
        environment.insert("HOMEPATH".into(), home[2..].into());
        environment.insert("GIT_CONFIG_NOSYSTEM".into(), "1".into());
        let mut leases = Vec::new();
        for relative in [
            "",
            "project",
            "home",
            "roaming",
            "local",
            "xdg",
            "temporary",
            "sqlite",
            "logs",
            "reports",
        ] {
            leases.push(Root::open(&root.join(relative)).map_err(io::Error::other)?);
        }
        let home_lease = Root::open(&root.join("codex-home")).map_err(io::Error::other)?;
        leases.extend(
            home_lease
                .pin_delete_containers()
                .map_err(io::Error::other)?,
        );
        leases.push(home_lease);
        Ok(Self {
            root,
            project,
            environment,
            leases,
        })
    }
}

fn verified_executable() -> io::Result<File> {
    use std::os::windows::fs::MetadataExt;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(CODEX_EXE)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.file_attributes() & 0x400 != 0
        || metadata.len() != CODEX_BYTES
    {
        return Err(io::Error::other("INSTALLED_EXECUTABLE_UNSAFE"));
    }
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    if format!("{:x}", hash.finalize()) != CODEX_SHA256 {
        return Err(io::Error::other("INSTALLED_EXECUTABLE_HASH_CHANGED"));
    }
    Ok(file)
}

// 测试模块不能访问生产内部 Job 类型；此围栏只管理本次显式探测创建的进程组。
struct ProbeJob(HANDLE);
impl Drop for ProbeJob {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}
impl ProbeJob {
    fn new() -> io::Result<Self> {
        let job =
            Self(unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(io::Error::other)?);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        }
        .map_err(io::Error::other)?;
        Ok(job)
    }
    fn assign_and_resume(&self, child: &Child) -> io::Result<()> {
        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }
            .map_err(io::Error::other)?;
        let snapshot = Self(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }.map_err(io::Error::other)?,
        );
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        unsafe { Thread32First(snapshot.0, &mut entry) }.map_err(io::Error::other)?;
        loop {
            if entry.th32OwnerProcessID == child.id() {
                let thread = Self(
                    unsafe { OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) }
                        .map_err(io::Error::other)?,
                );
                if unsafe { ResumeThread(thread.0) } == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            if unsafe { Thread32Next(snapshot.0, &mut entry) }.is_err() {
                break;
            }
        }
        Err(io::Error::other("SUSPENDED_THREAD_UNAVAILABLE"))
    }
    fn empty(&self) -> io::Result<bool> {
        let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        unsafe {
            QueryInformationJobObject(
                Some(self.0),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            )
        }
        .map_err(io::Error::other)?;
        Ok(info.ActiveProcesses == 0)
    }
    fn terminate(&self) -> io::Result<()> {
        unsafe { TerminateJobObject(self.0, 1) }.map_err(io::Error::other)
    }
}

#[derive(Debug)]
struct OwnedReport {
    status: ExitStatus,
    forced: bool,
    root_before_group: bool,
    protocol: Option<ReadOnlyTrace>,
}
struct OwnedTask(mpsc::Receiver<io::Result<OwnedReport>>);
impl OwnedTask {
    fn wait(&self, timeout: Duration) -> io::Result<OwnedReport> {
        self.0.recv_timeout(timeout).map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => {
                io::Error::new(io::ErrorKind::TimedOut, "PROBE_OWNER_PENDING")
            }
            mpsc::RecvTimeoutError::Disconnected => {
                io::Error::new(io::ErrorKind::BrokenPipe, "PROBE_OWNER_DISCONNECTED")
            }
        })?
    }
    fn wait_until_settled(&self) -> io::Result<OwnedReport> {
        let mut pending = false;
        loop {
            let timeout = if pending {
                Duration::from_secs(5)
            } else {
                Duration::from_secs(35)
            };
            match self.wait(timeout) {
                Ok(mut report) => {
                    // 超时后的等待仅为回收；即使随后自然退出，也不能再把原命令判为及时成功。
                    report.forced |= pending;
                    return Ok(report);
                }
                Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                    pending = true;
                    println!("NATIVE_DELETE_LIVE_OWNER_PENDING");
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn start_owned(
    fixture: Arc<Fixture>,
    name: String,
    mut command: Command,
    executable_lease: Option<File>,
    inject_observation_fault: bool,
    system_metadata_gates: bool,
    protocol: Option<ReadOnlyProtocol>,
) -> io::Result<OwnedTask> {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("isolated-probe-owner".into())
        .spawn(move || {
            let _executable_lease = executable_lease;
            let result = owned_command(
                &fixture,
                &name,
                &mut command,
                inject_observation_fault,
                protocol,
            );
            // 即使调用端已报告 pending，实际回收后的系统元数据复查仍由持有资源的 owner 完成。
            let after = if system_metadata_gates {
                system_configuration_absent()
            } else {
                Ok(())
            };
            let result = result.and_then(|report| after.map(|()| report));
            let _ = sender.send(result);
        })?;
    Ok(OwnedTask(receiver))
}

fn owned_command(
    fixture: &Fixture,
    name: &str,
    command: &mut Command,
    inject_observation_fault: bool,
    mut protocol: Option<ReadOnlyProtocol>,
) -> io::Result<OwnedReport> {
    let stdout = fixture.root.join(format!("reports/{name}.stdout"));
    let stderr = fixture.root.join(format!("reports/{name}.stderr"));
    let job = ProbeJob::new()?;
    command
        .env_clear()
        .envs(&fixture.environment)
        .current_dir(&fixture.project)
        .stdin(if protocol.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(File::create(&stdout)?)
        .stderr(File::create(&stderr)?)
        .creation_flags(0x08000000 | 0x4);
    let mut child = command.spawn()?;
    let assigned = job.assign_and_resume(&child);
    if assigned.is_err() {
        let _ = child.kill();
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut status = None;
    let mut forced = false;
    let mut failed = assigned.is_err();
    let mut input = child.stdin.take();
    if assigned.is_ok() {
        if let Some(protocol) = protocol.as_mut() {
            let frame = protocol.initialize();
            let sent = input
                .as_mut()
                .ok_or_else(|| io::Error::other("READ_ONLY_STDIN_MISSING"))
                .and_then(|input| protocol.send(input, &frame));
            if sent.is_err() {
                protocol.fail("INITIALIZE_SEND_FAILED");
                failed = true;
                input.take();
            }
        }
    }
    let mut root_before_group = false;
    let mut fault = inject_observation_fault;
    loop {
        if let Some(protocol) = protocol.as_mut() {
            if protocol.pump(&stdout, &mut input).is_err() {
                protocol.fail("PROTOCOL_STREAM_ERROR");
                failed = true;
                input.take();
            }
        }
        if fault {
            failed = true;
            fault = false;
        }
        failed |= fixture
            .leases
            .iter()
            .any(|lease| lease.source_current().is_err());
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => failed = true,
            }
        }
        match job.empty() {
            Ok(true) if status.is_some() => break,
            Err(_) => failed = true,
            Ok(false) if status.is_some() => root_before_group = true,
            _ => {}
        }
        let output_ok = [&stdout, &stderr]
            .iter()
            .all(|path| fs::metadata(path).is_ok_and(|m| m.len() <= OUTPUT_LIMIT));
        if !forced && (failed || Instant::now() >= deadline || !output_ok) {
            failed |= job.terminate().is_err();
            forced = true;
        }
        // 若回收不能确认，保留二进制租约及 Job，不能把终止请求当成回收完成。
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(protocol) = protocol.as_mut() {
        if protocol.pump(&stdout, &mut input).is_err() {
            protocol.fail("PROTOCOL_STREAM_ERROR");
            failed = true;
        }
    }
    assigned?;
    if fs::metadata(stdout)?.len() > OUTPUT_LIMIT || fs::metadata(stderr)?.len() > OUTPUT_LIMIT {
        return Err(io::Error::other("PROBE_OUTPUT_LIMIT"));
    }
    Ok(OwnedReport {
        status: status.unwrap(),
        forced: forced || failed || Instant::now() >= deadline,
        root_before_group,
        protocol: protocol.map(|protocol| protocol.trace),
    })
}

fn probe(fixture: &Arc<Fixture>, name: &str, arguments: &[&str]) -> io::Result<ExitStatus> {
    system_configuration_absent()?;
    let executable_lease = verified_executable()?;
    let mut command = Command::new(CODEX_EXE);
    command.args(arguments);
    let task = start_owned(
        fixture.clone(),
        name.into(),
        command,
        Some(executable_lease),
        false,
        true,
        None,
    )?;
    let report = task.wait_until_settled()?;
    if report.forced {
        return Err(io::Error::other("PROBE_FORCED_TERMINATION"));
    }
    Ok(report.status)
}

fn synthetic_command(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "tests::native_cli_delete_live::NativeDeleteLive_SyntheticOwnedChild_098",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    command.env("CC_DESK_SYNTHETIC_PROBE_MODE", mode);
    command
}

#[test]
fn NativeDeleteLive_GroupWaitsForDescendantAndRetainsPendingOwner_003() {
    let fixture = Arc::new(Fixture::new().unwrap());
    let mut fixture = fixture;
    Arc::get_mut(&mut fixture)
        .unwrap()
        .environment
        .insert("CC_DESK_SYNTHETIC_PROBE_MODE".into(), "parent".into());
    let task = start_owned(
        fixture.clone(),
        "synthetic-late".into(),
        synthetic_command("parent"),
        None,
        false,
        false,
        None,
    )
    .unwrap();
    assert_eq!(
        task.wait(Duration::from_millis(10)).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert!(Arc::strong_count(&fixture) >= 2);
    assert!(fs::rename(&fixture.project, fixture.root.join("replacement-project")).is_err());
    let report = task.wait(Duration::from_secs(5)).unwrap();
    assert!(report.status.success() && !report.forced && report.root_before_group);
    assert_eq!(
        fs::read(fixture.root.join("reports/synthetic-late-marker")).unwrap(),
        b"descendant completed"
    );
}

#[test]
fn NativeDeleteLive_ObservationFaultReapsOwnGroupBeforeReturning_004() {
    let mut fixture = Arc::new(Fixture::new().unwrap());
    Arc::get_mut(&mut fixture)
        .unwrap()
        .environment
        .insert("CC_DESK_SYNTHETIC_PROBE_MODE".into(), "leaf".into());
    let task = start_owned(
        fixture.clone(),
        "synthetic-fault".into(),
        synthetic_command("leaf"),
        None,
        true,
        false,
        None,
    )
    .unwrap();
    let report = task.wait(Duration::from_secs(5)).unwrap();
    assert!(report.forced && !report.status.success());
    assert!(!fixture.root.join("reports/synthetic-late-marker").exists());
}

#[test]
#[ignore = "synthetic descendant worker; only its owning test may invoke it"]
fn NativeDeleteLive_SyntheticOwnedChild_098() {
    let mode = std::env::var("CC_DESK_SYNTHETIC_PROBE_MODE").unwrap();
    if mode == "parent" {
        let _child = synthetic_command("leaf").spawn().unwrap();
        // 父 worker 故意先退出，后代的唯一回收责任由外部测试 owner 的 Job 承担。
        std::process::exit(0);
    } else {
        assert_eq!(mode, "leaf");
        std::thread::sleep(Duration::from_millis(500));
        fs::write("../reports/synthetic-late-marker", b"descendant completed").unwrap();
    }
}

#[test]
#[ignore = "requires separately reviewed isolation Go; never run through the default suite"]
fn NativeDeleteLive_InstalledCodexStageA_099() {
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_DELETE_LIVE_GO").as_deref(),
        Ok("stage-a-isolated-probes")
    );
    let fixture = Arc::new(Fixture::new().unwrap());
    // HOME 环境覆盖不代表 Windows KnownFolder 已隔离；本入口须先通过独立源码审查。
    // 同一固定安装文件的版本已在先前独立 Job 观察，旧预期失败保留；这里只补未执行项。
    for (name, arguments) in [
        ("help", &["--help"][..]),
        (
            "app-server-help",
            &["app-server", "--strict-config", "--help"][..],
        ),
        (
            "startup-config",
            &["app-server", "--strict-config", "--print-startup-config"][..],
        ),
    ] {
        let status = probe(&fixture, name, arguments).unwrap();
        let stdout = fs::read(fixture.root.join(format!("reports/{name}.stdout"))).unwrap();
        let stderr = fs::read(fixture.root.join(format!("reports/{name}.stderr"))).unwrap();
        let mut report = serde_json::json!({
            "probe":name,"exitCode":status.code(),"success":status.success(),
            "stdoutBytes":stdout.len(),"stderrBytes":stderr.len(),
            "stdoutSha256":format!("{:x}",Sha256::digest(&stdout)),
            "stderrSha256":format!("{:x}",Sha256::digest(&stderr)),
            "rootReaped":true,"jobEmpty":true
        });
        let text = std::str::from_utf8(&stdout).unwrap_or("");
        let shape_ok = match name {
            "help" => {
                let present = text.contains("app-server");
                report["appServerCommandAdvertised"] = present.into();
                present
            }
            "app-server-help" => {
                let present =
                    text.contains("--print-startup-config") && text.contains("--strict-config");
                report["startupConfigAndStrictFlagsAdvertised"] = present.into();
                present
            }
            "startup-config" => {
                let value: serde_json::Value = serde_json::from_slice(&stdout).unwrap_or_default();
                let version_present = value.get("appServerVersion").is_some();
                let requirements_present = value.get("configRequirements").is_some();
                report["appServerVersionPresent"] = version_present.into();
                report["configRequirementsPresent"] = requirements_present.into();
                report["expectedVersionObserved"] =
                    (value["appServerVersion"] == CODEX_VERSION).into();
                version_present
                    && requirements_present
                    && value["appServerVersion"] == CODEX_VERSION
            }
            _ => unreachable!(),
        };
        fs::write(
            fixture.root.join(format!("reports/{name}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("NATIVE_DELETE_LIVE_PROBE {report}");
        assert!(status.success() && shape_ok, "probe failed: {name}");
    }
}

fn synthetic_rollout(
    fixture: &Fixture,
    home: &Path,
    id: &str,
    archived: bool,
) -> (PathBuf, Vec<u8>) {
    let now = chrono::Utc::now();
    let timestamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let directory = if archived {
        home.join("archived_sessions")
    } else {
        home.join("sessions")
            .join(now.format("%Y/%m/%d").to_string())
    };
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!(
        "rollout-{}-{id}.jsonl",
        now.format("%Y-%m-%dT%H-%M-%S")
    ));
    let line = serde_json::json!({
        "timestamp":timestamp,"type":"session_meta","payload":{
            "session_id":id,"id":id,"timestamp":timestamp,"cwd":fixture.project,
            "originator":"cc_desk_acceptance","cli_version":CODEX_VERSION,"source":"cli"
        }
    });
    let bytes = format!("{line}\n").into_bytes();
    fs::write(&path, &bytes).unwrap();
    (path, bytes)
}

enum BackendProgram {
    UnavailableFixture,
    InstalledPinned,
}

struct BackendFixture {
    fixture: Arc<Fixture>,
    repository: WorkspaceRepository,
    launch: Arc<LaunchService>,
    service: ProjectionService,
    caller: CallerIdentity,
    source: SourceRef,
    project_id: String,
    id: String,
    rollout: PathBuf,
    original: Vec<u8>,
    active_id: String,
    active_rollout: PathBuf,
    unrelated_rollout: PathBuf,
    other_source: SourceRef,
    other_rollout: PathBuf,
    observations: Vec<&'static str>,
}
impl BackendFixture {
    fn new(fixture: Arc<Fixture>, program: BackendProgram) -> Self {
        let home = fixture.root.join("codex-home");
        let id = uuid::Uuid::new_v4().to_string();
        let (rollout, original) = synthetic_rollout(&fixture, &home, &id, true);
        let active_id = uuid::Uuid::new_v4().to_string();
        let (active_rollout, _) = synthetic_rollout(&fixture, &home, &active_id, false);
        let (unrelated_rollout, _) =
            synthetic_rollout(&fixture, &home, &uuid::Uuid::new_v4().to_string(), true);
        let other_home = fixture.root.join("other-source");
        fs::create_dir_all(other_home.join("sessions")).unwrap();
        let (other_rollout, _) = synthetic_rollout(&fixture, &other_home, &id, true);
        let repository =
            WorkspaceRepository::open(fixture.root.join("desk/workspace.json")).unwrap();
        let project_id = crate::cli::workspace::register_project(&repository, &fixture.project)
            .unwrap()
            .project_id;
        let program = match program {
            BackendProgram::UnavailableFixture => fixture
                .project
                .join("must-not-exist.exe")
                .to_str()
                .unwrap()
                .to_owned(),
            BackendProgram::InstalledPinned => CODEX_EXE.to_owned(),
        };
        for (profile_id, selected_home) in [
            ("native-delete-fixture", home),
            ("other-profile", other_home),
        ] {
            let mut profile = Profile::new(profile_id, CliKind::Codex);
            // 另源仅验证本地授权拒绝，不给没有独立安全配置的另根任何可执行能力。
            profile.program_path = Override::Set(if profile_id == "other-profile" {
                fixture
                    .project
                    .join("must-not-exist.exe")
                    .to_str()
                    .unwrap()
                    .to_owned()
            } else {
                program.clone()
            });
            profile.default_args = Override::Set(vec![]);
            profile.env.insert(
                "CODEX_HOME".into(),
                Override::Set(EnvValue::Literal {
                    value: selected_home.to_str().unwrap().into(),
                    non_secret: true,
                }),
            );
            repository
                .apply(
                    repository.read().unwrap().revision,
                    Patch::Create { profile },
                )
                .unwrap();
        }
        let launch = Arc::new(LaunchService::new(
            repository.clone(),
            Some(fixture.environment.clone()),
            None,
        ));
        let caller = launch.registry().activate_window("main").unwrap();
        let service = ProjectionService::new(launch.clone());
        let source = Self::scope(
            &service,
            &repository,
            &caller,
            "native-delete-fixture",
            &project_id,
        );
        let other_source =
            Self::scope(&service, &repository, &caller, "other-profile", &project_id);
        Self {
            fixture,
            repository,
            launch,
            service,
            caller,
            source,
            project_id,
            id,
            rollout,
            original,
            active_id,
            active_rollout,
            unrelated_rollout,
            other_source,
            other_rollout,
            observations: vec![],
        }
    }
    fn scope(
        service: &ProjectionService,
        repository: &WorkspaceRepository,
        caller: &CallerIdentity,
        profile: &str,
        project: &str,
    ) -> SourceRef {
        service
            .scope(
                caller,
                &ScopeTarget::Profile {
                    profile_id: profile.into(),
                    expected_profile_revision: repository.get_profile(profile).unwrap().revision,
                    project_ids: vec![project.into()],
                },
            )
            .unwrap()
    }
    fn request(&self, source: &SourceRef, id: &str) -> PreviewRequest {
        PreviewRequest {
            operation_id: uuid::Uuid::new_v4().to_string(),
            source: source.clone(),
            session_key: serde_json::json!(["local", "codex", source.source_root_key, id])
                .to_string(),
        }
    }
    fn confirmation(request: &PreviewRequest, preview_id: String) -> ConfirmRequest {
        ConfirmRequest {
            operation_id: request.operation_id.clone(),
            preview_id,
            acknowledge_permanent_delete: true,
            acknowledge_native_subtree: true,
        }
    }
    fn preserved(&self) {
        assert!(self
            .fixture
            .leases
            .iter()
            .all(|lease| lease.source_current().is_ok()));
        assert_eq!(fs::read(&self.rollout).unwrap(), self.original);
        assert!(
            self.active_rollout.exists()
                && self.unrelated_rollout.exists()
                && self.other_rollout.exists()
        );
        assert!(self.repository.admission().check_metadata_idle().is_ok());
    }
    fn check_pre_mutation_guards(&mut self) {
        let request = self.request(&self.source, &self.id);
        let preview = self
            .service
            .preview_delete(&self.caller, request.clone())
            .unwrap();
        assert_eq!(preview.state, "ready");
        assert_eq!(preview.native_session_id.as_deref(), Some(self.id.as_str()));
        self.preserved();
        self.observations.push("preview-only-original-retained");
        // 取消止于预览；缺任一确认都不能调用原生删除。
        let confirmation = Self::confirmation(&request, preview.preview_id.unwrap());
        for (permanent, subtree) in [(false, true), (true, false)] {
            let mut incomplete = confirmation.clone();
            incomplete.acknowledge_permanent_delete = permanent;
            incomplete.acknowledge_native_subtree = subtree;
            assert_eq!(
                self.service
                    .confirm_delete(&self.caller, incomplete)
                    .err()
                    .unwrap()
                    .code,
                "DELETE_CONFIRMATION_REQUIRED"
            );
            self.preserved();
        }
        self.observations.push("either-missing-ack-rejected");
        let active = self
            .service
            .preview_delete(&self.caller, self.request(&self.source, &self.active_id))
            .unwrap();
        assert_eq!(active.reason, Some("DELETE_ROOT_NOT_ARCHIVED"));
        assert!(active.preview_id.is_none());
        self.preserved();
        self.observations.push("unarchived-sessions-root-rejected");
        let mut forged_root = self.request(&self.source, &self.id);
        forged_root.source.source_root_key = self.other_source.source_root_key.clone();
        assert!(self
            .service
            .preview_delete(&self.caller, forged_root)
            .is_err());
        let mut forged_profile = self.request(&self.source, &self.id);
        forged_profile.source.profile_id = "other-profile".into();
        assert!(self
            .service
            .preview_delete(&self.caller, forged_profile)
            .is_err());
        let mut retarget = self.request(&self.other_source, &self.id);
        retarget.operation_id = request.operation_id.clone();
        assert_eq!(
            self.service
                .preview_delete(&self.caller, retarget)
                .err()
                .unwrap()
                .code,
            "DELETE_OPERATION_CONFLICT"
        );
        self.preserved();
        self.observations
            .push("forged-root-profile-and-operation-retarget-rejected");
        let mut changed = self.original.clone();
        changed.extend_from_slice(b"\n");
        fs::write(&self.rollout, changed).unwrap();
        assert_eq!(
            self.service
                .confirm_delete(&self.caller, confirmation.clone())
                .unwrap()
                .reason,
            Some("DELETE_PREVIEW_CHANGED")
        );
        fs::write(&self.rollout, &self.original).unwrap();
        assert_eq!(
            self.service
                .confirm_delete(&self.caller, confirmation)
                .unwrap()
                .reason,
            Some("DELETE_PREVIEW_CHANGED")
        );
        self.preserved();
        self.observations
            .push("changed-preview-consumed-even-after-restoration");
        let revision_request = self.request(&self.source, &self.id);
        let revision_preview = self
            .service
            .preview_delete(&self.caller, revision_request.clone())
            .unwrap();
        let revision_confirmation =
            Self::confirmation(&revision_request, revision_preview.preview_id.unwrap());
        self.repository
            .apply(
                self.repository.read().unwrap().revision,
                Patch::Update {
                    id: "native-delete-fixture".into(),
                    changes: serde_json::json!({"name":"updated-fixture"})
                        .as_object()
                        .unwrap()
                        .clone(),
                },
            )
            .unwrap();
        assert!(self
            .service
            .confirm_delete(&self.caller, revision_confirmation)
            .is_err());
        self.preserved();
        self.source = Self::scope(
            &self.service,
            &self.repository,
            &self.caller,
            "native-delete-fixture",
            &self.project_id,
        );
        self.observations
            .push("profile-revision-revokes-old-confirmation");
    }
    fn confirm_and_settle(
        &self,
        confirmation: ConfirmRequest,
    ) -> Result<Receipt, crate::cli::types::SafeError> {
        let result = self
            .service
            .confirm_delete(&self.caller, confirmation.clone());
        // 返回 pending 不是 owner 已结束；宿主持续保活，只读协调同一次调用。
        while self.repository.admission().check_metadata_idle().is_err() {
            println!("NATIVE_DELETE_LIVE_PRODUCT_OWNER_PENDING");
            std::thread::sleep(Duration::from_secs(1));
        }
        let receipt = result?;
        if receipt.reason == Some("DELETE_OWNER_PENDING") {
            self.service.inspect_delete(
                &self.caller,
                InspectRequest {
                    operation_id: confirmation.operation_id,
                },
            )
        } else {
            Ok(receipt)
        }
    }
}

#[test]
fn NativeDeleteLive_BackendGuardsBeforeExecutableAdmission_005() {
    // 此自动用例刻意选不存在的任务文件；即使保护回归也不能执行已安装 CLI。
    let mut backend = BackendFixture::new(
        Arc::new(Fixture::new().unwrap()),
        BackendProgram::UnavailableFixture,
    );
    backend.check_pre_mutation_guards();
    assert_eq!(backend.observations.len(), 6);
}

#[test]
fn NativeDeleteLive_UnavailableProgramIsPrespawnFailure_008() {
    let backend = BackendFixture::new(
        Arc::new(Fixture::new().unwrap()),
        BackendProgram::UnavailableFixture,
    );
    assert!(
        matches!(fs::symlink_metadata(backend.fixture.project.join("must-not-exist.exe")), Err(ref error) if error.kind()==io::ErrorKind::NotFound)
    );
    let request = backend.request(&backend.source, &backend.id);
    let preview = backend
        .service
        .preview_delete(&backend.caller, request.clone())
        .unwrap();
    assert_eq!(preview.state, "ready");
    let confirmation = BackendFixture::confirmation(&request, preview.preview_id.unwrap());
    let receipt = backend.confirm_and_settle(confirmation).unwrap();
    backend.preserved();
    let report = serde_json::json!({"stage":"unavailable-selected-program","receipt":receipt,"originalUnchanged":true,"ownerIdle":true});
    fs::write(
        backend.fixture.root.join("reports/prespawn-error.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NATIVE_DELETE_LIVE_PRESPAWN {report}");
    assert_eq!(
        (receipt.state, receipt.reason, receipt.root_present),
        ("blocked", Some("DELETE_PREPARE_FAILED"), Some(true))
    );
}

#[test]
#[ignore = "requires frozen-source review and separate stage-B synthetic-delete Go"]
fn NativeDeleteLive_InstalledCodexBackendStageB_100() {
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_DELETE_LIVE_GO").as_deref(),
        Ok("stage-b-product-delete")
    );
    let fixture = Arc::new(Fixture::new().unwrap());
    system_configuration_absent().unwrap();
    let _executable_lease = verified_executable().unwrap();
    let _configuration_lease = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(fixture.root.join("codex-home/config.toml"))
        .unwrap();
    assert!(probe(
        &fixture,
        "stage-b-strict-config",
        &["app-server", "--strict-config", "--print-startup-config"]
    )
    .unwrap()
    .success());
    let startup: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.root.join("reports/stage-b-strict-config.stdout")).unwrap(),
    )
    .unwrap();
    assert_eq!(startup["appServerVersion"], CODEX_VERSION);
    assert!(startup.get("configRequirements").is_some());
    let mut backend = BackendFixture::new(fixture.clone(), BackendProgram::InstalledPinned);
    backend.check_pre_mutation_guards();
    let mut original_handle = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_DELETE.0)
        .open(&backend.rollout)
        .unwrap();
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    unsafe {
        GetFileInformationByHandle(HANDLE(original_handle.as_raw_handle()), &mut information)
    }
    .unwrap();
    assert_eq!(information.nNumberOfLinks, 1);
    let mut original_before = vec![];
    original_handle.read_to_end(&mut original_before).unwrap();
    assert_eq!(original_before, backend.original);
    // 基线不把 Windows 已打开文件的删除语义当作前提；捕获唯一原件身份后先关闭 reader。
    drop(original_handle);
    let request = backend.request(&backend.source, &backend.id);
    let preview = backend
        .service
        .preview_delete(&backend.caller, request.clone())
        .unwrap();
    assert_eq!(preview.state, "ready");
    let confirmation = BackendFixture::confirmation(&request, preview.preview_id.unwrap());
    system_configuration_absent().unwrap();
    let result = backend.confirm_and_settle(confirmation.clone());
    // 到这里生产 owner 的整个 Job 已收口，才能断言或结束测试宿主。
    system_configuration_absent().unwrap();
    let receipt = result.unwrap();
    let root_absent = matches!(fs::symlink_metadata(&backend.rollout), Err(ref error) if error.kind() == io::ErrorKind::NotFound);
    let mut report = serde_json::json!({
        "stage":"backend-service-real-cli","version":CODEX_VERSION,"observations":backend.observations,
        "receipt":receipt,"rootOriginalAbsent":root_absent,"originalOnlyOneHardLinkBeforeDelete":true,
        "originalFileIdBeforeDelete":[information.dwVolumeSerialNumber,information.nFileIndexHigh,information.nFileIndexLow],
        "originalBytesSha256":format!("{:x}",Sha256::digest(&original_before)),
        "originalBytesMatchedBeforeDelete":true,"unarchivedRootRetained":backend.active_rollout.exists(),
        "unrelatedRootRetained":backend.unrelated_rollout.exists(),"sameUuidOtherRootRetained":backend.other_rollout.exists(),
        "nativeDescendants":"unverified","documentBridgeExercised":false,"visibleDialogsExercised":false
    });
    fs::write(
        fixture.root.join("reports/stage-b-result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NATIVE_DELETE_LIVE_PRODUCT {report}");
    assert_eq!(receipt.state, "acknowledged");
    assert_eq!(receipt.root_present, Some(false));
    assert!(root_absent);
    assert!(
        backend.active_rollout.exists()
            && backend.unrelated_rollout.exists()
            && backend.other_rollout.exists()
    );
    let duplicate = backend.confirm_and_settle(confirmation).unwrap();
    let inspected = backend
        .service
        .inspect_delete(
            &backend.caller,
            InspectRequest {
                operation_id: request.operation_id,
            },
        )
        .unwrap();
    assert_eq!(
        serde_json::to_value(&duplicate).unwrap(),
        serde_json::to_value(&receipt).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&inspected).unwrap(),
        serde_json::to_value(&receipt).unwrap()
    );
    system_configuration_absent().unwrap();
    report["duplicateAndInspectReusedReceipt"] = true.into();
    fs::write(
        fixture.root.join("reports/stage-b-result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NATIVE_DELETE_LIVE_PRODUCT_FINAL {report}");
    assert!(backend
        .launch
        .repository()
        .admission()
        .check_metadata_idle()
        .is_ok());
}

#[test]
#[ignore = "requires frozen-source NoMutation review and separate read-only protocol Go"]
fn NativeDeleteLive_ReadOnlyNativeProtocol_101() {
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_DELETE_LIVE_GO").as_deref(),
        Ok("stage-b-readonly-protocol")
    );
    let fixture = Arc::new(Fixture::new().unwrap());
    let id = uuid::Uuid::new_v4().to_string();
    let (rollout, original) =
        synthetic_rollout(&fixture, &fixture.root.join("codex-home"), &id, true);
    let protocol = ReadOnlyProtocol::new(&fixture, id.clone());
    system_configuration_absent().unwrap();
    let executable_lease = verified_executable().unwrap();
    let _configuration_lease = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(fixture.root.join("codex-home/config.toml"))
        .unwrap();
    let mut command = Command::new(CODEX_EXE);
    command.args(["app-server", "--strict-config"]);
    let task = start_owned(
        fixture.clone(),
        "readonly-protocol".into(),
        command,
        Some(executable_lease),
        false,
        true,
        Some(protocol),
    )
    .unwrap();
    let report = task.wait_until_settled().unwrap();
    // 仅输出帧形状和哈希；响应正文、错误消息和 stderr 原件只保存在新任务 fixture。
    let trace = report.protocol.unwrap();
    let logical_stage = if trace.read_acknowledged {
        "read-acknowledged"
    } else if trace.outbound.contains(&"thread/read") {
        "thread-read"
    } else if trace.initialize_acknowledged {
        "startup-origin"
    } else {
        "initialize"
    };
    let safe = serde_json::json!({
        "stage":"fresh-root-read-only-native-protocol","version":CODEX_VERSION,"clientVersion":env!("CARGO_PKG_VERSION"),
        "experimentalApiExplicitFalse":true,"requestedNativeId":id,"trace":trace,
        "logicalStage":logical_stage,"sentMethods":trace.outbound,"deleteDispatchAllowed":false,"strictConfig":true,
        "exitCode":report.status.code(),"forced":report.forced,"rootReaped":true,"jobEmpty":true,
        "stdoutBytes":fs::metadata(fixture.root.join("reports/readonly-protocol.stdout")).unwrap().len(),
        "stderrBytes":fs::metadata(fixture.root.join("reports/readonly-protocol.stderr")).unwrap().len(),
        "syntheticOriginalUnchanged":fs::read(rollout).unwrap()==original
    });
    fs::write(
        fixture.root.join("reports/readonly-protocol.json"),
        serde_json::to_vec_pretty(&safe).unwrap(),
    )
    .unwrap();
    println!("NATIVE_DELETE_LIVE_READ_ONLY {safe}");
    assert!(report.status.success() && !report.forced);
    assert!(
        trace.initialize_acknowledged
            && trace.home_verified
            && trace.read_acknowledged
            && trace.thread_id_matched
    );
    assert_eq!(safe["syntheticOriginalUnchanged"], true);
}

#[test]
#[ignore = "requires frozen-source review and separate synthetic native-writer-lock Go"]
fn NativeDeleteLive_InstalledCodexWriterLock_102() {
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_DELETE_LIVE_GO").as_deref(),
        Ok("stage-b-writer-lock")
    );
    let fixture = Arc::new(Fixture::new().unwrap());
    system_configuration_absent().unwrap();
    let _executable_lease = verified_executable().unwrap();
    let _configuration_lease = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(fixture.root.join("codex-home/config.toml"))
        .unwrap();
    assert!(probe(
        &fixture,
        "writer-strict-config",
        &["app-server", "--strict-config", "--print-startup-config"]
    )
    .unwrap()
    .success());
    let startup: serde_json::Value = serde_json::from_slice(
        &fs::read(fixture.root.join("reports/writer-strict-config.stdout")).unwrap(),
    )
    .unwrap();
    assert_eq!(startup["appServerVersion"], CODEX_VERSION);
    assert!(startup.get("configRequirements").is_some());
    let backend = BackendFixture::new(fixture.clone(), BackendProgram::InstalledPinned);
    let writer_guard = NativeWriterLock::acquire(&fixture, &backend.id).unwrap();
    let lock_contended_before = writer_guard.contended().unwrap();
    assert!(lock_contended_before);
    backend.preserved();
    let controls = [
        &backend.active_rollout,
        &backend.unrelated_rollout,
        &backend.other_rollout,
    ];
    let before_hashes: Vec<_> = controls
        .iter()
        .map(|path| format!("{:x}", Sha256::digest(fs::read(path).unwrap())))
        .collect();
    let request = backend.request(&backend.source, &backend.id);
    let preview = backend
        .service
        .preview_delete(&backend.caller, request.clone())
        .unwrap();
    assert_eq!(preview.state, "ready");
    assert_eq!(
        preview.native_session_id.as_deref(),
        Some(backend.id.as_str())
    );
    let confirmation = BackendFixture::confirmation(&request, preview.preview_id.unwrap());
    system_configuration_absent().unwrap();
    let result = backend.confirm_and_settle(confirmation.clone());
    system_configuration_absent().unwrap();
    let receipt = result.unwrap();
    // 这是任务自建的原生锁，不冒称运行中的模型 writer；原生 RPC 错误正文仍不可推断。
    let original_unchanged =
        fs::read(&backend.rollout).is_ok_and(|bytes| bytes == backend.original);
    let after_hashes: Vec<_> = controls
        .iter()
        .map(|path| {
            fs::read(path)
                .ok()
                .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        })
        .collect();
    let controls_unchanged = after_hashes
        .iter()
        .zip(&before_hashes)
        .all(|(after, before)| after.as_ref() == Some(before));
    let lock_contended_after = writer_guard.contended().unwrap();
    let mut report = serde_json::json!({
        "stage":"product-backend-held-native-writer-lock","version":CODEX_VERSION,"requestedNativeId":backend.id,
        "syntheticWriterLockOnly":true,"runningModelWriter":false,"nativeRpcErrorProven":false,"associatedCleanupUnverified":true,
        "lockContendedBefore":lock_contended_before,"lockContendedAfter":lock_contended_after,
        "receipt":receipt,"originalUnchanged":original_unchanged,"controlsUnchanged":controls_unchanged,
        "originalBytesSha256":format!("{:x}",Sha256::digest(&backend.original)),"controlHashesBefore":before_hashes,
        "controlHashesAfter":after_hashes,"nativeDescendants":"unverified","ownerSettled":true,"unlockedRetryPerformed":false
    });
    fs::write(
        fixture.root.join("reports/writer-lock-result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NATIVE_DELETE_LIVE_WRITER_LOCK {report}");
    // 本入口未单独保留 pending→inspect 路径，初始结果只接受已派发后的直接 unknown。
    assert_eq!(
        (receipt.state, receipt.reason, receipt.root_present),
        ("unknown", Some("DELETE_OUTCOME_UNKNOWN"), Some(true))
    );
    assert!(original_unchanged && controls_unchanged && lock_contended_after);
    let duplicate = backend.confirm_and_settle(confirmation.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&duplicate).unwrap(),
        serde_json::to_value(&receipt).unwrap()
    );
    let inspected = backend
        .service
        .inspect_delete(
            &backend.caller,
            InspectRequest {
                operation_id: request.operation_id,
            },
        )
        .unwrap();
    assert_eq!(
        (inspected.state, inspected.reason, inspected.root_present),
        ("partial", Some("DELETE_NOT_ACKNOWLEDGED"), Some(true))
    );
    let duplicate_after_inspect = backend.confirm_and_settle(confirmation).unwrap();
    assert_eq!(
        serde_json::to_value(&duplicate_after_inspect).unwrap(),
        serde_json::to_value(&inspected).unwrap()
    );
    backend.preserved();
    assert!(writer_guard.contended().unwrap());
    system_configuration_absent().unwrap();
    report["duplicateReusedReceipt"] = true.into();
    report["inspectReceipt"] = serde_json::to_value(&inspected).unwrap();
    report["afterInspectDuplicateReusedReceipt"] = true.into();
    fs::write(
        fixture.root.join("reports/writer-lock-result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("NATIVE_DELETE_LIVE_WRITER_LOCK_FINAL {report}");
    // 所有调用结束后才释放锁；不对这个结果不确定的目标解锁重发。
    drop(writer_guard);
}
