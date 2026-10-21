//! 原生历史列表的显示元数据缓存。
//!
//! 只缓存**展示**所需字段（身份、标题、时间），判定依据是 (路径, 大小, 修改时间)。
//! 缓存绝不充当「会话存在/缺失」的证据：缺失记录移除仍要求同一次完整鉴权扫描。
//! 命中仍须经当前持有文件验证首个身份记录，只复用匹配后的显示字段；失败不回退缓存。
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// 条目上限，防止异常增长；正常规模（数千会话）远低于此。
const MAX_ENTRIES: usize = 32_768;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Stamps {
    pub len: u64,
    pub secs: i64,
    pub nanos: u32,
}

impl Stamps {
    pub(crate) fn from_system(len: u64, modified: Option<SystemTime>) -> Option<Self> {
        let (secs, nanos) = match modified?.duration_since(UNIX_EPOCH) {
            Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
            Err(e) => (
                -(e.duration().as_secs() as i64),
                e.duration().subsec_nanos(),
            ),
        };
        Some(Self { len, secs, nanos })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CachedMetadata {
    pub native_id: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub title: String,
    /// 0=Prompt 1=Ai 2=Custom；None 表示标题未知。
    #[serde(default)]
    pub title_priority: Option<u8>,
    #[serde(default)]
    pub updated: Option<String>,
    /// 观察仅覆盖文件前缀，未读到完整尾部。
    #[serde(default)]
    pub metadata_incomplete: bool,
}

/// 当前缓存格式版本。缓存只保存用户主动创建的会话；标题来源规则或收录范围
/// 变化时递增，旧文件按未命中处理，以免旧条目把已排除的会话重新列出来。
const SCHEMA: u32 = 8;

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    entries: HashMap<String, (Stamps, CachedMetadata)>,
}

pub(crate) struct MetadataCache {
    path: Option<PathBuf>,
    entries: HashMap<String, (Stamps, CachedMetadata)>,
    loaded: bool,
    dirty: bool,
}

impl MetadataCache {
    pub(crate) fn with_path(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            entries: HashMap::new(),
            loaded: false,
            dirty: false,
        }
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(path) = self.path.as_ref() else {
            return;
        };
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        // 损坏/超限一律当作空缓存：显示元数据不是权威证据。
        if bytes.len() > 32 * 1024 * 1024 {
            return;
        }
        if let Ok(file) = serde_json::from_slice::<CacheFile>(&bytes) {
            if file.schema == SCHEMA {
                self.entries = file.entries;
            }
        }
    }

    /// 命中要求大小与修改时间完全一致；任何不符都返回 None 触发重新观察。
    pub(crate) fn lookup(&mut self, key: &str, stamps: &Stamps) -> Option<&CachedMetadata> {
        self.ensure_loaded();
        self.entries
            .get(key)
            .filter(|(cached, _)| cached == stamps)
            .map(|(_, metadata)| metadata)
    }

    pub(crate) fn record(&mut self, key: String, stamps: Stamps, metadata: CachedMetadata) {
        self.ensure_loaded();
        self.entries.insert(key, (stamps, metadata));
        self.dirty = true;
        if self.entries.len() > MAX_ENTRIES {
            // 超限时整体丢弃，避免无界增长；下次扫描按未命中重建。
            self.entries.clear();
        }
    }

    /// 未命中时移除过期条目，防止陈旧身份长期滞留。
    pub(crate) fn forget(&mut self, key: &str) {
        self.ensure_loaded();
        if self.entries.remove(key).is_some() {
            self.dirty = true;
        }
    }

    pub(crate) fn flush(&mut self) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let file = CacheFile {
            schema: SCHEMA,
            entries: self.entries.clone(),
        };
        let bytes = serde_json::to_vec(&file)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path.with_extension("json.lock"))?;
        crate::store::acquire_lock_with_label(
            &lock,
            true,
            std::time::Duration::from_millis(100),
            "native-history-metadata",
        )
        .map_err(std::io::Error::other)?;
        crate::store::write_bytes_atomic(&path, &bytes)?;
        self.dirty = false;
        Ok(())
    }
}

static CACHE: LazyLock<parking_lot::Mutex<HashMap<PathBuf, MetadataCache>>> =
    LazyLock::new(|| parking_lot::Mutex::new(HashMap::new()));

pub(crate) fn key(root_key: &str, cli: &str, relative_path: &Path) -> String {
    format!("{root_key}\u{0}{cli}\u{0}{}", relative_path.display())
}

#[cfg(test)]
pub(crate) fn with<R>(f: impl FnOnce(&mut MetadataCache) -> R) -> R {
    with_route(route().as_ref(), f).expect("test cache route")
}

pub(crate) fn empty_bytes() -> Vec<u8> {
    serde_json::to_vec(&CacheFile {
        schema: SCHEMA,
        entries: HashMap::new(),
    })
    .expect("fixed cache schema")
}

pub(crate) fn route() -> Option<crate::session_index_maintenance::IndexRoute> {
    crate::session_index_maintenance::production().route().ok()
}

/// 每次扫描携带自己的 generation；迟到结果不能污染提交后或取消后的缓存。
pub(crate) fn with_route<R>(
    route: Option<&crate::session_index_maintenance::IndexRoute>,
    f: impl FnOnce(&mut MetadataCache) -> R,
) -> Option<R> {
    let route = route?;
    let _guard = match crate::session_index_maintenance::production().write_guard(route) {
        Ok(guard) => guard,
        Err(_) => {
            crate::session_index_maintenance::production().record_failure(route);
            return None;
        }
    };
    let published = crate::session_index_maintenance::production()
        .published()
        .ok()
        .map(|route| route.path("native-history-metadata.json"));
    let mut caches = CACHE.lock();
    let path = route.path("native-history-metadata.json");
    // 保留已发布与当前暂存两份内存，取消时原缓存仍在；旧代不会无界累计。
    caches.retain(|cached_path, _| *cached_path == path || published.as_ref() == Some(cached_path));
    let cache = caches
        .entry(path.clone())
        .or_insert_with(|| MetadataCache::with_path(path));
    Some(f(cache))
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    fn stamps(len: u64, secs: i64) -> Stamps {
        Stamps {
            len,
            secs,
            nanos: 0,
        }
    }

    fn metadata(id: &str) -> CachedMetadata {
        CachedMetadata {
            native_id: id.into(),
            cwd: Some("/repo".into()),
            title: "Title".into(),
            title_priority: Some(0),
            updated: Some("2026-10-01T00:00:00Z".into()),
            metadata_incomplete: true,
        }
    }

    #[test]
    fn Cache_HitRequiresExactStamps_001() {
        let mut cache = MetadataCache::with_path(PathBuf::from("/nonexistent"));
        cache.record("k".into(), stamps(10, 100), metadata("id"));
        assert_eq!(cache.lookup("k", &stamps(10, 100)).unwrap().native_id, "id");
        // 大小或时间任一变化都视为未命中。
        assert!(cache.lookup("k", &stamps(11, 100)).is_none());
        assert!(cache.lookup("k", &stamps(10, 101)).is_none());
        assert!(cache.lookup("other", &stamps(10, 100)).is_none());
    }

    #[test]
    fn Cache_RoundTripsThroughDisk_002() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        let mut cache = MetadataCache::with_path(path.clone());
        cache.record("k".into(), stamps(10, 100), metadata("id"));
        cache.flush().unwrap();
        let mut reloaded = MetadataCache::with_path(path);
        assert_eq!(
            reloaded.lookup("k", &stamps(10, 100)).unwrap().native_id,
            "id"
        );
    }

    #[test]
    fn Cache_CorruptFileIsTreatedAsEmpty_003() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache.json");
        std::fs::write(&path, b"{not json").unwrap();
        let mut cache = MetadataCache::with_path(path);
        assert!(cache.lookup("k", &stamps(1, 1)).is_none());
        // 损坏后仍可正常记录并落盘。
        cache.record("k".into(), stamps(1, 1), metadata("id"));
        cache.flush().unwrap();
    }

    #[test]
    fn Cache_ForgetRemovesStaleEntry_004() {
        let mut cache = MetadataCache::with_path(PathBuf::from("/nonexistent"));
        cache.record("k".into(), stamps(10, 100), metadata("id"));
        cache.forget("k");
        assert!(cache.lookup("k", &stamps(10, 100)).is_none());
    }
}
