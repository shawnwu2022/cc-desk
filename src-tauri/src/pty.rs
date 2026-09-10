//! PTY 管理模块
//! 基于 portable-pty 实现 Claude CLI 进程管理

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use portable_pty::{
    native_pty_system, Child, ChildKiller, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize,
};
use std::collections::HashMap;
use std::env;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{mpsc, Arc, LazyLock, Weak};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

// Windows ConPTY 对连续突发写入存在截断风险；小块刷新并短暂让出时间，避免大粘贴丢字节。
pub(crate) const PTY_WRITE_CHUNK_SIZE: usize = 4 * 1024;
const PTY_WRITE_CHUNK_DELAY: Duration = Duration::from_millis(1);
const PTY_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

pub(crate) fn write_pty_data<W: Write + ?Sized>(
    writer: &mut W,
    data: &[u8],
) -> std::io::Result<()> {
    let mut chunks = data.chunks(PTY_WRITE_CHUNK_SIZE).peekable();
    if chunks.peek().is_none() {
        return writer.flush();
    }

    while let Some(chunk) = chunks.next() {
        writer.write_all(chunk)?;
        writer.flush()?;
        if chunks.peek().is_some() {
            thread::sleep(PTY_WRITE_CHUNK_DELAY);
        }
    }
    Ok(())
}

pub(crate) fn validate_pty_id(id: &str) -> std::result::Result<(), String> {
    Uuid::parse_str(id)
        .map(|_| ())
        .map_err(|error| format!("invalid PTY id '{id}': {error}"))
}

/// PTY slave 关闭后，不同平台可能返回 EOF、BrokenPipe，或 Unix 的 EIO。
/// 这些都表示输出流已结束，应由 child waiter 上报真实退出状态。
pub(crate) fn is_pty_stream_end(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::BrokenPipe
    ) {
        return true;
    }

    #[cfg(unix)]
    {
        // EIO 在 Linux/macOS/BSD 上的 errno 均为 5；PTY master 在 slave 关闭时常返回它。
        if error.raw_os_error() == Some(5) {
            return true;
        }
    }

    false
}

/// PTY 实例信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct PtyInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub pty_type: String,
    pub cwd: String,
}

/// PTY 输出事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct PtyOutputPayload {
    pub id: String,
    pub data: String,
}

/// PTY 退出事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct PtyExitPayload {
    pub id: String,
    pub exit_code: i32,
    pub signal: Option<String>,
}

pub(crate) fn exit_payload(id: &str, status: &ExitStatus) -> PtyExitPayload {
    PtyExitPayload {
        id: id.to_string(),
        exit_code: i32::try_from(status.exit_code()).unwrap_or(i32::MAX),
        // portable-pty 0.8.x exposes a portable exit code but no public
        // signal accessor. Do not infer a signal from formatted text.
        signal: None,
    }
}

/// PTY 错误事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct PtyErrorPayload {
    pub id: String,
    pub error: String,
    pub stage: String,
}

/// 只保存主端和可独立发送终止信号的句柄。
/// Child 本体由专用 waiter 线程持有并负责 reap。
struct PtyInstanceData {
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

/// 每个 PTY 独立的 writer 锁。全局 registry 锁只用于 O(1) 查找，
/// 不在阻塞 write/flush 或分块等待期间持有。
pub(crate) struct PtyWriterEntry {
    writer: Mutex<Box<dyn Write + Send>>,
}

impl PtyWriterEntry {
    pub(crate) fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: Mutex::new(writer),
        }
    }
}

pub(crate) type PtyWriterRegistry = Mutex<HashMap<String, Arc<PtyWriterEntry>>>;

pub(crate) fn lookup_writer(writers: &PtyWriterRegistry, id: &str) -> Option<Arc<PtyWriterEntry>> {
    writers.lock().get(id).cloned()
}

/// PTY 管理器（全局）
pub struct PtyManager {
    instances: Mutex<HashMap<String, PtyInstanceData>>,
    writers: PtyWriterRegistry,
    app_handle: AppHandle,
}

impl PtyManager {
    /// 创建 PTY 管理器
    pub fn new(app_handle: AppHandle) -> Self {
        log::info!("PTY Manager initialized");
        Self {
            instances: Mutex::new(HashMap::new()),
            writers: Mutex::new(HashMap::new()),
            app_handle,
        }
    }

    fn validate_spawn_request(&self, id: &str, cwd: &str) -> Result<()> {
        validate_pty_id(id).map_err(anyhow::Error::msg)?;

        if !Path::new(cwd).is_dir() {
            return Err(anyhow!("Working directory does not exist: {cwd}"));
        }

        if self.instances.lock().contains_key(id) || self.writers.lock().contains_key(id) {
            return Err(anyhow!("PTY id already registered: {id}"));
        }

        Ok(())
    }

    fn remove_registration(&self, id: &str) -> Option<PtyInstanceData> {
        self.writers.lock().remove(id);
        self.instances.lock().remove(id)
    }

    #[allow(clippy::too_many_arguments)]
    fn register_process(
        self: &Arc<Self>,
        id: String,
        master: Box<dyn MasterPty + Send>,
        child: Box<dyn Child + Send + Sync>,
        writer: Box<dyn Write + Send>,
        reader: Box<dyn Read + Send>,
        reader_label: &'static str,
    ) -> Result<()> {
        let killer = child.clone_killer();
        self.instances
            .lock()
            .insert(id.clone(), PtyInstanceData { master, killer });
        self.writers
            .lock()
            .insert(id.clone(), Arc::new(PtyWriterEntry::new(writer)));

        let (reader_done_tx, reader_done_rx) = mpsc::sync_channel(1);
        let reader_id = id.clone();
        let reader_app = self.app_handle.clone();
        let reader_manager: Weak<Self> = Arc::downgrade(self);
        let reader_thread = thread::Builder::new()
            .name(format!(
                "pty-reader-{}",
                &reader_id[..8.min(reader_id.len())]
            ))
            .spawn(move || {
                log::debug!("[{}] {} reader thread started", reader_id, reader_label);
                let failure = Self::read_output_loop(reader_id.clone(), reader, reader_app);
                if let Some(error) = failure {
                    if let Some(manager) = reader_manager.upgrade() {
                        manager.fail_reader(&reader_id, &error);
                    }
                }
                let _ = reader_done_tx.send(());
            });

        if let Err(error) = reader_thread {
            self.remove_registration(&id);
            let mut child = child;
            Self::terminate_unregistered_child(&mut child);
            return Err(anyhow!(
                "Failed to spawn reader thread for PTY {id}: {error}"
            ));
        }

        // 使用 slot 的原因：若线程创建失败，Child 仍可在当前线程 kill + wait，
        // 不会因为 closure 被丢弃而留下无人 reap 的子进程。
        let child_slot = Arc::new(Mutex::new(Some(child)));
        let waiter_child = child_slot.clone();
        let waiter_id = id.clone();
        let waiter_manager: Weak<Self> = Arc::downgrade(self);
        let waiter_thread = thread::Builder::new()
            .name(format!(
                "pty-waiter-{}",
                &waiter_id[..8.min(waiter_id.len())]
            ))
            .spawn(move || {
                let mut child = waiter_child
                    .lock()
                    .take()
                    .expect("PTY child slot already consumed");
                let status = child.wait();

                // 保证 reader 已经把最终输出 emit 后，再发送 pty-exit。
                // 若子孙进程错误地继续持有 slave，超时后清理 master 以收敛。
                let _ = reader_done_rx.recv_timeout(PTY_OUTPUT_DRAIN_TIMEOUT);

                if let Some(manager) = waiter_manager.upgrade() {
                    manager.finish_natural_exit(&waiter_id, status);
                }
            });

        if let Err(error) = waiter_thread {
            self.remove_registration(&id);
            if let Some(mut child) = child_slot.lock().take() {
                Self::terminate_unregistered_child(&mut child);
            }
            return Err(anyhow!(
                "Failed to spawn waiter thread for PTY {id}: {error}"
            ));
        }

        Ok(())
    }

    fn terminate_unregistered_child(child: &mut Box<dyn Child + Send + Sync>) {
        let _ = child.kill();
        let _ = child.wait();
    }

    fn finish_natural_exit(&self, id: &str, status: std::io::Result<ExitStatus>) {
        if self.remove_registration(id).is_none() {
            // 显式 kill/read failure 已先移除并发送事件；避免重复 pty-exit。
            return;
        }

        let payload = match status {
            Ok(status) => exit_payload(id, &status),
            Err(error) => PtyExitPayload {
                id: id.to_string(),
                exit_code: -1,
                signal: Some(format!("wait_error: {error}")),
            },
        };

        if let Err(error) = self.app_handle.emit("pty-exit", payload) {
            log::warn!("[{}] Failed to emit natural exit: {}", id, error);
        }
    }

    fn fail_reader(&self, id: &str, error: &str) {
        let Some(mut instance) = self.remove_registration(id) else {
            return;
        };

        let _ = instance.killer.kill();
        let payload = PtyExitPayload {
            id: id.to_string(),
            exit_code: -1,
            signal: Some(error.to_string()),
        };
        if let Err(emit_error) = self.app_handle.emit("pty-exit", payload) {
            log::warn!("[{}] Failed to emit reader failure: {}", id, emit_error);
        }
    }

    /// 检测 Git Bash 路径（Windows）
    fn detect_git_bash() -> Option<String> {
        if !cfg!(target_os = "windows") {
            return None;
        }

        log::debug!("Detecting Git Bash on Windows...");

        // 1. 配置文件
        if let Ok(config) = crate::store::get_app_config() {
            if let Some(ref path) = config.git_bash_path {
                if Path::new(path).exists() {
                    log::info!("Git Bash path from config: {}", path);
                    return Some(path.clone());
                }
            }
        }

        // 2. 环境变量
        if let Ok(path) = env::var("CLAUDE_CODE_GIT_BASH_PATH") {
            if Path::new(&path).exists() {
                log::info!("Git Bash found via env var: {}", path);
                return Some(path);
            }
        }

        // 3. where git → 同目录下找 bash.exe
        if let Some(git_path) = crate::platform::find_executable("git") {
            if let Some(parent) = Path::new(&git_path).parent() {
                let bash_path = parent.join("bash.exe");
                if bash_path.exists() {
                    log::info!("Git Bash found via 'where git': {}", bash_path.display());
                    return Some(bash_path.to_string_lossy().to_string());
                }
            }
        }

        log::warn!("Git Bash not found - Claude CLI may not work properly on Windows");
        None
    }

    /// 获取 Claude CLI 路径（优先使用配置，其次自动检测）
    fn get_claude_path() -> Option<String> {
        if let Ok(config) = crate::store::get_app_config() {
            if let Some(ref path) = config.claude_path {
                if Path::new(path).exists() {
                    log::info!("Claude CLI path from config: {}", path);
                    return Some(path.clone());
                }
            }
        }

        crate::platform::find_executable("claude")
    }

    fn apply_common_environment(cmd: &mut CommandBuilder, truecolor: bool) {
        for (key, value) in env::vars() {
            cmd.env(key, value);
        }

        cmd.env("TERM", "xterm-256color");
        if truecolor {
            cmd.env("COLORTERM", "truecolor");
        }

        if let Ok(config) = crate::store::get_app_config() {
            if let Some(env_vars) = config.claude_env_vars {
                for (key, value) in &env_vars {
                    cmd.env(key, value);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_command(
        self: &Arc<Self>,
        id: String,
        cwd: &str,
        cols: u16,
        rows: u16,
        pty_type: &str,
        cmd: CommandBuilder,
        description: &str,
    ) -> Result<PtyInfo> {
        let pty_system = native_pty_system();
        let PtyPair { master, slave } = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("Failed to open PTY with size {cols}x{rows}"))?;

        let mut child = match slave.spawn_command(cmd) {
            Ok(child) => child,
            Err(error) => {
                let message = format!("Failed to spawn {description}: {error}");
                self.emit_error(&id, &message, "spawn");
                return Err(anyhow!(message));
            }
        };

        // 父进程绝不能继续持有 slave；否则 Unix master 可能永远收不到 EOF。
        drop(slave);

        let writer = match master.take_writer() {
            Ok(writer) => writer,
            Err(error) => {
                let message = format!("Failed to take PTY writer: {error}");
                self.emit_error(&id, &message, "writer");
                Self::terminate_unregistered_child(&mut child);
                return Err(anyhow!(message));
            }
        };

        let reader = match master.try_clone_reader() {
            Ok(reader) => reader,
            Err(error) => {
                let message = format!("Failed to clone PTY reader: {error}");
                self.emit_error(&id, &message, "reader");
                Self::terminate_unregistered_child(&mut child);
                return Err(anyhow!(message));
            }
        };

        self.register_process(
            id.clone(),
            master,
            child,
            writer,
            reader,
            if pty_type == "claude" {
                "Claude output"
            } else {
                "Shell output"
            },
        )?;

        Ok(PtyInfo {
            id,
            pty_type: pty_type.to_string(),
            cwd: cwd.to_string(),
        })
    }

    /// 启动 Claude CLI（通过 shell 执行，模拟终端行为）
    pub fn spawn_claude(
        self: &Arc<Self>,
        id: String,
        cwd: &str,
        cols: u16,
        rows: u16,
        args: Option<Vec<String>>,
    ) -> Result<PtyInfo> {
        self.validate_spawn_request(&id, cwd).inspect_err(|error| {
            self.emit_error(&id, &error.to_string(), "validation");
        })?;

        log::info!(
            "Spawning Claude CLI via shell with id={}, cwd={}, size={}x{}",
            id,
            cwd,
            cols,
            rows
        );

        if Self::get_claude_path().is_none() {
            let message = "Claude CLI not found. Please install Claude Code using its official installation method";
            self.emit_error(&id, message, "detection");
            return Err(anyhow!(message));
        }

        // 参数边界将在 platform 层统一转义；此处暂保持既有 shell 启动策略。
        let claude_cmd = if let Ok(config) = crate::store::get_app_config() {
            if let Some(ref custom_path) = config.claude_path {
                if let Some(extra_args) = &args {
                    format!("\"{}\" {}", custom_path, extra_args.join(" "))
                } else {
                    format!("\"{}\"", custom_path)
                }
            } else if let Some(extra_args) = &args {
                format!("claude {}", extra_args.join(" "))
            } else {
                "claude".to_string()
            }
        } else if let Some(extra_args) = &args {
            format!("claude {}", extra_args.join(" "))
        } else {
            "claude".to_string()
        };

        let plugin_dir = crate::hook_config::plugin_dir();
        let claude_cmd = if plugin_dir.exists() {
            format!("{} --plugin-dir \"{}\"", claude_cmd, plugin_dir.display())
        } else {
            claude_cmd
        };

        let git_bash = if cfg!(target_os = "windows") {
            Self::detect_git_bash()
        } else {
            None
        };
        let (program, shell_args) =
            crate::platform::get_claude_shell(&claude_cmd, git_bash.as_deref());

        let mut cmd = CommandBuilder::new(&program);
        for arg in &shell_args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        Self::apply_common_environment(&mut cmd, true);

        if let Some(hook_port) = crate::hook_server::get_port() {
            cmd.env("CC_BOX_HOOK_PORT", hook_port.to_string());
            cmd.env("CC_BOX_SESSION_ID", &id);
        }

        if cfg!(target_os = "windows") {
            if let Some(git_bash) = Self::detect_git_bash() {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", git_bash);
            }
        }

        log::debug!("Shell command: {:?}", claude_cmd);
        self.spawn_command(id, cwd, cols, rows, "claude", cmd, "Claude shell command")
    }

    /// 启动普通 Shell
    pub fn spawn_shell(
        self: &Arc<Self>,
        id: String,
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> Result<PtyInfo> {
        self.validate_spawn_request(&id, cwd).inspect_err(|error| {
            self.emit_error(&id, &error.to_string(), "validation");
        })?;

        log::info!(
            "Spawning shell with id={}, cwd={}, size={}x{}",
            id,
            cwd,
            cols,
            rows
        );

        let (program, shell_args) = crate::platform::get_default_shell();
        let mut cmd = CommandBuilder::new(program);
        for arg in &shell_args {
            cmd.arg(*arg);
        }
        cmd.cwd(cwd);
        Self::apply_common_environment(&mut cmd, false);

        self.spawn_command(
            id,
            cwd,
            cols,
            rows,
            "shell",
            cmd,
            &format!("shell '{program}'"),
        )
    }

    /// 输出读取循环。退出事件由 child waiter 统一发送，保证真实 exit code 与 exactly-once。
    fn read_output_loop(
        pty_id: String,
        mut reader: Box<dyn Read + Send>,
        app_handle: AppHandle,
    ) -> Option<String> {
        let mut buf = [0u8; 4096];
        let mut decoder = crate::pty_decoder::PtyDecoder::new();
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: i32 = 5;

        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let output = decoder.flush();
                    if !output.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutputPayload {
                                id: pty_id.clone(),
                                data: output,
                            },
                        );
                    }
                    log::info!("[{}] PTY EOF reached", pty_id);
                    return None;
                }
                Ok(count) => {
                    consecutive_errors = 0;
                    let output = decoder.decode(&buf[..count]);
                    if !output.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutputPayload {
                                id: pty_id.clone(),
                                data: output,
                            },
                        );
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) if is_pty_stream_end(&error) => {
                    let output = decoder.flush();
                    if !output.is_empty() {
                        let _ = app_handle.emit(
                            "pty-output",
                            PtyOutputPayload {
                                id: pty_id.clone(),
                                data: output,
                            },
                        );
                    }
                    log::debug!("[{}] PTY stream ended: {}", pty_id, error);
                    return None;
                }
                Err(error) => {
                    consecutive_errors += 1;
                    log::warn!(
                        "[{}] Read error ({}/{}): {}",
                        pty_id,
                        consecutive_errors,
                        MAX_CONSECUTIVE_ERRORS,
                        error
                    );

                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        return Some(format!("read_error: {error}"));
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    fn emit_error(&self, id: &str, error: &str, stage: &str) {
        let _ = self.app_handle.emit(
            "pty-error",
            PtyErrorPayload {
                id: id.to_string(),
                error: error.to_string(),
                stage: stage.to_string(),
            },
        );
    }

    /// 写入输入到 PTY。全局 map 锁在取得 Arc 后立即释放。
    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        let entry = lookup_writer(&self.writers, id).ok_or_else(|| {
            let error = format!("PTY writer not found for id: {id}");
            log::warn!("{}", error);
            anyhow!(error)
        })?;

        let mut writer = entry.writer.lock();
        write_pty_data(writer.as_mut(), data.as_bytes()).with_context(|| {
            format!("Failed to write or flush {} bytes to PTY {id}", data.len())
        })?;
        Ok(())
    }

    /// 调整 PTY 大小
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let instances = self.instances.lock();
        let instance = instances
            .get(id)
            .ok_or_else(|| anyhow!("PTY instance not found for id: {id}"))?;

        instance
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("Failed to resize PTY {id} to {cols}x{rows}"))
    }

    /// 杀掉单个 PTY。仅在发送 kill 时短暂持有实例 map 锁，不等待 child。
    /// 专用 waiter 线程负责真正 reap；实例先移除以抑制其 natural-exit 重复事件。
    pub fn kill(&self, id: &str) -> Result<()> {
        let removed = {
            let mut instances = self.instances.lock();
            let Some(instance) = instances.get_mut(id) else {
                log::warn!("[{}] PTY not found when trying to kill", id);
                return Ok(());
            };
            instance
                .killer
                .kill()
                .with_context(|| format!("Failed to kill child process {id}"))?;
            instances.remove(id)
        };

        if removed.is_some() {
            self.writers.lock().remove(id);
            let _ = self.app_handle.emit(
                "pty-exit",
                PtyExitPayload {
                    id: id.to_string(),
                    exit_code: 137,
                    signal: Some("killed".to_string()),
                },
            );
        }
        Ok(())
    }

    /// 杀掉所有 PTY。先释放全局 maps，再逐个发信号；不与阻塞 writer 形成锁环。
    pub fn kill_all(&self) {
        let instances: Vec<(String, PtyInstanceData)> = self.instances.lock().drain().collect();
        self.writers.lock().clear();

        for (id, mut instance) in instances {
            if let Err(error) = instance.killer.kill() {
                log::warn!("[{}] Failed to kill child: {}", id, error);
            }
        }
    }
}

/// 全局 PTY 管理器存储
static PTY_MANAGER: LazyLock<Mutex<Option<Arc<PtyManager>>>> = LazyLock::new(|| Mutex::new(None));

/// 初始化 PTY 管理器
pub fn init_pty_manager(app_handle: AppHandle) {
    let manager = Arc::new(PtyManager::new(app_handle));
    *PTY_MANAGER.lock() = Some(manager);
    log::info!("PTY manager initialized");
}

/// 获取 PTY 管理器
pub fn get_pty_manager() -> Option<Arc<PtyManager>> {
    PTY_MANAGER.lock().clone()
}
