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

    /// 使用 where (Windows) / which (Unix) 查找可执行文件
    fn find_executable(name: &str) -> Option<String> {
        let cmd = if cfg!(target_os = "windows") {
            "where"
        } else {
            "which"
        };

        let output = std::process::Command::new(cmd)
            .arg(name)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // where/which 可能返回多行，取第一个
        stdout.lines().next().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
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
        if let Some(git_path) = Self::find_executable("git") {
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

    /// 检测 Claude CLI 路径
    fn detect_claude_path() -> Option<String> {
        log::debug!("Detecting Claude CLI path...");

        // 1. 配置文件
        if let Ok(config) = crate::store::get_app_config() {
            if let Some(ref path) = config.claude_path {
                if Path::new(path).exists() {
                    log::info!("Claude CLI path from config: {}", path);
                    return Some(path.clone());
                }
            }
        }

        // 2. where/which claude
        if let Some(path) = Self::find_executable("claude") {
            log::info!("Claude CLI found via 'where claude': {}", path);
            return Some(path);
        }

        log::warn!("Claude CLI not found");
        None
    }

    /// 判断是否需要用node启动（从配置读取或检测）
    fn should_use_node_launcher(claude_path: &str) -> bool {
        // 1. 从配置读取启动类型
        if let Ok(config) = crate::store::get_app_config() {
            if let Some(ref launcher_type) = config.claude_launcher_type {
                if launcher_type == "node" {
                    log::info!("Launcher type from config: node");
                    return true;
                } else if launcher_type == "direct" {
                    log::info!("Launcher type from config: direct");
                    return false;
                }
            }
        }

        // 2. 配置无值时进行检测
        let needs_node = Self::detect_needs_node(claude_path);

        // 3. 保存检测结果到配置
        let launcher_type = if needs_node { "node" } else { "direct" };
        let updates = serde_json::json!({ "claudeLauncherType": launcher_type });
        if let Err(e) = crate::store::update_app_config(updates) {
            log::warn!("Failed to save launcher type: {}", e);
        }

        needs_node
    }

    /// 检测文件是否需要node执行
    fn detect_needs_node(path: &str) -> bool {
        // 1. 直接检查扩展名
        if path.ends_with(".js") {
            log::info!("Direct .js file: {}", path);
            return true;
        }

        // 2. 检查文件内容
        if let Ok(content) = std::fs::read_to_string(path) {
            let first_lines = content.lines().take(5).collect::<Vec<_>>();

            // 检查node shebang
            if first_lines.iter().any(|line| line.contains("#!/usr/bin/env node")) {
                log::info!("Detected Node.js script by shebang: {}", path);
                return true;
            }

            // 检查Anthropic版权信息（cli.js的特征）
            if first_lines.iter().any(|line|
                line.contains("// (c) Anthropic") && line.contains("Version:")
            ) {
                log::info!("Detected Anthropic CLI.js by content: {}", path);
                return true;
            }
        }

        // 3. Mac/Linux: 解析符号链接
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(full_path) = std::fs::canonicalize(path) {
                let full_path_str = full_path.to_string_lossy();

                // 检查真实文件扩展名
                if full_path_str.ends_with(".js") {
                    log::info!("Symlink points to .js file: {} -> {}", path, full_path_str);
                    return true;
                }

                // 检查真实文件内容
                if let Ok(content) = std::fs::read_to_string(&full_path) {
                    let first_lines = content.lines().take(5).collect::<Vec<_>>();
                    if first_lines.iter().any(|line| line.contains("#!/usr/bin/env node")) {
                        log::info!("Real file is Node.js script: {}", full_path_str);
                        return true;
                    }
                    // 检查Anthropic版权信息
                    if first_lines.iter().any(|line|
                        line.contains("// (c) Anthropic") && line.contains("Version:")
                    ) {
                        log::info!("Real file is Anthropic CLI.js: {}", full_path_str);
                        return true;
                    }
                }
            }
        }

        false
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

        // Windows: 非 .exe 路径需要特殊处理
        // Mac/Linux: 检测启动类型，node脚本需要用node执行
        let mut cmd = if cfg!(target_os = "windows")
            && !claude_path.to_lowercase().ends_with(".exe")
        {
            // Windows上检查是否是node脚本
            if Self::should_use_node_launcher(&claude_path) {
                let node_path = match Self::find_executable("node") {
                    Some(p) => p,
                    None => {
                        let err_msg = "Claude CLI is a Node.js script but 'node' command not found. Please install Node.js first.";
                        log::error!("{}", err_msg);
                        self.emit_error(&id, err_msg, "node_detection");
                        return Err(anyhow!("{}", err_msg));
                    }
                };
                log::info!("Windows Node.js script, using node to launch: {} {}", node_path, claude_path);
                let mut c = CommandBuilder::new(&node_path);
                c.arg(&claude_path);
                c
            } else {
                // Windows shim脚本用cmd.exe执行
                log::info!("Windows shim, using cmd.exe to launch: {}", claude_path);
                let mut c = CommandBuilder::new("cmd.exe");
                c.arg("/C");
                c.arg(&claude_path);
                c
            }
        } else if Self::should_use_node_launcher(&claude_path) {
            // Mac/Linux: node脚本需要用node执行
            let node_path = match Self::find_executable("node") {
                Some(p) => p,
                None => {
                    let err_msg = "Claude CLI is a Node.js script but 'node' command not found. Please install Node.js first.";
                    log::error!("{}", err_msg);
                    self.emit_error(&id, err_msg, "node_detection");
                    return Err(anyhow!("{}", err_msg));
                }
            };
            log::info!("Using Node.js to launch Claude CLI: {} {}", node_path, claude_path);
            let mut c = CommandBuilder::new(&node_path);
            c.arg(&claude_path);
            c
        } else {
            CommandBuilder::new(&claude_path)
        };

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
