//! Optional observer preparation. Owns observation authority, not a CLI process.
use crate::cli::environment::{EnvMap, ObserverEnv};
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use crate::observer_registry::{ObserverDelivery, ObserverLease, ObserverRegistry, ObserverRun};
use std::path::PathBuf;
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
    /// db 路径取 Desk 进程的默认数据根；profile 显式覆盖 XDG_DATA_HOME 的场景
    /// 拿不到 cue，退化为与「无观察」一致，不影响任何授权语义。
    pub(crate) fn spawn_opencode_db_cue(
        &self,
        binding: crate::observer_registry::ObserverBinding,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        session_id: String,
    ) {
        let registry = self.registry.clone();
        std::thread::Builder::new()
            .name("opencode-db-cue".into())
            .spawn(move || {
                let Some(publish) = crate::hook_server::publisher() else {
                    return;
                };
            let base = match std::env::var_os("XDG_DATA_HOME") {
                Some(value) if !value.is_empty() => std::path::PathBuf::from(value),
                _ => {
                    let Some(home) = dirs::home_dir() else { return };
                    home.join(".local").join("share")
                }
            };
            let database = base.join("opencode").join("opencode.db");
            let mut last: Option<std::time::SystemTime> = None;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if !authorize() {
                    return;
                }
                let Ok(metadata) = std::fs::metadata(&database) else {
                        continue;
                    };
                    let modified = metadata.modified().ok();
                    // 首次读取只建立基线；启动即发的 cue 没有任何信息量。
                    if modified.is_none() || modified == last {
                        last = modified.or(last);
                        continue;
                    }
                    let baseline = last;
                    last = modified;
                    if baseline.is_none() {
                        continue;
                    }
                    // opencode 的 replay 窗口只按唯一性去重（不要求单调），毫秒级
                    // 时间戳作为事件 ID 即可；重复收到的 Duplicate 会被安全忽略。
                    let millis = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|elapsed| elapsed.as_millis() % 10_000_000_000_000_000)
                        .unwrap_or(0);
                    let event_id = format!("o{millis:016}");
                    let payload = serde_json::json!({
                        "hook_event_name": "Notification",
                        "session_id": session_id,
                        "notification_type": "opencode_unknown",
                    });
                    match registry
                        .accept_event(&binding, &event_id, payload.to_string().as_bytes())
                    {
                        Ok(crate::observer_registry::ObserverAccept::Accepted(event)) => {
                            let Ok(delivery) = registry.delivery(&binding) else {
                                return;
                            };
                            let mut payload =
                                crate::hook_events::HookPayload::from_validated(event);
                            payload.pty_id = delivery.legacy_pty.clone();
                            if publish(&delivery, payload).is_err() {
                                return;
                            }
                        }
                        // Duplicate 继续等下一次变化；FORBIDDEN 说明 run 已撤销。
                        _ => {
                            if !authorize() {
                                return;
                            }
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
    use crate::observer_registry::{ObserverAccept, ObserverRun, ObserverSource};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[test]
    #[allow(non_snake_case)]
    fn Opencode_DbCueAcceptsBoundedNotificationOnMtimeChange_060() {
        let guard = tempfile::tempdir().unwrap();
        let database = guard.path().join("opencode").join("opencode.db");
        std::fs::create_dir_all(database.parent().unwrap()).unwrap();
        std::fs::write(&database, b"init").unwrap();
        let db_path = database.clone();
        // watcher 从进程默认根解析路径；测试用 XDG_DATA_HOME 指向临时目录。
        std::env::set_var("XDG_DATA_HOME", guard.path());
        let registry = Arc::new(ObserverRegistry::with_capacity(8));
        let binding = registry
            .mint(
                ObserverRun { run_id: "run-opencode-cue".into(), generation: 1 },
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
        let host = ObserverHost::new(
            registry.clone(),
            PathBuf::new(),
            Arc::new(|| None),
        );
        host.spawn_opencode_db_cue(
            binding,
            Arc::new(move || alive_for_watcher.load(Ordering::SeqCst)),
            "ses_cue_fixture".into(),
        );
        // 首轮只建立基线；先等过基线窗口再改 mtime。
        std::thread::sleep(std::time::Duration::from_millis(2600));
        std::fs::write(&db_path, b"changed").unwrap();
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(&db_path)
            .unwrap();
        let newer = std::time::SystemTime::now() + std::time::Duration::from_secs(2);
        file.set_modified(newer).unwrap();
        drop(file);
        for _ in 0..40 {
            if received.load(Ordering::SeqCst) >= 1 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        assert!(
            received.load(Ordering::SeqCst) >= 1,
            "mtime change must deliver exactly the bounded Notification cue"
        );
        assert!(last_id.lock().unwrap().starts_with('o'));
        alive.store(false, Ordering::SeqCst);
    }
}
