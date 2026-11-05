//! Optional observer preparation. Owns observation authority, not a CLI process.
use crate::cli::environment::{EnvMap, ObserverEnv};
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use crate::observer_registry::{ObserverDelivery, ObserverLease, ObserverRegistry, ObserverRun};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub(crate) struct ObserverHost {
    registry: Arc<ObserverRegistry>,
    plugin_dir: PathBuf,
    port: Arc<dyn Fn() -> Option<u16> + Send + Sync>,
}
pub(crate) struct PreparedObservation {
    pub(crate) lease: ObserverLease,
    pub(crate) environment: ObserverEnv,
    pub(crate) plugin_dir: PathBuf,
    pub(crate) codex_args: Option<Vec<std::ffi::OsString>>,
    pub(crate) opencode_plugin: Option<PathBuf>,
}
impl ObserverHost {
    pub(crate) fn new(
        registry: Arc<ObserverRegistry>,
        plugin_dir: PathBuf,
        port: Arc<dyn Fn() -> Option<u16> + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            plugin_dir,
            port,
        }
    }
    pub(crate) fn prepare(
        &self,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PreparedObservation, SafeError> {
        let port = (self.port)()
            .filter(|port| *port != 0)
            .ok_or_else(|| error("OBSERVER_SERVICE_UNAVAILABLE"))?;
        if !crate::hook_config::plugin_ready(&self.plugin_dir) {
            return Err(error("OBSERVER_PLUGIN_UNAVAILABLE"));
        }
        let lease = self.registry.lease(run, delivery, authorize)?;
        let binding = lease.binding();
        let environment = ObserverEnv {
            values: EnvMap::from([
                ("CC_BOX_HOOK_PORT".into(), port.to_string().into()),
                (
                    "CC_DESK_OBSERVER_RUN".into(),
                    binding.run.run_id.clone().into(),
                ),
                (
                    "CC_DESK_OBSERVER_GENERATION".into(),
                    binding.run.generation.to_string().into(),
                ),
                (
                    "CC_DESK_OBSERVER_CAPABILITY".into(),
                    binding.capability.clone().into(),
                ),
            ]),
        };
        Ok(PreparedObservation {
            lease,
            environment,
            plugin_dir: self.plugin_dir.clone(),
            codex_args: None,
            opencode_plugin: None,
        })
    }
    pub(crate) fn prepare_codex(
        &self,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PreparedObservation, SafeError> {
        let port = (self.port)()
            .filter(|port| *port != 0)
            .ok_or_else(|| error("OBSERVER_SERVICE_UNAVAILABLE"))?;
        let args = crate::codex_observer::hook_args(
            &std::env::current_exe().map_err(|_| error("OBSERVER_PREPARE_FAILED"))?,
        )?;
        let lease = self.registry.lease_source(
            run,
            delivery,
            authorize,
            crate::observer_registry::ObserverSource::CodexHook,
        )?;
        let binding = lease.binding();
        let names = crate::codex_observer::ENV_NAMES;
        let values = EnvMap::from([
            (names[0].into(), port.to_string().into()),
            (names[1].into(), binding.run.run_id.clone().into()),
            (names[2].into(), binding.run.generation.to_string().into()),
            (names[3].into(), binding.capability.clone().into()),
        ]);
        Ok(PreparedObservation {
            lease,
            environment: ObserverEnv { values },
            plugin_dir: PathBuf::new(),
            codex_args: Some(args),
            opencode_plugin: None,
        })
    }

    pub(crate) fn prepare_opencode(
        &self,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PreparedObservation, SafeError> {
        let port = (self.port)()
            .filter(|p| *p != 0)
            .ok_or_else(|| error("OBSERVER_SERVICE_UNAVAILABLE"))?;
        let resource = tempfile::tempdir().map_err(|_| error("OBSERVER_PLUGIN_UNAVAILABLE"))?;
        let path = resource.path().join("cc-desk-observer.mjs");
        std::fs::write(&path, include_str!("opencode_observer.mjs"))
            .map_err(|_| error("OBSERVER_PLUGIN_UNAVAILABLE"))?;
        let mut lease = self.registry.lease_source(
            run,
            delivery,
            authorize,
            crate::observer_registry::ObserverSource::OpencodeHook,
        )?;
        let binding = lease.binding();
        let values = EnvMap::from([
            ("CC_BOX_HOOK_PORT".into(), port.to_string().into()),
            (
                "CC_DESK_OBSERVER_RUN".into(),
                binding.run.run_id.clone().into(),
            ),
            (
                "CC_DESK_OBSERVER_GENERATION".into(),
                binding.run.generation.to_string().into(),
            ),
            (
                "CC_DESK_OBSERVER_CAPABILITY".into(),
                binding.capability.clone().into(),
            ),
        ]);
        lease.keep_resource(resource);
        Ok(PreparedObservation {
            lease,
            environment: ObserverEnv { values },
            plugin_dir: PathBuf::new(),
            codex_args: None,
            opencode_plugin: Some(path),
        })
    }
}

impl ObserverHost {
    /// opencode v2 不加载 v1 观察插件，会话状态改由 CLI 直接写进 SQLite（title/
    /// time_idle/time_busy）。此 watcher 以完全相同的 accept/delivery 校验路径，
    /// 把「数据库文件变化」转换为一个有界 Notification 刷新 cue——它只是刷新
    /// 信号，永远不是身份或标题来源；身份由 prepare 的精确会话 ID 提供。
    /// db 路径由调用方从冻结启动环境经 selection::locations 解析后传入（与
    /// scope/probe 同一规则）；channel 命名库在 prepare 阶段尚未决议，此时传入
    /// 默认文件名属已知盲区，退化为与「无观察」一致，不影响授权语义。
    pub(crate) fn spawn_opencode_db_cue(
        &self,
        binding: crate::observer_registry::ObserverBinding,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        session_id: String,
        database: std::path::PathBuf,
        interval: std::time::Duration,
    ) {
        let registry = self.registry.clone();
        let cue_sequence = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let cue_sequence_for_thread = cue_sequence.clone();
        std::thread::Builder::new()
            .name("opencode-db-cue".into())
            .spawn(move || {
                let cue_sequence = cue_sequence_for_thread;
                let Some(publish) = crate::hook_server::publisher() else {
                    return;
                };
                let mut last: Option<std::time::SystemTime> = None;
                loop {
                    std::thread::sleep(interval);
                    if !authorize() {
                        return;
                    }
                    // WAL 模式下提交通常只写 -wal，主库要 checkpoint 才动；
                    // 取 db 与 wal 的最新 mtime 作为观察值，漏一侧就漏整个
                    // 运行期 cue（cli/AGENTS.md 的 WAL 约束）。
                    let mut wal = database.clone().into_os_string();
                    wal.push("-wal");
                    let observed = [
                        std::fs::metadata(&database).ok(),
                        std::fs::metadata(std::path::PathBuf::from(wal)).ok(),
                    ]
                    .into_iter()
                    .flatten()
                    .filter_map(|metadata| metadata.modified().ok())
                    .max();
                    let Ok(modified) = observed.ok_or("no source") else {
                        continue;
                    };
                    // 首次读取只建立基线；启动即发的 cue 没有任何信息量。
                    if Some(modified) == last {
                        continue;
                    }
                    let first_sample = last.is_none();
                    last = Some(modified);
                    if first_sample {
                        continue;
                    }
                    // opencode 的 replay 窗口只按唯一性去重（不要求单调）。事件
                    // ID = 毫秒时间戳（13 位）+ watcher 内 3 位序列：同毫秒内的
                    // 连续两次 mtime 变化也各得唯一 ID，不会被 Duplicate 吞掉。
                    let millis = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_millis() % 10_000_000_000_000)
                        .unwrap_or(0);
                    let sequence = cue_sequence.fetch_add(1, Ordering::Relaxed) % 1_000;
                    let event_id = format!("o{millis:013}{sequence:03}");
                    let payload = serde_json::json!({
                        "hook_event_name": "Notification",
                        "session_id": session_id,
                        "notification_type": "opencode_unknown",
                    });
                    // Duplicate 静默等下一次变化；撤销由循环头的 authorize 检测，
                    // Err（FORBIDDEN/容量）只损失一轮 cue，不改变授权语义。
                    if let Ok(crate::observer_registry::ObserverAccept::Accepted(event)) =
                        registry.accept_event(&binding, &event_id, payload.to_string().as_bytes())
                    {
                        if registry.deliver(&binding, event, &publish).is_err() {
                            return;
                        }
                    }
                }
            })
            .ok(); // 线程创建失败只是没有刷新 cue；启动本身不依赖观察。
    }
}

#[cfg(test)]
mod db_cue_tests {
    use super::*;
    use crate::observer_registry::{ObserverRun, ObserverSource};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_DbCueAcceptsBoundedNotificationOnMtimeChange_060() {
        let guard = tempfile::tempdir().unwrap();
        let database = guard.path().join("opencode").join("opencode.db");
        let db_path = database.clone();
        // 真实 opencode 是 WAL：提交易先写 -wal。测试模拟「主库不动、只有
        // WAL 变化」的运行中会话形态（code-review P1）。
        let wal_path = guard.path().join("opencode").join("opencode.db-wal");
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        std::fs::write(&database, b"init").unwrap();
        std::fs::write(&wal_path, b"wal").unwrap();
        // 路径与间隔已参数化：测试直接传临时文件与毫秒级轮询，无进程级 env 竞争。
        let registry = Arc::new(ObserverRegistry::with_capacity(8));
        let binding = registry
            .mint(
                ObserverRun {
                    run_id: "run-opencode-cue".into(),
                    generation: 1,
                },
                ObserverSource::OpencodeHook,
            )
            .unwrap();
        let received = Arc::new(AtomicUsize::new(0));
        let last_id = Arc::new(Mutex::new(String::new()));
        let received_for_publish = received.clone();
        let last_for_publish = last_id.clone();
        crate::hook_server::set_publisher_for_test(Arc::new(move |delivery, payload| {
            assert_eq!(delivery.window_label, "main");
            assert_eq!(payload.event_name, "Notification");
            *last_for_publish.lock().unwrap() = payload.event_id.unwrap_or_default();
            received_for_publish.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));
        let alive = Arc::new(AtomicBool::new(true));
        let alive_for_watcher = alive.clone();
        let host = ObserverHost::new(registry.clone(), PathBuf::new(), Arc::new(|| None));
        host.spawn_opencode_db_cue(
            binding,
            Arc::new(move || alive_for_watcher.load(Ordering::SeqCst)),
            "ses_cue_fixture".into(),
            db_path,
            std::time::Duration::from_millis(20),
        );
        // 首轮只建立基线；先等过基线窗口再改 WAL 的 mtime（主库保持不动，
        // 覆盖「正在运行的会话标题写入 WAL、主库等 checkpoint」的现场形态）。
        std::thread::sleep(std::time::Duration::from_millis(120));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&wal_path)
            .unwrap();
        let newer = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        file.set_modified(newer).unwrap();
        drop(file);
        for _ in 0..120 {
            if received.load(Ordering::SeqCst) >= 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            received.load(Ordering::SeqCst) >= 1,
            "mtime change must deliver exactly the bounded Notification cue"
        );
        assert!(last_id.lock().unwrap().starts_with('o'));
        alive.store(false, Ordering::SeqCst);
    }
}
