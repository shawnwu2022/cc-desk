//! PTY 管理模块
//! 基于 portable-pty 实现 Claude CLI 进程管理

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtyPair, PtySize, Child};
use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

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

/// PTY 错误事件 payload
#[derive(Debug, Clone, serde::Serialize)]
pub struct PtyErrorPayload {
    pub id: String,
    pub error: String,
    pub stage: String,
}

/// PTY 进程状态
#[derive(Debug, Clone, PartialEq)]
pub enum PtyStatus {
    Starting,
    Running,
    Stopping,
    Stopped,
}

/// PTY 实例内部数据
struct PtyInstanceData {
    pair: PtyPair,
    child: Box<dyn Child + Send>,  // 存储 child 句柄
    cwd: String,
    pty_type: String,
    status: PtyStatus,
    created_at: std::time::Instant,
}

/// PTY 管理器（全局）
pub struct PtyManager {
    /// PTY 实例（包含 pair 和元数据）
    instances: Mutex<HashMap<String, PtyInstanceData>>,
    /// Writer 句柄（需要单独存储因为实现了 Send）
    writers: Mutex<HashMap<String, Box<dyn Write + Send>>>,
    /// Tauri AppHandle 用于发送事件
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

    /// 检测 Git Bash 路径（Windows）
    fn detect_git_bash() -> Option<String> {
        if !cfg!(target_os = "windows") {
            return None;
        }

        log::debug!("Detecting Git Bash on Windows...");

        // 常见安装路径
        let candidates = [
            "D:\\Program Files\\Git\\bin\\bash.exe",
            "C:\\Program Files\\Git\\bin\\bash.exe",
            "C:\\Program Files (x86)\\Git\\bin\\bash.exe",
        ];

        for path in &candidates {
            if Path::new(path).exists() {
                log::info!("Git Bash found at candidate path: {}", path);
                return Some(path.to_string());
            }
        }

        // 从 PATH 环境变量查找
        let path_env = match env::var("PATH") {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Failed to read PATH environment: {}", e);
                return None;
            }
        };

        for entry in path_env.split(';') {
            if entry.contains("Git") && entry.contains("bin") {
                let bash_path = format!(
                    "{}\\bash.exe",
                    entry.trim_end_matches('\\').trim_end_matches('/')
                );
                if Path::new(&bash_path).exists() {
                    log::info!("Git Bash found in PATH: {}", bash_path);
                    return Some(bash_path);
                }
            }
        }

        // 检查环境变量覆盖
        if let Ok(path) = env::var("CLAUDE_CODE_GIT_BASH_PATH") {
            if Path::new(&path).exists() {
                log::info!("Git Bash found via CLAUDE_CODE_GIT_BASH_PATH: {}", path);
                return Some(path);
            }
        }

        log::warn!("Git Bash not found - Claude CLI may not work properly on Windows");
        None
    }

    /// 检测 Claude CLI 路径
    fn detect_claude_path() -> Option<String> {
        log::debug!("Detecting Claude CLI path...");

        // Windows: 优先检查用户目录下的安装
        if cfg!(target_os = "windows") {
            if let Some(home) = dirs::home_dir() {
                let claude_path = home.join(".local").join("bin").join("claude.exe");
                if claude_path.exists() {
                    log::info!("Claude CLI found at: {}", claude_path.display());
                    return Some(claude_path.to_string_lossy().to_string());
                }
            }
        }

        // 从 PATH 环境变量查找
        let path_env = match env::var("PATH") {
            Ok(p) => p,
            Err(_) => return None,
        };

        let separator = if cfg!(target_os = "windows") {
            ';'
        } else {
            ':'
        };
        let claude_name = if cfg!(target_os = "windows") {
            "claude.exe"
        } else {
            "claude"
        };

        for entry in path_env.split(separator) {
            let trimmed = entry.trim_end_matches('\\').trim_end_matches('/');
            if trimmed.is_empty() {
                continue;
            }
            let claude_path = format!(
                "{}{}{}",
                trimmed,
                if cfg!(target_os = "windows") {
                    "\\"
                } else {
                    "/"
                },
                claude_name
            );
            if Path::new(&claude_path).exists() {
                log::info!("Claude CLI found in PATH: {}", claude_path);
                return Some(claude_path);
            }
        }

        log::warn!("Claude CLI not found in PATH or common locations");
        None
    }

    /// 构建环境变量（继承父进程环境 + 自定义设置）
    fn build_env_vars(cwd: &str) -> Vec<(String, String)> {
        let mut env_vars: Vec<(String, String)> = Vec::new();

        // 基础终端环境变量
        env_vars.push(("TERM".to_string(), "xterm-256color".to_string()));
        env_vars.push(("COLORTERM".to_string(), "truecolor".to_string()));
        env_vars.push(("PWD".to_string(), cwd.to_string()));

        // Windows 特定：Git Bash 路径
        if cfg!(target_os = "windows") {
            if let Some(git_bash) = Self::detect_git_bash() {
                env_vars.push(("CLAUDE_CODE_GIT_BASH_PATH".to_string(), git_bash));
            }
        }

        env_vars
    }

    /// 启动 Claude CLI
    pub fn spawn_claude(
        &self,
        cwd: &str,
        cols: u16,
        rows: u16,
        args: Option<Vec<String>>,
    ) -> Result<PtyInfo> {
        let id = Uuid::new_v4().to_string();
        log::info!(
            "Spawning Claude CLI with id={}, cwd={}, size={}x{}",
            id,
            cwd,
            cols,
            rows
        );

        // 验证工作目录
        if !Path::new(cwd).exists() {
            let err_msg = format!("Working directory does not exist: {}", cwd);
            log::error!("{}", err_msg);
            self.emit_error(&id, &err_msg, "validation");
            return Err(anyhow!("{}", err_msg));
        }

        // 创建 PTY
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("Failed to open PTY with size {}x{}", cols, rows))?;

        // 检测 Claude CLI 路径
        let claude_path = match Self::detect_claude_path() {
            Some(p) => p,
            None => {
                let err_msg = "Claude CLI not found. Please install Claude CLI first (npm install -g @anthropic-ai/claude-code)";
                log::error!("{}", err_msg);
                self.emit_error(&id, err_msg, "detection");
                return Err(anyhow!("{}", err_msg));
            }
        };

        // 构建命令
        let mut cmd = CommandBuilder::new(&claude_path);

        // 添加参数
        if let Some(extra_args) = &args {
            for arg in extra_args {
                cmd.arg(arg);
            }
        }
        cmd.cwd(cwd);

        // 关键：先继承所有父进程环境变量，再添加自定义
        for (key, value) in env::vars() {
            cmd.env(key, value);
        }

        // 添加自定义环境变量（会覆盖同名的父进程变量）
        for (key, value) in Self::build_env_vars(cwd) {
            cmd.env(key, value);
        }

        log::debug!("Command: {:?} with args: {:?}", claude_path, args);

        // 启动进程 - 存储 child 句柄
        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("Failed to spawn Claude CLI '{}': {}", claude_path, e);
                log::error!("{}", err_msg);
                self.emit_error(&id, &err_msg, "spawn");
                return Err(anyhow!("{}", err_msg));
            }
        };

        // 获取 writer 和 reader
        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                let err_msg = format!("Failed to take PTY writer: {}", e);
                log::error!("{}", err_msg);
                self.emit_error(&id, &err_msg, "writer");
                return Err(anyhow!("{}", err_msg));
            }
        };

        let reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Failed to clone PTY reader: {}", e);
                log::error!("{}", err_msg);
                self.emit_error(&id, &err_msg, "reader");
                return Err(anyhow!("{}", err_msg));
            }
        };

        // 启动输出读取线程
        let pty_id = id.clone();
        let app_handle = self.app_handle.clone();
        let cwd_for_log = cwd.to_string();

        thread::Builder::new()
            .name(format!("pty-reader-{}", &pty_id[..8.min(pty_id.len())]))
            .spawn(move || {
                log::debug!("[{}] Output reader thread started", pty_id);
                Self::read_output_loop(pty_id, reader, app_handle, &cwd_for_log);
            })
            .with_context(|| format!("Failed to spawn reader thread for PTY {}", id))?;

        // 给进程启动一点时间
        thread::sleep(Duration::from_millis(50));

        // 存储 PTY 实例（包含 child）
        let instance = PtyInstanceData {
            pair,
            child,
            cwd: cwd.to_string(),
            pty_type: "claude".to_string(),
            status: PtyStatus::Running,
            created_at: std::time::Instant::now(),
        };

        self.instances.lock().insert(id.clone(), instance);
        self.writers.lock().insert(id.clone(), writer);

        log::info!("[{}] Claude CLI spawned successfully", id);

        Ok(PtyInfo {
            id,
            pty_type: "claude".to_string(),
            cwd: cwd.to_string(),
        })
    }

    /// 输出读取循环（静态方法，在线程中运行）
    fn read_output_loop(
        pty_id: String,
        mut reader: Box<dyn std::io::Read + Send>,
        app_handle: AppHandle,
        _cwd: &str,
    ) {
        let mut buf = [0u8; 4096];
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: i32 = 5;

        loop {
            use std::io::Read;
            match reader.read(&mut buf) {
                Ok(0) => {
                    // EOF - 进程正常退出
                    log::info!("[{}] PTY EOF reached, process exited normally", pty_id);
                    app_handle
                        .emit(
                            "pty-exit",
                            PtyExitPayload {
                                id: pty_id.clone(),
                                exit_code: 0,
                                signal: None,
                            },
                        )
                        .ok();
                    break;
                }
                Ok(n) => {
                    consecutive_errors = 0;
                    let output = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = app_handle
                        .emit(
                            "pty-output",
                            PtyOutputPayload {
                                id: pty_id.clone(),
                                data: output,
                            },
                        );
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        // 非阻塞模式下的暂时性错误，继续等待
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    }

                    consecutive_errors += 1;
                    log::warn!(
                        "[{}] Read error ({}/{}): {}",
                        pty_id,
                        consecutive_errors,
                        MAX_CONSECUTIVE_ERRORS,
                        e
                    );

                    if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                        log::error!(
                            "[{}] Too many consecutive read errors, stopping reader",
                            pty_id
                        );
                        app_handle
                            .emit(
                                "pty-exit",
                                PtyExitPayload {
                                    id: pty_id.clone(),
                                    exit_code: -1,
                                    signal: Some(format!("read_error: {}", e)),
                                },
                            )
                            .ok();
                        break;
                    }

                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }

    /// 发送错误事件
    fn emit_error(&self, id: &str, error: &str, stage: &str) {
        self.app_handle
            .emit(
                "pty-error",
                PtyErrorPayload {
                    id: id.to_string(),
                    error: error.to_string(),
                    stage: stage.to_string(),
                },
            )
            .ok();
    }

    /// 启动普通 Shell
    pub fn spawn_shell(&self, cwd: &str, cols: u16, rows: u16) -> Result<PtyInfo> {
        let id = Uuid::new_v4().to_string();
        log::info!(
            "Spawning shell with id={}, cwd={}, size={}x{}",
            id,
            cwd,
            cols,
            rows
        );

        // 验证工作目录
        if !Path::new(cwd).exists() {
            let err_msg = format!("Working directory does not exist: {}", cwd);
            log::error!("{}", err_msg);
            self.emit_error(&id, &err_msg, "validation");
            return Err(anyhow!("{}", err_msg));
        }

        // 创建 PTY
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("Failed to open PTY with size {}x{}", cols, rows))?;

        // 确定使用的 shell
        let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
            ("cmd.exe", vec![])
        } else {
            ("/bin/bash", vec!["-i"])
        };

        log::debug!("Using shell: {} with args: {:?}", program, args);

        // 构建命令
        let mut cmd = CommandBuilder::new(program);
        for arg in &args {
            cmd.arg(*arg);
        }
        cmd.cwd(cwd);

        // 继承父进程环境变量
        for (key, value) in env::vars() {
            cmd.env(key, value);
        }

        // 添加终端环境变量
        cmd.env("TERM", "xterm-256color");

        // 启动进程 - 存储 child 句柄
        let child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                let err_msg = format!("Failed to spawn shell '{}': {}", program, e);
                log::error!("{}", err_msg);
                self.emit_error(&id, &err_msg, "spawn");
                return Err(anyhow!("{}", err_msg));
            }
        };

        // 获取 writer 和 reader
        let writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                let err_msg = format!("Failed to take PTY writer: {}", e);
                log::error!("{}", err_msg);
                self.emit_error(&id, &err_msg, "writer");
                return Err(anyhow!("{}", err_msg));
            }
        };

        let reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("Failed to clone PTY reader: {}", e);
                log::error!("{}", err_msg);
                self.emit_error(&id, &err_msg, "reader");
                return Err(anyhow!("{}", err_msg));
            }
        };

        // 启动输出读取线程
        let pty_id = id.clone();
        let app_handle = self.app_handle.clone();
        let cwd_for_log = cwd.to_string();

        thread::Builder::new()
            .name(format!("pty-reader-{}", &pty_id[..8.min(pty_id.len())]))
            .spawn(move || {
                log::debug!("[{}] Shell output reader thread started", pty_id);
                Self::read_output_loop(pty_id, reader, app_handle, &cwd_for_log);
            })
            .with_context(|| format!("Failed to spawn reader thread for PTY {}", id))?;

        // 给进程启动一点时间
        thread::sleep(Duration::from_millis(50));

        // 存储 PTY 实例（包含 child）
        let instance = PtyInstanceData {
            pair,
            child,
            cwd: cwd.to_string(),
            pty_type: "shell".to_string(),
            status: PtyStatus::Running,
            created_at: std::time::Instant::now(),
        };

        self.instances.lock().insert(id.clone(), instance);
        self.writers.lock().insert(id.clone(), writer);

        log::info!("[{}] Shell spawned successfully", id);

        Ok(PtyInfo {
            id,
            pty_type: "shell".to_string(),
            cwd: cwd.to_string(),
        })
    }

    /// 写入输入到 PTY
    pub fn write(&self, id: &str, data: &str) -> Result<()> {
        log::trace!("[{}] Writing {} bytes to PTY", id, data.len());

        let mut writers = self.writers.lock();
        let writer = writers.get_mut(id).ok_or_else(|| {
            let err = format!("PTY writer not found for id: {}", id);
            log::warn!("{}", err);
            anyhow!("{}", err)
        })?;

        writer
            .write_all(data.as_bytes())
            .with_context(|| format!("Failed to write {} bytes to PTY {}", data.len(), id))?;

        writer
            .flush()
            .with_context(|| format!("Failed to flush PTY {}", id))?;

        log::trace!("[{}] Write successful", id);
        Ok(())
    }

    /// 调整 PTY 大小
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        log::debug!("[{}] Resizing PTY to {}x{}", id, cols, rows);

        let instances = self.instances.lock();
        let instance = instances.get(id).ok_or_else(|| {
            let err = format!("PTY instance not found for id: {}", id);
            log::warn!("{}", err);
            anyhow!("{}", err)
        })?;

        instance
            .pair
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .with_context(|| format!("Failed to resize PTY {} to {}x{}", id, cols, rows))?;

        log::debug!("[{}] Resize successful", id);
        Ok(())
    }

    /// 杀掉 PTY 进程
    pub fn kill(&self, id: &str) -> Result<()> {
        log::info!("[{}] Killing PTY", id);

        // 从 instances 中取出（而非只是移除引用）
        let instance = self.instances.lock().remove(id);

        if let Some(mut instance) = instance {
            // 更新状态
            instance.status = PtyStatus::Stopping;

            // 强制终止进程
            instance.child.kill()
                .with_context(|| format!("Failed to kill child process {}", id))?;

            // 等待进程退出（最多等待 5 秒）
            let _ = instance.child.wait();

            // 移除 writer
            self.writers.lock().remove(id);

            // 发送退出事件
            self.app_handle
                .emit(
                    "pty-exit",
                    PtyExitPayload {
                        id: id.to_string(),
                        exit_code: 137, // SIGKILL
                        signal: Some("killed".to_string()),
                    },
                )
                .ok();

            log::info!("[{}] PTY killed successfully", id);
        } else {
            log::warn!("[{}] PTY not found when trying to kill", id);
        }

        Ok(())
    }

    /// 杀掉所有 PTY 进程
    pub fn kill_all(&self) {
        let mut instances = self.instances.lock();
        let mut writers = self.writers.lock();

        let count = instances.len();
        log::info!("Killing all {} PTY instances", count);

        // 终止所有进程
        for (id, mut instance) in instances.drain() {
            instance.status = PtyStatus::Stopping;

            // 强制终止
            if instance.child.kill().is_ok() {
                log::info!("[{}] Child killed", id);
            }

            // 等待退出
            let _ = instance.child.wait();
        }

        writers.clear();

        log::info!("Killed {} PTY instances", count);
    }

    /// 获取所有 PTY ID
    pub fn list(&self) -> Vec<String> {
        self.instances.lock().keys().cloned().collect()
    }

    /// 获取 PTY 信息
    pub fn info(&self, id: &str) -> Option<PtyInfo> {
        let instances = self.instances.lock();
        instances.get(id).map(|i| PtyInfo {
            id: id.to_string(),
            pty_type: i.pty_type.clone(),
            cwd: i.cwd.clone(),
        })
    }

    /// 获取 PTY 状态
    pub fn status(&self, id: &str) -> Option<PtyStatus> {
        let instances = self.instances.lock();
        instances.get(id).map(|i| i.status.clone())
    }

    /// 检查 PTY 是否存在
    pub fn exists(&self, id: &str) -> bool {
        self.instances.lock().contains_key(id)
    }

    /// 获取 PTY 运行时长（秒）
    pub fn uptime(&self, id: &str) -> Option<u64> {
        let instances = self.instances.lock();
        instances.get(id).map(|i| i.created_at.elapsed().as_secs())
    }

    /// 清理已停止的 PTY（由 exited 事件触发）
    pub fn cleanup(&self, id: &str) {
        log::debug!("[{}] Cleaning up PTY resources", id);

        if let Some(instance) = self.instances.lock().get_mut(id) {
            instance.status = PtyStatus::Stopped;
        }

        // 注意：不立即移除，让前端有机会获取退出信息
        // writer 可以安全移除
        self.writers.lock().remove(id);
        log::debug!("[{}] PTY writer cleaned up", id);
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
