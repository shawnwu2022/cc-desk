use super::*;
use crate::cli::environment::{lookup, EnvMap};
use crate::version_history::windows::files::{ComponentName, Directory, FileAccess, PinnedFile};
use std::{
    ffi::OsStr,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
const SOURCE_LIMIT: u64 = 1024 * 1024 * 1024;
fn unavailable<T>(_: T) -> SafeError {
    error("ARCHIVE_SOURCE_UNAVAILABLE")
}
fn local_path(path: &Path) -> Result<PathBuf, SafeError> {
    let text = path
        .to_str()
        .ok_or_else(|| error("OPENCODE_ARCHIVE_UNSUPPORTED"))?;
    let text = text.strip_prefix(r"\\?\").unwrap_or(text);
    if text.len() < 3
        || !text.as_bytes()[0].is_ascii_alphabetic()
        || text.as_bytes()[1] != b':'
        || !matches!(text.as_bytes()[2], b'\\' | b'/')
        || text.chars().any(char::is_control)
        || text.contains(['*', '?', ','])
    {
        return Err(error("OPENCODE_ARCHIVE_UNSUPPORTED"));
    }
    Ok(PathBuf::from(text))
}
/// 持有本地 NTFS 祖先、数据库和固定侧车，允许 SQLite 正常写入但禁止换名/替换。
pub(crate) struct PinnedSource {
    directory: Arc<Directory>,
    files: Vec<PinnedFile>,
    pub(crate) paths: Vec<PathBuf>,
}
impl PinnedSource {
    pub(crate) fn open(root: &Path, filename: &str) -> Result<Self, SafeError> {
        let root = local_path(root)?;
        let directory = Directory::open_absolute(&root).map_err(unavailable)?;
        let mut files = Vec::new();
        let mut paths = Vec::new();
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let name = format!("{filename}{suffix}");
            let component = ComponentName::new(OsStr::new(&name)).map_err(unavailable)?;
            let file = directory
                .pin_sqlite_file(component, !suffix.is_empty())
                .map_err(unavailable)?;
            if suffix.is_empty() {
                let header = file.read_prefix(100).map_err(unavailable)?;
                if header.len() != 100
                    || &header[..16] != b"SQLite format 3\0"
                    || header[18] != 2
                    || header[19] != 2
                {
                    return Err(error("OPENCODE_ARCHIVE_UNSUPPORTED"));
                }
            }
            let file = if suffix == "-journal" {
                // WAL 来源不应携带热回滚日志；禁止 SQLite 在结构核验前做回滚恢复。
                // 改用 deny-write 句柄还阻止并发切换到 rollback journal 模式。
                drop(file);
                let journal = directory
                    .open_file(
                        ComponentName::new(OsStr::new(&name)).map_err(unavailable)?,
                        FileAccess::Read,
                    )
                    .map_err(unavailable)?;
                if journal.size().map_err(unavailable)? != 0 {
                    return Err(error("OPENCODE_ARCHIVE_UNSUPPORTED"));
                }
                journal
            } else {
                file
            };
            paths.push(root.join(name));
            files.push(file);
        }
        let result = Self {
            directory,
            files,
            paths,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.directory.recheck().map_err(unavailable)?;
        let mut total = 0u64;
        for file in &self.files {
            file.verify().map_err(unavailable)?;
            total = total
                .checked_add(file.size().map_err(unavailable)?)
                .ok_or_else(|| error("ARCHIVE_SOURCE_UNAVAILABLE"))?;
        }
        if total > SOURCE_LIMIT {
            return Err(error("ARCHIVE_SOURCE_UNAVAILABLE"));
        }
        Ok(())
    }
}
pub(crate) struct Prepared {
    pub(crate) command: Command,
    source: PinnedSource,
    bridge: PinnedFile,
    // 资源先释放，再清理仅本次创建的临时目录。
    _namespace: tempfile::TempDir,
}
impl Prepared {
    pub(crate) fn new(environment: &EnvMap, source: PinnedSource) -> Result<Self, SafeError> {
        let namespace = tempfile::Builder::new()
            .prefix("ccdesk-opencode-archive-")
            .tempdir()
            .map_err(|_| error("SDK_HELPER_UNAVAILABLE"))?;
        let path = local_path(namespace.path())?;
        let directory =
            Directory::open_absolute(&path).map_err(|_| error("SDK_HELPER_UNAVAILABLE"))?;
        let bytes = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/opencode-native-archive/bridge.mjs"
        ));
        std::fs::write(path.join("bridge.mjs"), bytes)
            .map_err(|_| error("SDK_HELPER_UNAVAILABLE"))?;
        let bridge = directory
            .open_file(
                ComponentName::new(OsStr::new("bridge.mjs")).map_err(unavailable)?,
                FileAccess::Read,
            )
            .map_err(|_| error("SDK_HELPER_UNAVAILABLE"))?;
        if bridge.read_prefix(bytes.len() + 1).map_err(unavailable)? != bytes {
            return Err(error("SDK_HELPER_UNAVAILABLE"));
        }
        let mut command = Command::new(crate::cli::native_rename::claude::node_program(
            environment,
        )?);
        command.arg("--permission").arg(format!(
            "--allow-fs-read={}",
            path.join("bridge.mjs").display()
        ));
        for file in &source.paths {
            let file = local_path(file)?;
            command
                .arg(format!("--allow-fs-read={}", file.display()))
                .arg(format!("--allow-fs-write={}", file.display()));
        }
        // 不继承 NODE_OPTIONS、OpenCode 配置/凭据或任意模块搜索路径。
        command.arg("--input-type=module").arg("-e").arg(r#"const [a,b]=process.versions.node.split('.').map(Number);if(a<24||(a===24&&b<18)||!process.permission){process.stdout.write('{"state":"failed","code":"NODE_RUNTIME_UNAVAILABLE"}\n');process.exit(1)}await import((await import('node:url')).pathToFileURL(process.argv[1]).href);"#).arg(path.join("bridge.mjs"));
        command.env_clear().current_dir(&path);
        for key in [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "TEMP",
            "TMP",
        ] {
            command.env(key, &path);
        }
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = lookup(environment, OsStr::new(key)) {
                command.env(key, value);
            }
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x4);
        Ok(Self {
            command,
            source,
            bridge,
            _namespace: namespace,
        })
    }
    pub(crate) fn process(
        &self,
        child: &mut Child,
        native_id: &str,
        cwd: &str,
        archived: bool,
        current: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<(), SafeError> {
        let mut input = child
            .stdin
            .take()
            .ok_or_else(|| error("SDK_HELPER_UNAVAILABLE"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| error("SDK_HELPER_UNAVAILABLE"))?;
        let (sender, receiver) = std::sync::mpsc::sync_channel(2);
        std::thread::Builder::new()
            .name("opencode-archive-output".into())
            .spawn(move || {
                let mut reader = std::io::BufReader::new(output);
                for _ in 0..2 {
                    let mut line = Vec::new();
                    if !matches!(
                        reader.by_ref().take(32769).read_until(b'\n', &mut line),
                        Ok(1..)
                    ) || line.len() > 32768
                    {
                        break;
                    }
                    let Ok(value) = serde_json::from_slice::<Response>(&line) else {
                        break;
                    };
                    if sender.send(value).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| error("SDK_HELPER_UNAVAILABLE"))?;
        let deadline = Instant::now() + Duration::from_secs(30);
        let receive = || loop {
            current()?;
            self.source.verify()?;
            self.bridge.verify().map_err(unavailable)?;
            if Instant::now() >= deadline {
                return Err(error("SDK_HELPER_UNAVAILABLE"));
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(value) => return Ok(value),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(error("SDK_HELPER_UNAVAILABLE")),
            }
        };
        match receive()? {
            Response::Ready => {}
            Response::Failed { code } if code == "NODE_RUNTIME_UNAVAILABLE" => {
                return Err(error("NODE_RUNTIME_UNAVAILABLE"))
            }
            _ => return Err(error("SDK_HELPER_UNAVAILABLE")),
        }
        self.source.verify()?;
        let payload = serde_json::json!({"databasePath":self.source.paths[0],"nativeSessionId":native_id,"expectedCwd":cwd,"archived":archived});
        // 最后的可阻塞来源检查之后再判断授权/期限，避免超时后才首次派发。
        current()?;
        // 此后断连、失权、超时或不匹配回执都不是确定失败，不自动重放。
        serde_json::to_writer(&mut input, &payload)
            .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
        input
            .flush()
            .map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))?;
        drop(input);
        match receive().map_err(|_| error("ARCHIVE_OUTCOME_UNKNOWN"))? {
            Response::Archived {
                native_session_id,
                archived: actual,
            } if native_session_id == native_id && actual == archived => Ok(()),
            Response::Failed { code }
                if matches!(
                    code.as_str(),
                    "ARCHIVE_SCHEMA_UNSUPPORTED" | "ARCHIVE_SOURCE_UNAVAILABLE"
                ) =>
            {
                Err(error(if code == "ARCHIVE_SCHEMA_UNSUPPORTED" {
                    "ARCHIVE_SCHEMA_UNSUPPORTED"
                } else {
                    "ARCHIVE_SOURCE_UNAVAILABLE"
                }))
            }
            _ => Err(error("ARCHIVE_OUTCOME_UNKNOWN")),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "lowercase", deny_unknown_fields)]
enum Response {
    Ready,
    Failed {
        code: String,
    },
    Archived {
        #[serde(rename = "nativeSessionId")]
        native_session_id: String,
        archived: bool,
    },
}
