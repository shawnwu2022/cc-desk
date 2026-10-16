//! PTY 管理模块
//! 基于 portable-pty 实现 Claude CLI 进程管理

use crate::platform::admitted_child::AdmittedChild;
use crate::platform::legacy_control::LegacyProcessControl;
use crate::platform::{shell_quote_arg, ShellKind};
use crate::version_history::maintenance::{AdmissionGate, PreparingStart, RuntimeKind};
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, ExitStatus, MasterPty, PtyPair, PtySize};
use std::collections::HashMap;
use std::env;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{mpsc, Arc, LazyLock, Weak};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

// 普通输入保留既有分块策略；Windows 完整粘贴帧走下方独立编码与真实管道排空。
pub(crate) const PTY_WRITE_CHUNK_SIZE: usize = 4 * 1024;
const PTY_WRITE_CHUNK_DELAY: Duration = Duration::from_millis(1);
pub(crate) const PTY_OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// Match Windows Terminal paste semantics on Windows: submit the complete,
/// unmodified bracketed-paste frame through one logical pipe write.
///
/// Earlier CC Desk builds rewrote ESC into Win32 INPUT_RECORD encodings and
/// inserted FlushFileBuffers boundaries. That protocol is only valid after
/// win32-input-mode negotiation and leaked `[201~` on Windows 10 build 19045.
/// Claude Code already enables the input mode it requires; the terminal host
/// must preserve the frame instead of inventing a second keyboard protocol.
#[cfg(windows)]
fn write_conpty_paste<W: Write + ?Sized>(writer: &mut W, data: &[u8]) -> io::Result<()> {
    writer.write_all(data)
}

pub(crate) fn write_pty_data<W: Write + ?Sized>(
    writer: &mut W,
    data: &[u8],
) -> std::io::Result<()> {
    #[cfg(windows)]
    if data.starts_with(b"\x1b[200~") && data.ends_with(b"\x1b[201~") {
        return write_conpty_paste(writer, data);
    }

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
#[serde(rename_all = "camelCase")]
pub struct PtyInfo {
    pub id: String,
    #[serde(rename = "type")]
    pub pty_type: String,
    pub cwd: String,
    /// 本次启动冻结的根身份证明：配置未重定向子进程 home/Claude 根，且所选
    /// shell 启动路径不会再执行可改写该变量的用户启动脚本。
    pub uses_default_claude_root: bool,
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
    killer: Arc<Mutex<LegacyProcessControl>>,
    _observer: Option<crate::observer_registry::ObserverLease>,
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
    admission: AdmissionGate,
}

impl PtyManager {
    /// 创建 PTY 管理器
    pub(crate) fn new(app_handle: AppHandle, admission: AdmissionGate) -> Self {
        log::info!("PTY Manager initialized");
        Self {
            instances: Mutex::new(HashMap::new()),
            writers: Mutex::new(HashMap::new()),
            app_handle,
            admission,
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

    fn remove_owned_registration(
        &self,
        id: &str,
        owner: &Arc<Mutex<LegacyProcessControl>>,
    ) -> Option<PtyInstanceData> {
        let mut instances = self.instances.lock();
        if !instances
            .get(id)
            .is_some_and(|instance| Arc::ptr_eq(&instance.killer, owner))
        {
            return None;
        }
        self.writers.lock().remove(id);
        instances.remove(id)
    }

    #[allow(clippy::too_many_arguments)]
    fn register_process(
        self: &Arc<Self>,
        id: String,
        master: Box<dyn MasterPty + Send>,
        mut child: AdmittedChild,
        writer: Box<dyn Write + Send>,
        reader: Box<dyn Read + Send>,
        reader_label: &'static str,
        observer: Option<crate::observer_registry::ObserverLease>,
    ) -> Result<()> {
        let killer = match LegacyProcessControl::capture(&child) {
            Ok(control) => Arc::new(Mutex::new(control)),
            Err(failure) => {
                let _ = Self::terminate_unregistered_child(&mut child);
                return Err(failure).context("Failed to retain exact child control");
            }
        };
        self.instances.lock().insert(
            id.clone(),
            PtyInstanceData {
                master,
                killer: killer.clone(),
                _observer: observer,
            },
        );
        self.writers
            .lock()
            .insert(id.clone(), Arc::new(PtyWriterEntry::new(writer)));

        let (reader_done_tx, reader_done_rx) = mpsc::sync_channel(1);
        let reader_id = id.clone();
        let reader_app = self.app_handle.clone();
        let reader_manager: Weak<Self> = Arc::downgrade(self);
        let reader_owner = killer.clone();
        let reader_thread = spawn_pty_thread(
            format!("pty-reader-{}", &reader_id[..8.min(reader_id.len())]),
            move || {
                log::debug!("[{}] {} reader thread started", reader_id, reader_label);
                let failure = Self::read_output_loop(reader_id.clone(), reader, reader_app);
                if let Some(error) = failure {
                    if let Some(manager) = reader_manager.upgrade() {
                        manager.fail_reader(&reader_id, &reader_owner, &error);
                    }
                }
                let _ = reader_done_tx.send(());
            },
        );

        if let Err(error) = reader_thread {
            self.remove_owned_registration(&id, &killer);
            let mut child = child;
            let _ = Self::terminate_unregistered_child(&mut child);
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
        let waiter_owner = killer.clone();
        let waiter_thread = spawn_pty_thread(
            format!("pty-waiter-{}", &waiter_id[..8.min(waiter_id.len())]),
            move || {
                let mut child = waiter_child
                    .lock()
                    .take()
                    .expect("PTY child slot already consumed");
                // Settles the exact child ticket before output drain and even
                // when the manager/registrations have already been dropped.
                let status = child.wait();

                // 保证 reader 已经把最终输出 emit 后，再发送 pty-exit。
                // 若子孙进程错误地继续持有 slave，超时后清理 master 以收敛。
                let _ = reader_done_rx.recv_timeout(PTY_OUTPUT_DRAIN_TIMEOUT);

                if let Some(manager) = waiter_manager.upgrade() {
                    manager.finish_natural_exit(&waiter_id, &waiter_owner, status);
                }
            },
        );

        if let Err(error) = waiter_thread {
            self.remove_owned_registration(&id, &killer);
            if let Some(mut child) = child_slot.lock().take() {
                let _ = Self::terminate_unregistered_child(&mut child);
            }
            return Err(anyhow!(
                "Failed to spawn waiter thread for PTY {id}: {error}"
            ));
        }

        Ok(())
    }

    fn terminate_unregistered_child(child: &mut AdmittedChild) -> std::io::Result<ExitStatus> {
        let _ = child.kill();
        child.wait()
    }

    fn finish_natural_exit(
        &self,
        id: &str,
        owner: &Arc<Mutex<LegacyProcessControl>>,
        status: std::io::Result<ExitStatus>,
    ) {
        if !self
            .instances
            .lock()
            .get(id)
            .is_some_and(|instance| Arc::ptr_eq(&instance.killer, owner))
        {
            return;
        }
        let status = match status {
            Ok(status) => status,
            Err(failure) => {
                // A failed waiter is not process-exit evidence. Keep the exact
                // control owner available for a later explicit stop, and never
                // manufacture a stopped row from a wait error.
                self.emit_error(id, &failure.to_string(), "wait");
                return;
            }
        };
        if self.remove_owned_registration(id, owner).is_none() {
            // 显式 kill/read failure 已先移除并发送事件；避免重复 pty-exit。
            return;
        }

        let payload = exit_payload(id, &status);

        if let Err(error) = self.app_handle.emit("pty-exit", payload) {
            log::warn!("[{}] Failed to emit natural exit: {}", id, error);
        }
    }

    fn fail_reader(&self, id: &str, owner: &Arc<Mutex<LegacyProcessControl>>, error: &str) {
        if !self
            .instances
            .lock()
            .get(id)
            .is_some_and(|instance| Arc::ptr_eq(&instance.killer, owner))
        {
            return;
        }
        if let Err(failure) = self.kill_owned(id, owner) {
            // Read failure does not imply child exit. A failed stop keeps its
            // registration and retained control; no `pty-exit` may authorize a
            // frontend close while that exact process remains unconfirmed.
            self.emit_error(id, &failure.to_string(), "reader-stop");
            return;
        }
        self.emit_error(id, error, "reader");
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

    /// 写入有效环境，并返回**同一份快照**推导出的环境前置证明：继承进程环境和
    /// 配置 env 均未把 Claude 根重定向。最终根证明还必须核对所选 shell 是否会执行
    /// 当前进程无法检查的用户启动脚本。
    ///
    /// 出现 `BASH_ENV` 时返回 false（根未知）：Windows 的 Git Bash 启动路径使用
    /// `bash -c`，会执行该脚本，脚本可以自行设置 `CLAUDE_CONFIG_DIR`；父进程环境里
    /// 看不到这个改写，因此不能声称本次终端跑在默认根上。这与
    /// `version_history/windows/scope.rs` 把 `BASH_ENV` 视为根未知的既有判定一致。
    fn apply_common_environment(cmd: &mut CommandBuilder, truecolor: bool) -> bool {
        let mut inherited_redirect = false;
        let mut bash_env = false;
        for (key, value) in env::vars_os() {
            if key
                .to_string_lossy()
                .eq_ignore_ascii_case("CLAUDE_CONFIG_DIR")
            {
                inherited_redirect = true;
            }
            if key.to_string_lossy().eq_ignore_ascii_case("BASH_ENV") && !value.is_empty() {
                bash_env = true;
            }
            cmd.env(key, value);
        }

        cmd.env("TERM", "xterm-256color");
        if truecolor {
            cmd.env("COLORTERM", "truecolor");
        }

        let mut configured_root_proof = true;
        if let Ok(config) = crate::store::get_app_config() {
            if let Some(env_vars) = config.claude_env_vars {
                configured_root_proof = apply_configured_claude_environment(cmd, &env_vars);
            }
        }
        // Strip only this application's capability namespace, never provider
        // credentials. A fresh legacy Claude lease is applied after this step.
        for name in crate::cli::environment::OBSERVER_ENV_NAMES {
            cmd.env_remove(name);
        }
        !inherited_redirect && !bash_env && configured_root_proof
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_command(
        self: &Arc<Self>,
        mut admission: PreparingStart,
        id: String,
        cwd: &str,
        cols: u16,
        rows: u16,
        pty_type: &str,
        cmd: CommandBuilder,
        description: &str,
        observer: Option<crate::observer_registry::ObserverLease>,
        uses_default_claude_root: bool,
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

        let mut child =
            match AdmittedChild::spawn_native(slave.as_ref(), cmd, admission.begin_creation()) {
                Ok(child) => child,
                Err(error) => {
                    let message = format!("Failed to spawn {description}: {error}");
                    self.emit_error(&id, &message, "spawn");
                    return Err(anyhow!(message));
                }
            };

        // 父进程绝不能继续持有 slave；否则 Unix master 可能永远收不到 EOF。
        drop(slave);

        let writer = match acquire_legacy_writer(master.as_ref()) {
            Ok(writer) => writer,
            Err(error) => {
                let message = format!("Failed to take PTY writer: {error}");
                self.emit_error(&id, &message, "writer");
                let _ = Self::terminate_unregistered_child(&mut child);
                return Err(anyhow!(message));
            }
        };

        let reader = match acquire_legacy_reader(master.as_ref()) {
            Ok(reader) => reader,
            Err(error) => {
                let message = format!("Failed to clone PTY reader: {error}");
                self.emit_error(&id, &message, "reader");
                let _ = Self::terminate_unregistered_child(&mut child);
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
            observer,
        )?;

        Ok(PtyInfo {
            id,
            pty_type: pty_type.to_string(),
            cwd: cwd.to_string(),
            uses_default_claude_root,
        })
    }

    /// 启动 Claude CLI（通过 shell 执行，模拟终端行为）
    pub fn spawn_claude(
        self: &Arc<Self>,
        id: String,
        cwd: &str,
        cols: u16,
        rows: u16,
        resume_session_id: Option<String>,
        args: Option<Vec<String>>,
    ) -> Result<PtyInfo> {
        let admission = self
            .admission
            .begin_start(RuntimeKind::Legacy)
            .map_err(|e| anyhow!(e.code))?
            .preparing();
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

        // Windows 先探测一次 Git Bash：既决定 shell 引用风格，也复用于
        // shell 选择与 CLAUDE_CODE_GIT_BASH_PATH 注入，避免重复探测。
        let git_bash = if cfg!(target_os = "windows") {
            Self::detect_git_bash()
        } else {
            None
        };
        // 无 Git Bash 的 Windows 回退 PowerShell，单引号转义规则不同
        let shell_kind = if cfg!(target_os = "windows") && git_bash.is_none() {
            ShellKind::PowerShell
        } else {
            ShellKind::Posix
        };

        let prepared_args =
            match prepare_legacy_claude_args(resume_session_id.as_deref(), args.as_deref()) {
                Ok(args) => args,
                Err(reason) => {
                    let message = format!("Invalid owned --resume session id: {reason}");
                    self.emit_error(&id, &message, "validation");
                    return Err(anyhow!(message));
                }
            };

        // 可执行路径与每个参数逐一引用为 shell 字面量后交给 shell -c / -Command
        let custom_path = crate::store::get_app_config()
            .ok()
            .and_then(|config| config.claude_path);
        let mut claude_cmd =
            build_claude_command(custom_path.as_deref(), prepared_args.as_deref(), shell_kind);

        // Preserve the legacy Claude observer preference, but never enable an
        // unauthenticated fallback. This lease follows this exact spawned PTY.
        let prepared = crate::hook_server::prepare_legacy(&self.app_handle, &id);
        if let Some(observer) = &prepared {
            claude_cmd = format!(
                "{} --plugin-dir {}",
                claude_cmd,
                shell_quote_arg(&observer.plugin_dir.to_string_lossy(), shell_kind)
            );
        }

        let (program, shell_args) =
            crate::platform::get_claude_shell(&claude_cmd, git_bash.as_deref());

        let mut cmd = CommandBuilder::new(&program);
        for arg in &shell_args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        let environment_proves_default_root = Self::apply_common_environment(&mut cmd, true);
        let uses_default_claude_root =
            legacy_claude_root_is_proven(environment_proves_default_root, git_bash.as_deref());

        if let Some(observer) = &prepared {
            for (name, value) in &observer.environment.values {
                cmd.env(name, value);
            }
        }

        if cfg!(target_os = "windows") {
            if let Some(git_bash) = &git_bash {
                cmd.env("CLAUDE_CODE_GIT_BASH_PATH", git_bash);
            }
        }

        self.spawn_command(
            admission,
            id,
            cwd,
            cols,
            rows,
            "claude",
            cmd,
            "Claude shell command",
            prepared.map(|prepared| prepared.lease),
            uses_default_claude_root,
        )
    }

    /// 启动普通 Shell
    pub fn spawn_shell(
        self: &Arc<Self>,
        id: String,
        cwd: &str,
        cols: u16,
        rows: u16,
    ) -> Result<PtyInfo> {
        let admission = self
            .admission
            .begin_start(RuntimeKind::Legacy)
            .map_err(|e| anyhow!(e.code))?
            .preparing();
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
        let _ = Self::apply_common_environment(&mut cmd, false);

        self.spawn_command(
            admission,
            id,
            cwd,
            cols,
            rows,
            "shell",
            cmd,
            &format!("shell '{program}'"),
            None,
            true,
        )
    }

    /// 输出读取循环。退出事件由 child waiter 统一发送，保证真实 exit code 与 exactly-once。
    fn read_output_loop(
        pty_id: String,
        mut reader: Box<dyn Read + Send>,
        app_handle: AppHandle,
    ) -> Option<String> {
        let mut emit = |output: String| {
            let _ = app_handle.emit(
                "pty-output",
                PtyOutputPayload {
                    id: pty_id.clone(),
                    data: output,
                },
            );
        };
        pump_pty_output(&pty_id, reader.as_mut(), &mut emit)
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
    /// 粘贴诊断只记录来源、尺寸、标记位置和耗时，绝不记录剪贴板正文。
    pub fn write(&self, id: &str, data: &str, source: Option<&str>) -> Result<()> {
        let entry = lookup_writer(&self.writers, id).ok_or_else(|| {
            let error = format!("PTY writer not found for id: {id}");
            log::warn!("{}", error);
            anyhow!(error)
        })?;

        let bytes = data.as_bytes();
        let open = data.find("\x1b[200~");
        let close = data.rfind("\x1b[201~");
        let complete_frame = data.starts_with("\x1b[200~") && data.ends_with("\x1b[201~");
        let source = source
            .unwrap_or("unknown")
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | ':'))
            .take(48)
            .collect::<String>();
        let paste_like = source.contains("paste") || open.is_some() || close.is_some();
        let transport = if cfg!(windows) && complete_frame {
            "windows-conpty-raw-frame"
        } else {
            "generic"
        };
        let short_id = &id[..id.len().min(8)];
        if paste_like {
            log::info!(
                target: "paste_diag",
                "input build={} version={} pty={} source={} utf8_bytes={} chars={} lf={} open_at={:?} close_at={:?} complete={} transport={}",
                env!("CC_DESK_BUILD_SHA"),
                env!("APP_VERSION"),
                short_id,
                source,
                bytes.len(),
                data.chars().count(),
                bytes.iter().filter(|byte| **byte == b'\n').count(),
                open,
                close,
                complete_frame,
                transport
            );
        }

        let started = Instant::now();
        let mut writer = entry.writer.lock();
        let result = write_pty_data(writer.as_mut(), bytes);
        if paste_like {
            log::info!(
                target: "paste_diag",
                "write pty={} source={} success={} elapsed_ms={}",
                short_id,
                source,
                result.is_ok(),
                started.elapsed().as_millis()
            );
        }
        result.with_context(|| {
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

    /// 杀掉精确 PTY。控制和 Windows 退出检查不持有全局 map 锁。
    /// 专用 waiter 负责 reap；自然退出和重复请求均不重复清理。
    pub fn kill(&self, id: &str) -> Result<()> {
        let Some(killer) = self
            .instances
            .lock()
            .get(id)
            .map(|instance| instance.killer.clone())
        else {
            return Ok(());
        };
        self.kill_owned(id, &killer)
    }

    fn kill_owned(&self, id: &str, killer: &Arc<Mutex<LegacyProcessControl>>) -> Result<()> {
        #[cfg(all(test, windows))]
        maintenance_tests::check_control()?;
        killer
            .lock()
            .kill()
            .with_context(|| format!("Failed to kill child process {id}"))?;
        // Natural exit may settle while control waits. Callback ownership,
        // not just the caller-supplied ID, protects a replacement registration.
        let removed = self.remove_owned_registration(id, killer);

        if removed.is_some() {
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

        for (id, instance) in instances {
            if let Err(error) = instance.killer.lock().kill() {
                log::warn!("[{}] Failed to kill child: {}", id, error);
            }
        }
    }
}

/// 全局 PTY 管理器存储
static PTY_MANAGER: LazyLock<Mutex<Option<Arc<PtyManager>>>> = LazyLock::new(|| Mutex::new(None));

/// 初始化 PTY 管理器
pub(crate) fn init_pty_manager(app_handle: AppHandle, admission: AdmissionGate) {
    let manager = Arc::new(PtyManager::new(app_handle, admission));
    *PTY_MANAGER.lock() = Some(manager);
    log::info!("PTY manager initialized");
}

/// 获取 PTY 管理器
pub fn get_pty_manager() -> Option<Arc<PtyManager>> {
    PTY_MANAGER.lock().clone()
}

/// 读循环核心（与 AppHandle 解耦以便单测错误计数语义）。
/// 返回 None 表示流正常结束；Some 描述触发 fail_reader 的读取错误。
fn pump_pty_output(
    pty_id: &str,
    reader: &mut dyn Read,
    emit: &mut dyn FnMut(String),
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
                    emit(output);
                }
                log::info!("[{}] PTY EOF reached", pty_id);
                return None;
            }
            Ok(count) => {
                consecutive_errors = 0;
                let output = decoder.decode(&buf[..count]);
                if !output.is_empty() {
                    emit(output);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // WouldBlock 是无数据的成功轮询而非读取错误，重置计数，
                // 保持「连续 5 次真实错误才终止」不被非错误事件稀释。
                consecutive_errors = 0;
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) if is_pty_stream_end(&error) => {
                let output = decoder.flush();
                if !output.is_empty() {
                    emit(output);
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

/// 校验 --resume 的 sessionId：先做字符白名单（拒绝 shell 元字符），
/// 再复用 store 的文件名组件校验（防路径逃逸、Windows 保留名、agent- 前缀）。
fn validate_resume_session_id(session_id: &str) -> Result<(), String> {
    if session_id.starts_with('-') {
        return Err("session id cannot start with '-'".to_string());
    }
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(format!(
            "session id contains characters outside [A-Za-z0-9._-]: {session_id}"
        ));
    }
    crate::store::validate_session_id_component(session_id)
}

/// 只校验 Desk 自己构建的 owned `--resume` 参数对；不得用它解释用户原始 CLI 参数语义。
fn validate_resume_session_ids(args: &[String]) -> Result<(), String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--resume" {
            let session_id = iter
                .next()
                .ok_or_else(|| "--resume requires a session id".to_string())?;
            validate_resume_session_id(session_id)?;
        } else if let Some(session_id) = arg.strip_prefix("--resume=") {
            validate_resume_session_id(session_id)?;
        }
    }
    Ok(())
}

/// 将 Desk 历史 locator 作为受校验的参数对前置，再原样附加用户 argv。
/// 用户 argv 由 Claude CLI 自己解释，不能扫描、补值或改写其中的 `--resume` / `-r`。
fn prepare_legacy_claude_args(
    resume_session_id: Option<&str>,
    user_args: Option<&[String]>,
) -> Result<Option<Vec<String>>, String> {
    let mut prepared = Vec::new();
    if let Some(session_id) = resume_session_id {
        let owned = ["--resume".to_string(), session_id.to_string()];
        validate_resume_session_ids(&owned)?;
        prepared.extend(owned);
    }
    if let Some(args) = user_args {
        prepared.extend_from_slice(args);
    }
    Ok((!prepared.is_empty()).then_some(prepared))
}

fn apply_configured_claude_environment(
    cmd: &mut CommandBuilder,
    env_vars: &HashMap<String, String>,
) -> bool {
    let mut root_proof = true;
    for (key, value) in env_vars {
        if key.eq_ignore_ascii_case("CLAUDE_CONFIG_DIR")
            || (cfg!(target_os = "windows") && key.eq_ignore_ascii_case("USERPROFILE"))
            || (key.eq_ignore_ascii_case("BASH_ENV") && !value.is_empty())
        {
            root_proof = false;
        }
        cmd.env(key, value);
    }
    root_proof
}

/// 环境快照只有在 shell 不会继续加载用户启动脚本时，才能升级为默认根证明。
/// Windows Git Bash 的非交互 `-c` 启动脚本入口由环境检查中的 `BASH_ENV` 覆盖；
/// PowerShell fallback 会读取 profile，Unix 的 `bash -i -c` 会读取 `.bashrc`。
fn legacy_claude_root_is_proven(
    environment_proves_default_root: bool,
    git_bash_path: Option<&str>,
) -> bool {
    #[cfg(target_os = "windows")]
    {
        environment_proves_default_root && git_bash_path.is_some()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = environment_proves_default_root;
        let _ = git_bash_path;
        false
    }
}

/// 组装交给 shell 执行的 Claude 命令：可执行路径与每个参数逐一引用为
/// shell 字面量，元字符不会越过引号边界被 shell 重新解释。
///
/// PowerShell 的 `-Command` 把整串当脚本解析，裸引号字符串是表达式而非
/// 命令，必须以调用操作符 `&` 开头才能真正执行；POSIX 无此要求。
fn build_claude_command(
    claude_path: Option<&str>,
    args: Option<&[String]>,
    shell_kind: ShellKind,
) -> String {
    let program = match claude_path {
        Some(path) => shell_quote_arg(path, shell_kind),
        None => "claude".to_string(),
    };
    let Some(extra_args) = args.filter(|extra| !extra.is_empty()) else {
        return format!("{}{program}", ps_call_prefix(shell_kind));
    };
    let quoted = extra_args
        .iter()
        .map(|arg| shell_quote_arg(arg, shell_kind))
        .collect::<Vec<_>>()
        .join(" ");
    format!("{}{program} {quoted}", ps_call_prefix(shell_kind))
}

/// PowerShell 需要调用操作符前缀；返回值自带尾随空格
fn ps_call_prefix(shell_kind: ShellKind) -> &'static str {
    match shell_kind {
        ShellKind::Posix => "",
        ShellKind::PowerShell => "& ",
    }
}

fn acquire_legacy_writer(master: &dyn MasterPty) -> Result<Box<dyn Write + Send>> {
    #[cfg(all(test, windows))]
    maintenance_tests::check_io("writer")?;
    master.take_writer()
}

fn acquire_legacy_reader(master: &dyn MasterPty) -> Result<Box<dyn Read + Send>> {
    #[cfg(all(test, windows))]
    maintenance_tests::check_io("reader")?;
    master.try_clone_reader()
}

fn spawn_pty_thread(
    name: String,
    run: impl FnOnce() + Send + 'static,
) -> io::Result<thread::JoinHandle<()>> {
    #[cfg(all(test, windows))]
    {
        if maintenance_tests::fail_thread(&name) {
            return Err(io::Error::other("injected thread creation failure"));
        }
        let barrier = maintenance_tests::waiter_barrier(&name);
        thread::Builder::new().name(name).spawn(move || {
            if let Some(barrier) = barrier {
                barrier.wait();
            }
            run();
        })
    }
    #[cfg(not(all(test, windows)))]
    thread::Builder::new().name(name).spawn(run)
}

#[cfg(all(test, windows))]
#[path = "tests/version_history_legacy.rs"]
mod maintenance_tests;

#[cfg(test)]
mod read_loop_tests {
    #![allow(non_snake_case)]

    use super::pump_pty_output;
    use std::collections::VecDeque;
    use std::io;

    /// 按脚本回放的伪 reader；脚本耗尽按 EOF 处理，避免测试挂死
    struct ScriptedReader {
        steps: VecDeque<Step>,
    }

    enum Step {
        Data(Vec<u8>),
        Fail(io::Error),
    }

    fn data(bytes: &[u8]) -> Step {
        Step::Data(bytes.to_vec())
    }

    fn fail() -> Step {
        // 选用不会被 is_pty_stream_end 判定为流结束的错误类型
        Step::Fail(io::Error::new(
            io::ErrorKind::InvalidData,
            "injected read failure",
        ))
    }

    fn would_block() -> Step {
        Step::Fail(io::Error::from(io::ErrorKind::WouldBlock))
    }

    impl ScriptedReader {
        fn new(steps: Vec<Step>) -> Self {
            Self {
                steps: steps.into(),
            }
        }
    }

    impl io::Read for ScriptedReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.steps.pop_front() {
                Some(Step::Data(bytes)) => {
                    let count = bytes.len().min(buf.len());
                    buf[..count].copy_from_slice(&bytes[..count]);
                    Ok(count)
                }
                Some(Step::Fail(error)) => Err(error),
                None => Ok(0),
            }
        }
    }

    fn run(steps: Vec<Step>) -> (Option<String>, Vec<String>) {
        let mut reader = ScriptedReader::new(steps);
        let mut emitted: Vec<String> = Vec::new();
        let result = pump_pty_output("test-pty", &mut reader, &mut |output| emitted.push(output));
        (result, emitted)
    }

    #[test]
    fn ReadLoop_FiveConsecutiveRealErrors_Terminates() {
        let steps = (0..5).map(|_| fail()).collect();
        let (result, _) = run(steps);
        let reason = result.expect("5 consecutive real errors must terminate the loop");
        assert!(reason.starts_with("read_error"));
    }

    #[test]
    fn ReadLoop_WouldBlockBreaksErrorStreak_KeepsFiveConsecutiveSemantics() {
        // 4 次真实错误后出现 WouldBlock（非错误事件），连续性被打断；
        // 之后的 4 次真实错误不应累计到 5，最终按 EOF 正常结束。
        let mut steps: Vec<Step> = (0..4).map(|_| fail()).collect();
        steps.push(would_block());
        steps.extend((0..4).map(|_| fail()));
        let (result, _) = run(steps);
        assert!(result.is_none(), "error streak must reset on WouldBlock");
    }

    #[test]
    fn ReadLoop_ManyWouldBlocks_DoNotTerminate() {
        let steps = (0..20).map(|_| would_block()).collect();
        let (result, _) = run(steps);
        assert!(result.is_none());
    }

    #[test]
    fn ReadLoop_DataThenEof_EmitsDecodedOutput() {
        let (result, emitted) = run(vec![data("hello".as_bytes())]);
        assert!(result.is_none());
        assert_eq!(emitted, vec!["hello".to_string()]);
    }
}

#[cfg(test)]
#[path = "tests/legacy_claude_launch.rs"]
mod shell_hardening_tests;
