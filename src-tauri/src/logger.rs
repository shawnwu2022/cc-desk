//! 日志模块
//! 将日志写入 ~/.cc-box/logs/ 目录，按日期分文件
//! - {date}.log: 全部日志（INFO 及以上）
//! - {date}.error.log: 仅 WARN 及以上
//! 保留最近 7 天日志，自动清理旧文件

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;

/// 日志目录
fn log_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cc-box").join("logs"))
}

/// 确保日志目录存在
fn ensure_log_dir() -> Option<PathBuf> {
    let dir = log_dir()?;
    if !dir.exists() {
        fs::create_dir_all(&dir).ok()?;
    }
    Some(dir)
}

/// 当天日期前缀
fn date_prefix() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// 打开日志文件（追加）
fn open_log_file(name: &str) -> Option<File> {
    let dir = ensure_log_dir()?;
    let path = dir.join(name);
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// 格式化日志行
fn format_line(record: &log::Record) -> String {
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    format!(
        "[{}][{}] [{}] {}\n",
        ts,
        record.level(),
        record.target(),
        record.args()
    )
}

/// 清理超过 keep_days 天的旧日志
fn cleanup_old_logs(keep_days: u64) {
    let dir = match log_dir() {
        Some(d) if d.exists() => d,
        _ => return,
    };

    let cutoff =
        std::time::SystemTime::now() - std::time::Duration::from_secs(keep_days * 24 * 60 * 60);

    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "log").unwrap_or(false) {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        if modified < cutoff {
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
            }
        }
    }
}

struct FileLogger {
    app_log: Mutex<Option<File>>,
    error_log: Mutex<Option<File>>,
}

static LOGGER: FileLogger = FileLogger {
    app_log: Mutex::new(None),
    error_log: Mutex::new(None),
};

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let line = format_line(record);

        // 写入全部日志文件
        if let Ok(mut guard) = self.app_log.lock() {
            if let Some(ref mut f) = *guard {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }

        // WARN 及以上写入错误日志文件
        if record.level() <= log::Level::Warn {
            if let Ok(mut guard) = self.error_log.lock() {
                if let Some(ref mut f) = *guard {
                    let _ = f.write_all(line.as_bytes());
                    let _ = f.flush();
                }
            }
        }

        // debug 模式同时输出到 stderr
        #[cfg(debug_assertions)]
        {
            eprint!("{}", line);
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.app_log.lock() {
            if let Some(ref mut f) = *guard {
                let _ = f.flush();
            }
        }
        if let Ok(mut guard) = self.error_log.lock() {
            if let Some(ref mut f) = *guard {
                let _ = f.flush();
            }
        }
    }
}

/// 初始化日志系统，在 Tauri setup 阶段调用
pub fn init() {
    let prefix = date_prefix();

    // 打开日志文件
    let app_file = open_log_file(&format!("{}.log", prefix));
    let err_file = open_log_file(&format!("{}.error.log", prefix));

    if let Ok(mut guard) = LOGGER.app_log.lock() {
        *guard = app_file;
    }
    if let Ok(mut guard) = LOGGER.error_log.lock() {
        *guard = err_file;
    }

    // debug 模式用 Debug 级别，release 用 Info
    let level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    log::set_max_level(level);

    // 设置全局日志器
    if log::set_logger(&LOGGER).is_err() {
        // 已有其他 logger（理论上不会发生）
        eprintln!("[Logger] Warning: another logger already set");
    }

    log::info!("=== CC-Box started ===");
    if let Some(dir) = log_dir() {
        log::info!("Log directory: {}", dir.display());
    }

    // 后台清理旧日志
    std::thread::spawn(|| {
        cleanup_old_logs(7);
    });
}
