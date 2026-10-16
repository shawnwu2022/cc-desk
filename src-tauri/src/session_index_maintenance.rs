//! 可再生索引的分代事务；原生会话和 Desk 用户元数据不在此模块的写入范围。
use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::SafeError;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io;
use std::io::Read;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexRoute {
    root: PathBuf,
    generation: Option<String>,
}
impl IndexRoute {
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        match &self.generation {
            Some(generation) => self
                .root
                .join("session-index-generations")
                .join(generation)
                .join(name),
            None => self.root.join(name),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MaintenanceRequest {
    pub(crate) id: String,
    pub(crate) action: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MaintenanceStatus {
    pub(crate) id: String,
    pub(crate) state: &'static str,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    generation: String,
}
struct Transaction {
    owner: CallerIdentity,
    id: String,
    route: IndexRoute,
    _lease: File,
    failed: bool,
    base_manifest: Option<Vec<u8>>,
}
#[derive(Default)]
struct State {
    active: Option<Transaction>,
    completed: Vec<(CallerIdentity, MaintenanceStatus)>,
}
const MAX_RECEIPTS: usize = 4096;
fn remember(state: &mut State, owner: &CallerIdentity, status: MaintenanceStatus) {
    if let Some((_, previous)) = state
        .completed
        .iter_mut()
        .find(|(saved_owner, previous)| saved_owner == owner && previous.id == status.id)
    {
        *previous = status;
    } else {
        state.completed.push((owner.clone(), status));
    }
}
pub(crate) struct IndexMaintenance {
    root: PathBuf,
    state: Mutex<State>,
}
pub(crate) struct IndexWriteGuard {
    _publish: File,
    _transaction: Option<File>,
}
impl IndexMaintenance {
    pub(crate) fn new(root: PathBuf) -> Self {
        Self {
            root,
            state: Mutex::new(State::default()),
        }
    }
    fn lock(&self, name: &str, exclusive: bool) -> io::Result<File> {
        std::fs::create_dir_all(&self.root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(name))?;
        crate::store::acquire_lock_with_label(
            &file,
            exclusive,
            Duration::from_millis(100),
            "session-index-maintenance",
        )
        .map_err(io::Error::other)?;
        Ok(file)
    }
    fn manifest_bytes(&self) -> io::Result<Option<Vec<u8>>> {
        let path = self.root.join("session-index-generation.json");
        match File::open(path).and_then(|file| {
            let mut bytes = Vec::new();
            file.take(1025).read_to_end(&mut bytes)?;
            Ok(bytes)
        }) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(bytes) => {
                if bytes.len() > 1024 {
                    return Err(io::Error::other("invalid index manifest"));
                }
                Ok(Some(bytes))
            }
        }
    }
    pub(crate) fn published(&self) -> io::Result<IndexRoute> {
        let generation = match self.manifest_bytes()? {
            None => None,
            Some(bytes) => {
                let manifest: Manifest = serde_json::from_slice(&bytes)?;
                if manifest.schema_version != 1
                    || uuid::Uuid::parse_str(&manifest.generation).is_err()
                {
                    return Err(io::Error::other("invalid index manifest"));
                }
                Some(manifest.generation)
            }
        };
        Ok(IndexRoute {
            root: self.root.clone(),
            generation,
        })
    }
    pub(crate) fn route(&self) -> io::Result<IndexRoute> {
        let state = self.state.lock();
        if let Some(active) = &state.active {
            return Ok(active.route.clone());
        }
        self.published()
    }
    pub(crate) fn is_staging(&self, route: &IndexRoute) -> bool {
        self.state
            .lock()
            .active
            .as_ref()
            .is_some_and(|active| active.route == *route)
    }
    pub(crate) fn record_failure(&self, route: &IndexRoute) {
        if let Some(active) = self
            .state
            .lock()
            .active
            .as_mut()
            .filter(|active| active.route == *route)
        {
            active.failed = true;
        }
    }
    pub(crate) fn release_revoked(&self, authorized: impl Fn(&CallerIdentity) -> bool) {
        let mut state = self.state.lock();
        if state
            .active
            .as_ref()
            .is_some_and(|active| !authorized(&active.owner))
        {
            // 新 document 只能丢弃失权的暂存事务，不能提交它。
            state.active = None;
        }
        state.completed.retain(|(owner, _)| authorized(owner));
    }
    /// 持共享发布锁直到写回完成，避免检查 generation 后发生提交。
    pub(crate) fn write_guard(&self, route: &IndexRoute) -> io::Result<IndexWriteGuard> {
        let state = self.state.lock();
        let transaction = if let Some(active) = &state.active {
            if active.route != *route {
                return Err(io::Error::other("stale index generation"));
            }
            None
        } else {
            Some(self.lock("session-index-transaction.lock", false)?)
        };
        let guard = self.lock("session-index-publish.lock", false)?;
        if route.root != self.root
            || (!state
                .active
                .as_ref()
                .is_some_and(|active| active.route == *route)
                && self.published()? != *route)
        {
            return Err(io::Error::other("stale index generation"));
        }
        Ok(IndexWriteGuard {
            _publish: guard,
            _transaction: transaction,
        })
    }
    #[cfg(test)]
    pub(crate) fn apply(
        &self,
        owner: &CallerIdentity,
        id: &str,
        action: &str,
    ) -> Result<MaintenanceStatus, SafeError> {
        self.apply_checked(owner, id, action, || Ok(()))
    }
    pub(crate) fn apply_checked(
        &self,
        owner: &CallerIdentity,
        id: &str,
        action: &str,
        check: impl Fn() -> Result<(), SafeError>,
    ) -> Result<MaintenanceStatus, SafeError> {
        if owner.window_label != "main" {
            return Err(error("FORBIDDEN"));
        }
        if id.is_empty()
            || id.len() > 128
            || id.chars().any(char::is_control)
            || !matches!(action, "begin" | "commit" | "cancel" | "status")
        {
            return Err(error("INVALID_REQUEST"));
        }
        let mut state = self.state.lock();
        check()?;
        if let Some((_, previous)) = state
            .completed
            .iter()
            .find(|(saved_owner, previous)| saved_owner == owner && previous.id == id)
        {
            if matches!(action, "begin" | "status")
                || action
                    == if previous.state == "committed" {
                        "commit"
                    } else {
                        "cancel"
                    }
            {
                return Ok(previous.clone());
            }
            return Err(error("MAINTENANCE_CONFLICT"));
        }
        if action == "status"
            && state
                .completed
                .iter()
                .any(|(saved_owner, previous)| saved_owner != owner && previous.id == id)
        {
            return Err(error("FORBIDDEN"));
        }
        let known_active = state
            .active
            .as_ref()
            .is_some_and(|active| active.owner == *owner && active.id == id);
        if !known_active
            && action != "status"
            && state.completed.len() + usize::from(state.active.is_some()) >= MAX_RECEIPTS
        {
            return Err(error("MAINTENANCE_BUSY"));
        }
        if action == "begin" {
            if let Some(active) = &state.active {
                if active.owner != *owner {
                    return Err(error("FORBIDDEN"));
                }
                if active.id != id {
                    return Err(error("MAINTENANCE_CONFLICT"));
                }
            } else {
                // 独立事务锁阻止另一个进程同时创建维护事务，不阻塞进程启动或普通索引读取。
                let lease = self
                    .lock("session-index-transaction.lock", true)
                    .map_err(|_| error("MAINTENANCE_BUSY"))?;
                let base_manifest = self
                    .manifest_bytes()
                    .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
                let route = IndexRoute {
                    root: self.root.clone(),
                    generation: Some(uuid::Uuid::new_v4().to_string()),
                };
                let parent = route
                    .path("native-history-metadata.json")
                    .parent()
                    .unwrap()
                    .to_owned();
                std::fs::create_dir_all(parent.parent().expect("fixed generation parent"))
                    .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
                // UUID 碰撞也必须拒绝；不能复用已有代并覆盖已发布索引。
                std::fs::create_dir(&parent).map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
                crate::store::write_bytes_atomic(
                    &route.path("native-history-metadata.json"),
                    &crate::cli::native_projection::cache::empty_bytes(),
                )
                .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
                let legacy =
                    serde_json::to_vec(&crate::session_name_index::SessionNameIndex::empty())
                        .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
                crate::store::write_bytes_atomic(&route.path("session-name-index.json"), &legacy)
                    .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
                check()?;
                state.active = Some(Transaction {
                    owner: owner.clone(),
                    id: id.into(),
                    route,
                    _lease: lease,
                    failed: false,
                    base_manifest,
                });
            }
            return Ok(MaintenanceStatus {
                id: id.into(),
                state: "staging",
            });
        }
        let Some(active) = state.active.as_ref() else {
            return if action == "cancel" {
                // 取消尚未到达的 begin：墓碑必须保留到 document 失权，不能被下一事务覆盖。
                let status = MaintenanceStatus {
                    id: id.into(),
                    state: "cancelled",
                };
                remember(&mut state, owner, status.clone());
                Ok(status)
            } else if action == "status" {
                Ok(MaintenanceStatus {
                    id: id.into(),
                    state: "absent",
                })
            } else {
                Err(error("MAINTENANCE_CONFLICT"))
            };
        };
        if active.owner != *owner {
            return Err(error("FORBIDDEN"));
        }
        if active.id != id {
            return if action == "cancel" {
                let status = MaintenanceStatus {
                    id: id.into(),
                    state: "cancelled",
                };
                remember(&mut state, owner, status.clone());
                Ok(status)
            } else if action == "status" {
                Ok(MaintenanceStatus {
                    id: id.into(),
                    state: "absent",
                })
            } else {
                Err(error("MAINTENANCE_CONFLICT"))
            };
        }
        if action == "status" {
            // 写回执不确定时，只读取发布指针协调；不重放提交或回滚。
            let unchanged = self
                .manifest_bytes()
                .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?
                == active.base_manifest;
            let committed = !unchanged
                && self
                    .published()
                    .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?
                    == active.route;
            let status = MaintenanceStatus {
                id: id.into(),
                state: if committed { "committed" } else { "staging" },
            };
            if committed {
                state.active = None;
                remember(&mut state, owner, status.clone());
            }
            return Ok(status);
        }
        if action == "commit" {
            let _publish = self
                .lock("session-index-publish.lock", true)
                .map_err(|_| error("MAINTENANCE_BUSY"))?;
            check()?;
            if active.failed {
                return Err(error("MAINTENANCE_UNAVAILABLE"));
            }
            let manifest = Manifest {
                schema_version: 1,
                generation: active.route.generation.clone().unwrap(),
            };
            let bytes =
                serde_json::to_vec(&manifest).map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
            crate::store::write_bytes_atomic(
                &self.root.join("session-index-generation.json"),
                &bytes,
            )
            .map_err(|_| error("MAINTENANCE_UNAVAILABLE"))?;
        }
        let status = MaintenanceStatus {
            id: id.into(),
            state: if action == "commit" {
                "committed"
            } else {
                "cancelled"
            },
        };
        state.active = None;
        remember(&mut state, owner, status.clone());
        Ok(status)
    }
}

static MAINTENANCE: LazyLock<IndexMaintenance> = LazyLock::new(|| {
    #[cfg(test)]
    let root = std::env::temp_dir().join(format!("cc-box-indexes-{}", std::process::id()));
    #[cfg(not(test))]
    let root = dirs::home_dir()
        .expect("native runtime requires home")
        .join(".cc-box");
    IndexMaintenance::new(root)
});
pub(crate) fn production() -> &'static IndexMaintenance {
    &MAINTENANCE
}

#[cfg(test)]
#[path = "tests/session_index_maintenance.rs"]
mod tests;
