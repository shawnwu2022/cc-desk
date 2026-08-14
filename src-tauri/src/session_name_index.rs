//! 会话名称派生索引。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEALTH_BACKOFF_MS: u64 = 30_000;
const DEFAULT_FLUSH_ATTEMPTS: usize = 4;
const DEFAULT_FLUSH_BUDGET: Duration = Duration::from_secs(1);
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
static PRODUCTION_INDEX_HEALTH: OnceLock<Arc<IndexHealth>> = OnceLock::new();

pub(crate) const SESSION_NAME_INDEX_SCHEMA_VERSION: u32 = 1;
pub(crate) const SESSION_NAME_PARSER_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileStamp {
    pub observed_length: u64,
    pub modified_secs: u64,
    pub modified_nanos: u32,
}

impl FileStamp {
    pub(crate) fn read(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Self::from_metadata(&metadata)
    }

    pub(crate) fn from_metadata(metadata: &std::fs::Metadata) -> io::Result<Self> {
        let modified = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?;
        Ok(Self {
            observed_length: metadata.len(),
            modified_secs: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
        })
    }
}

pub(crate) type SessionNameBuckets = BTreeMap<String, BTreeMap<String, SessionNameEntry>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionNameIndex {
    pub schema_version: u32,
    pub parser_version: u32,
    #[serde(default)]
    pub projects: SessionNameBuckets,
}

impl SessionNameIndex {
    pub(crate) fn empty() -> Self {
        Self {
            schema_version: SESSION_NAME_INDEX_SCHEMA_VERSION,
            parser_version: SESSION_NAME_PARSER_VERSION,
            projects: BTreeMap::new(),
        }
    }

    fn has_current_versions(&self) -> bool {
        self.schema_version == SESSION_NAME_INDEX_SCHEMA_VERSION
            && self.parser_version == SESSION_NAME_PARSER_VERSION
    }
}

impl Default for SessionNameIndex {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionNameIndexPaths {
    pub data: PathBuf,
    pub lock: PathBuf,
}

impl SessionNameIndexPaths {
    pub(crate) fn production() -> io::Result<Self> {
        let root = dirs::home_dir()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Home directory not found"))?
            .join(".cc-box");
        Ok(Self {
            data: root.join("session-name-index.json"),
            lock: root.join("session-name-index.json.lock"),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndexLimits {
    pub target_bytes: u64,
    pub soft_bytes: u64,
    pub hard_bytes: u64,
}

impl Default for IndexLimits {
    fn default() -> Self {
        Self {
            target_bytes: 6 * 1024 * 1024,
            soft_bytes: 8 * 1024 * 1024,
            hard_bytes: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RawIndexSnapshot {
    Missing,
    Bytes(Vec<u8>),
    Oversized(FileStamp),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexSnapshot {
    pub index: SessionNameIndex,
    pub raw: RawIndexSnapshot,
    pub needs_compaction: bool,
    pub parse_attempted: bool,
}

impl IndexSnapshot {
    fn empty(raw: RawIndexSnapshot, needs_compaction: bool, parse_attempted: bool) -> Self {
        Self {
            index: SessionNameIndex::empty(),
            raw,
            needs_compaction,
            parse_attempted,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InvalidFingerprint {
    length: u64,
    hash: u64,
}

impl InvalidFingerprint {
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut hash = 0xcbf29ce484222325u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Self {
            length: bytes.len() as u64,
            hash,
        }
    }
}

#[derive(Default)]
struct IndexHealthState {
    invalid: Option<(InvalidFingerprint, u64)>,
    write_backoff_until_ms: u64,
}

pub(crate) struct IndexHealth {
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
    warning_sink: Box<dyn Fn(String) + Send + Sync>,
    state: Mutex<IndexHealthState>,
}

impl IndexHealth {
    pub(crate) fn new(
        clock: impl Fn() -> u64 + Send + Sync + 'static,
        warning_sink: impl Fn(String) + Send + Sync + 'static,
    ) -> Self {
        Self {
            clock: Box::new(clock),
            warning_sink: Box::new(warning_sink),
            state: Mutex::new(IndexHealthState::default()),
        }
    }

    pub(crate) fn production() -> Self {
        Self::new(
            || {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_millis() as u64)
                    .unwrap_or(0)
            },
            |message| log::warn!("{message}"),
        )
    }

    fn should_attempt_invalid(&self, fingerprint: InvalidFingerprint) -> bool {
        let now_ms = (self.clock)();
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        !matches!(
            state.invalid,
            Some((current, retry_at_ms)) if current == fingerprint && now_ms < retry_at_ms
        )
    }

    fn record_invalid(&self, fingerprint: InvalidFingerprint, message: String) {
        let now_ms = (self.clock)();
        let should_warn = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            let throttled = matches!(
                state.invalid,
                Some((current, retry_at_ms)) if current == fingerprint && now_ms < retry_at_ms
            );
            if !throttled {
                state.invalid = Some((fingerprint, now_ms.saturating_add(HEALTH_BACKOFF_MS)));
            }
            !throttled
        };
        if should_warn {
            (self.warning_sink)(message);
        }
    }

    fn clear_invalid(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .invalid = None;
    }

    pub(crate) fn allows_write(&self) -> bool {
        let now_ms = (self.clock)();
        now_ms
            >= self
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .write_backoff_until_ms
    }

    pub(crate) fn record_write_failure(&self, detail: &str) {
        let now_ms = (self.clock)();
        let should_warn = {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if now_ms < state.write_backoff_until_ms {
                false
            } else {
                state.write_backoff_until_ms = now_ms.saturating_add(HEALTH_BACKOFF_MS);
                true
            }
        };
        if should_warn {
            (self.warning_sink)(format!("session name index write failed: {detail}"));
        }
    }

    pub(crate) fn record_write_success(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .write_backoff_until_ms = 0;
    }
}

pub(crate) struct SessionNameIndexStore {
    paths: SessionNameIndexPaths,
    limits: IndexLimits,
    health: Arc<IndexHealth>,
    shared_lock_timeout: Duration,
    flush_budget: Duration,
    flush_attempts: usize,
    probe: Option<Arc<dyn Fn(FlushStage, bool) + Send + Sync>>,
    before_exclusive: Option<Arc<dyn Fn(usize) + Send + Sync>>,
    replace: Arc<dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync>,
    snapshot_read_counter: Option<Arc<AtomicU64>>,
}

impl SessionNameIndexStore {
    pub(crate) fn new(
        paths: SessionNameIndexPaths,
        limits: IndexLimits,
        health: Arc<IndexHealth>,
        shared_lock_timeout: Duration,
    ) -> Self {
        Self {
            paths,
            limits,
            health,
            shared_lock_timeout,
            flush_budget: DEFAULT_FLUSH_BUDGET,
            flush_attempts: DEFAULT_FLUSH_ATTEMPTS,
            probe: None,
            before_exclusive: None,
            replace: Arc::new(|temporary, target| {
                crate::store::replace_file_atomic(temporary, target).map_err(io::Error::other)
            }),
            snapshot_read_counter: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_flush_test_config(
        mut self,
        flush_budget: Duration,
        flush_attempts: usize,
        probe: Option<Arc<dyn Fn(FlushStage, bool) + Send + Sync>>,
        before_exclusive: Option<Arc<dyn Fn(usize) + Send + Sync>>,
        replace: Option<Arc<dyn Fn(&Path, &Path) -> io::Result<()> + Send + Sync>>,
    ) -> Self {
        self.flush_budget = flush_budget;
        self.flush_attempts = flush_attempts;
        self.probe = probe;
        self.before_exclusive = before_exclusive;
        if let Some(replace) = replace {
            self.replace = replace;
        }
        self
    }

    #[cfg(test)]
    pub(crate) fn with_snapshot_read_counter(mut self, counter: Arc<AtomicU64>) -> Self {
        self.snapshot_read_counter = Some(counter);
        self
    }

    #[allow(dead_code)]
    pub(crate) fn production() -> io::Result<Self> {
        Ok(Self::new(
            SessionNameIndexPaths::production()?,
            IndexLimits::default(),
            Arc::clone(PRODUCTION_INDEX_HEALTH.get_or_init(|| Arc::new(IndexHealth::production()))),
            Duration::from_millis(100),
        ))
    }

    pub(crate) fn read_snapshot(&self) -> IndexSnapshot {
        if let Some(counter) = &self.snapshot_read_counter {
            counter.fetch_add(1, Ordering::SeqCst);
        }
        let raw = match self.read_raw_snapshot() {
            Ok(raw) => raw,
            Err(error) => {
                (self.health.warning_sink)(format!("session name index read failed: {error}"));
                return IndexSnapshot::empty(RawIndexSnapshot::Missing, false, false);
            }
        };
        let RawIndexSnapshot::Bytes(bytes) = &raw else {
            return IndexSnapshot::empty(raw, false, false);
        };
        let needs_compaction = bytes.len() as u64 > self.limits.soft_bytes;
        let fingerprint = InvalidFingerprint::from_bytes(bytes);
        if !self.health.should_attempt_invalid(fingerprint) {
            return IndexSnapshot::empty(raw, needs_compaction, false);
        }

        match serde_json::from_slice::<SessionNameIndex>(bytes) {
            Ok(index) if index.has_current_versions() => {
                self.health.clear_invalid();
                IndexSnapshot {
                    index,
                    raw,
                    needs_compaction,
                    parse_attempted: true,
                }
            }
            Ok(_) => {
                self.health.clear_invalid();
                IndexSnapshot::empty(raw, needs_compaction, true)
            }
            Err(error) => {
                self.health.record_invalid(
                    fingerprint,
                    format!("session name index is invalid: {error}"),
                );
                IndexSnapshot::empty(raw, needs_compaction, true)
            }
        }
    }

    pub(crate) fn flush_pending(&self, pending: PendingIndexFlush) -> io::Result<FlushMetrics> {
        let mut metrics = FlushMetrics::default();
        if !self.health.allows_write() {
            metrics.skipped_by_backoff = true;
            return Ok(metrics);
        }

        let flush_started = Instant::now();
        let mut remaining_delta = pending.delta;
        for attempt in 0..self.flush_attempts {
            metrics.attempts += 1;

            let revalidate_started = Instant::now();
            remaining_delta.mutations.retain(|mutation| {
                FileStamp::read(&mutation.path).ok() == Some(mutation.replacement.stamp())
            });
            self.observe_stage(
                &mut metrics,
                FlushStage::Revalidate,
                revalidate_started.elapsed(),
                false,
            );
            if remaining_delta.is_empty() {
                self.health.record_write_success();
                return Ok(metrics);
            }

            let remaining = match remaining_budget(flush_started, self.flush_budget) {
                Ok(remaining) => remaining,
                Err(error) => {
                    self.health.record_write_failure(&error.to_string());
                    return Err(error);
                }
            };
            let raw_started = Instant::now();
            let base_raw = match self
                .read_raw_snapshot_with_timeout(remaining.min(self.shared_lock_timeout))
            {
                Ok(raw) => raw,
                Err(error) => {
                    self.observe_stage(
                        &mut metrics,
                        FlushStage::RawRead,
                        raw_started.elapsed(),
                        false,
                    );
                    self.health.record_write_failure(&error.to_string());
                    return Err(error);
                }
            };
            self.observe_stage(
                &mut metrics,
                FlushStage::RawRead,
                raw_started.elapsed(),
                false,
            );
            metrics.input_bytes = raw_snapshot_len(&base_raw);

            let deserialize_started = Instant::now();
            let latest = index_from_raw_snapshot(&base_raw);
            self.observe_stage(
                &mut metrics,
                FlushStage::Deserialize,
                deserialize_started.elapsed(),
                false,
            );

            let merged =
                merge_delta_measured(latest, &remaining_delta, self.limits, |stage, elapsed| {
                    self.observe_stage(&mut metrics, stage, elapsed, false);
                })?;
            metrics.output_bytes = merged.serialized.len() as u64;
            metrics.applied_mutations += merged.applied_mutations;
            metrics.cleaned_entries += merged.cleaned_entries;
            metrics.evicted_entries += merged.evicted_entries;

            if let Some(parent) = self.paths.lock.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    self.health.record_write_failure(&error.to_string());
                    return Err(error);
                }
            }
            let lock_file = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&self.paths.lock)
            {
                Ok(file) => file,
                Err(error) => {
                    self.health.record_write_failure(&error.to_string());
                    return Err(error);
                }
            };
            let temporary = self.unique_temporary_path();
            let prepare_result =
                self.prepare_temporary(&temporary, &merged.serialized, &mut metrics);
            if let Err(error) = prepare_result {
                let _ = std::fs::remove_file(&temporary);
                self.health.record_write_failure(&error.to_string());
                return Err(error);
            }

            if let Some(before_exclusive) = &self.before_exclusive {
                before_exclusive(attempt);
            }

            let remaining = match remaining_budget(flush_started, self.flush_budget) {
                Ok(remaining) => remaining,
                Err(error) => {
                    let _ = std::fs::remove_file(&temporary);
                    self.health.record_write_failure(&error.to_string());
                    return Err(error);
                }
            };
            let lock_started = Instant::now();
            let lock_result = crate::store::acquire_lock_with_label(
                &lock_file,
                true,
                remaining,
                "session-name-index.json",
            )
            .map_err(io::Error::other);
            self.observe_stage(
                &mut metrics,
                FlushStage::LockWait,
                lock_started.elapsed(),
                false,
            );
            if let Err(error) = lock_result {
                let _ = std::fs::remove_file(&temporary);
                self.health.record_write_failure(&error.to_string());
                return Err(error);
            }

            let exclusive_started = Instant::now();
            let compare_started = Instant::now();
            let raw_matches = self.raw_matches_current_without_lock(&base_raw);
            self.observe_stage(
                &mut metrics,
                FlushStage::LockedRawCompare,
                compare_started.elapsed(),
                true,
            );
            let raw_matches = match raw_matches {
                Ok(raw_matches) => raw_matches,
                Err(error) => {
                    let _ = lock_file.unlock();
                    self.observe_stage(
                        &mut metrics,
                        FlushStage::ExclusiveHold,
                        exclusive_started.elapsed(),
                        true,
                    );
                    let _ = std::fs::remove_file(&temporary);
                    self.health.record_write_failure(&error.to_string());
                    return Err(error);
                }
            };

            if !raw_matches {
                let _ = lock_file.unlock();
                self.observe_stage(
                    &mut metrics,
                    FlushStage::ExclusiveHold,
                    exclusive_started.elapsed(),
                    true,
                );
                let _ = std::fs::remove_file(&temporary);
                continue;
            }

            let replace_started = Instant::now();
            let replace_result = (self.replace)(&temporary, &self.paths.data);
            self.observe_stage(
                &mut metrics,
                FlushStage::Replace,
                replace_started.elapsed(),
                true,
            );
            let _ = lock_file.unlock();
            self.observe_stage(
                &mut metrics,
                FlushStage::ExclusiveHold,
                exclusive_started.elapsed(),
                true,
            );
            if let Err(error) = replace_result {
                let _ = std::fs::remove_file(&temporary);
                self.health.record_write_failure(&error.to_string());
                return Err(error);
            }

            self.health.record_write_success();
            return Ok(metrics);
        }

        let error = io::Error::new(
            io::ErrorKind::WouldBlock,
            "session name index whole-file CAS exhausted",
        );
        self.health.record_write_failure(&error.to_string());
        Err(error)
    }

    fn observe_stage(
        &self,
        metrics: &mut FlushMetrics,
        stage: FlushStage,
        elapsed: Duration,
        exclusive_lock_held: bool,
    ) {
        metrics.add_stage(stage, elapsed);
        if let Some(probe) = &self.probe {
            probe(stage, exclusive_lock_held);
        }
    }

    fn prepare_temporary(
        &self,
        temporary: &Path,
        serialized: &[u8],
        metrics: &mut FlushMetrics,
    ) -> io::Result<()> {
        if let Some(parent) = temporary.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let write_started = Instant::now();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)?;
        use std::io::Write;
        file.write_all(serialized)?;
        self.observe_stage(
            metrics,
            FlushStage::TempWrite,
            write_started.elapsed(),
            false,
        );
        let sync_started = Instant::now();
        file.sync_all()?;
        self.observe_stage(metrics, FlushStage::Sync, sync_started.elapsed(), false);
        drop(file);
        Ok(())
    }

    fn unique_temporary_path(&self) -> PathBuf {
        let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let file_name = self
            .paths
            .data
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "session-name-index.json".to_string());
        self.paths.data.with_file_name(format!(
            "{file_name}.tmp.{}.{}",
            std::process::id(),
            sequence
        ))
    }

    fn read_raw_snapshot(&self) -> io::Result<RawIndexSnapshot> {
        self.read_raw_snapshot_with_timeout(self.shared_lock_timeout)
    }

    fn read_raw_snapshot_with_timeout(
        &self,
        lock_timeout: Duration,
    ) -> io::Result<RawIndexSnapshot> {
        if let Some(parent) = self.paths.lock.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock_file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.paths.lock)?;
        crate::store::acquire_lock_with_label(
            &lock_file,
            false,
            lock_timeout,
            "session-name-index.json",
        )
        .map_err(io::Error::other)?;

        let result = self.read_raw_without_lock();
        let _ = lock_file.unlock();
        result
    }

    fn read_raw_without_lock(&self) -> io::Result<RawIndexSnapshot> {
        let file = match File::open(&self.paths.data) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RawIndexSnapshot::Missing);
            }
            Err(error) => return Err(error),
        };
        let metadata = file.metadata()?;
        if metadata.len() > self.limits.hard_bytes {
            return Ok(RawIndexSnapshot::Oversized(FileStamp::from_metadata(
                &metadata,
            )?));
        }

        let expected_length = metadata.len();
        let mut bytes = Vec::with_capacity(expected_length as usize);
        file.take(expected_length).read_to_end(&mut bytes)?;
        Ok(RawIndexSnapshot::Bytes(bytes))
    }

    fn raw_matches_current_without_lock(&self, base: &RawIndexSnapshot) -> io::Result<bool> {
        match base {
            RawIndexSnapshot::Missing => match File::open(&self.paths.data) {
                Ok(_) => Ok(false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
                Err(error) => Err(error),
            },
            RawIndexSnapshot::Oversized(expected) => match std::fs::metadata(&self.paths.data) {
                Ok(metadata) => Ok(FileStamp::from_metadata(&metadata)? == *expected),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error),
            },
            RawIndexSnapshot::Bytes(expected) => {
                let mut file = match File::open(&self.paths.data) {
                    Ok(file) => file,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                    Err(error) => return Err(error),
                };
                if file.metadata()?.len() != expected.len() as u64 {
                    return Ok(false);
                }

                let mut compared = 0usize;
                let mut buffer = [0u8; 64 * 1024];
                while compared < expected.len() {
                    let wanted = buffer.len().min(expected.len() - compared);
                    file.read_exact(&mut buffer[..wanted])?;
                    if buffer[..wanted] != expected[compared..compared + wanted] {
                        return Ok(false);
                    }
                    compared += wanted;
                }
                Ok(true)
            }
        }
    }
}

fn remaining_budget(started: Instant, budget: Duration) -> io::Result<Duration> {
    budget.checked_sub(started.elapsed()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "session name index flush budget exhausted",
        )
    })
}

fn raw_snapshot_len(raw: &RawIndexSnapshot) -> u64 {
    match raw {
        RawIndexSnapshot::Missing => 0,
        RawIndexSnapshot::Bytes(bytes) => bytes.len() as u64,
        RawIndexSnapshot::Oversized(stamp) => stamp.observed_length,
    }
}

fn index_from_raw_snapshot(raw: &RawIndexSnapshot) -> SessionNameIndex {
    let RawIndexSnapshot::Bytes(bytes) = raw else {
        return SessionNameIndex::empty();
    };
    serde_json::from_slice::<SessionNameIndex>(bytes)
        .ok()
        .filter(SessionNameIndex::has_current_versions)
        .unwrap_or_else(SessionNameIndex::empty)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionNameEntry {
    pub name: String,
    pub observed_length: u64,
    pub modified_secs: u64,
    pub modified_nanos: u32,
    pub cached_at_ms: u64,
}

impl SessionNameEntry {
    fn stamp(&self) -> FileStamp {
        FileStamp {
            observed_length: self.observed_length,
            modified_secs: self.modified_secs,
            modified_nanos: self.modified_nanos,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolutionKind {
    ExactHit,
    FullRebuild,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NameResolution {
    pub name: String,
    pub kind: ResolutionKind,
    pub jsonl_bytes_read: u64,
    pub replacement: Option<SessionNameEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IndexMutation {
    pub project_key: String,
    pub file_name: String,
    pub path: PathBuf,
    pub base: Option<SessionNameEntry>,
    pub replacement: SessionNameEntry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectorySnapshot {
    pub project_key: String,
    pub base_bucket: BTreeMap<String, SessionNameEntry>,
    pub live_file_names: BTreeSet<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SessionNameIndexDelta {
    pub mutations: Vec<IndexMutation>,
    pub directories: Vec<DirectorySnapshot>,
    pub request_compaction: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MergeOutcome {
    pub index: SessionNameIndex,
    pub serialized: Vec<u8>,
    pub applied_mutations: usize,
    pub cleaned_entries: usize,
    pub evicted_entries: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlushStage {
    Revalidate,
    RawRead,
    Deserialize,
    Merge,
    Compaction,
    Serialize,
    TempWrite,
    Sync,
    LockWait,
    LockedRawCompare,
    Replace,
    ExclusiveHold,
}

impl FlushStage {
    /// 所有阶段（基准输出顺序）；仅 tests 的 flush_metric_stages 引用。
    #[allow(dead_code)]
    pub(crate) const ALL: [FlushStage; 12] = [
        FlushStage::Revalidate,
        FlushStage::RawRead,
        FlushStage::Deserialize,
        FlushStage::Merge,
        FlushStage::Compaction,
        FlushStage::Serialize,
        FlushStage::TempWrite,
        FlushStage::Sync,
        FlushStage::LockWait,
        FlushStage::LockedRawCompare,
        FlushStage::Replace,
        FlushStage::ExclusiveHold,
    ];

    /// 阶段标签（基准输出用）；仅 tests 的 flush_metric_stages 引用。
    #[allow(dead_code)]
    pub(crate) fn label(self) -> &'static str {
        match self {
            FlushStage::Revalidate => "revalidate",
            FlushStage::RawRead => "raw_read",
            FlushStage::Deserialize => "deserialize",
            FlushStage::Merge => "merge",
            FlushStage::Compaction => "compaction",
            FlushStage::Serialize => "serialize",
            FlushStage::TempWrite => "temp_write",
            FlushStage::Sync => "sync",
            FlushStage::LockWait => "lock_wait",
            FlushStage::LockedRawCompare => "locked_raw_compare",
            FlushStage::Replace => "replace",
            FlushStage::ExclusiveHold => "exclusive_hold",
        }
    }

    /// 指回该阶段在 metrics 里的累计 Duration 字段（消除 add_stage / 镜像的两处 switch）。
    pub(crate) fn field_mut<'a>(self, metrics: &'a mut FlushMetrics) -> &'a mut Duration {
        match self {
            FlushStage::Revalidate => &mut metrics.revalidate,
            FlushStage::RawRead => &mut metrics.raw_read,
            FlushStage::Deserialize => &mut metrics.deserialize,
            FlushStage::Merge => &mut metrics.merge,
            FlushStage::Compaction => &mut metrics.compaction,
            FlushStage::Serialize => &mut metrics.serialize,
            FlushStage::TempWrite => &mut metrics.temp_write,
            FlushStage::Sync => &mut metrics.sync,
            FlushStage::LockWait => &mut metrics.lock_wait,
            FlushStage::LockedRawCompare => &mut metrics.locked_raw_compare,
            FlushStage::Replace => &mut metrics.replace,
            FlushStage::ExclusiveHold => &mut metrics.exclusive_hold,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlushMetrics {
    pub revalidate: Duration,
    pub raw_read: Duration,
    pub deserialize: Duration,
    pub merge: Duration,
    pub compaction: Duration,
    pub serialize: Duration,
    pub temp_write: Duration,
    pub sync: Duration,
    pub lock_wait: Duration,
    pub locked_raw_compare: Duration,
    pub replace: Duration,
    pub exclusive_hold: Duration,
    pub attempts: usize,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub applied_mutations: usize,
    pub cleaned_entries: usize,
    pub evicted_entries: usize,
    pub skipped_by_backoff: bool,
}

impl FlushMetrics {
    fn add_stage(&mut self, stage: FlushStage, elapsed: Duration) {
        *stage.field_mut(self) += elapsed;
    }
}

impl SessionNameIndexDelta {
    fn is_empty(&self) -> bool {
        self.mutations.is_empty() && self.directories.is_empty() && !self.request_compaction
    }
}

#[allow(dead_code)] // 纯函数测试入口；生产 flush 通过同一 measured 内核调用。
pub(crate) fn merge_delta_in_memory(
    index: SessionNameIndex,
    delta: &SessionNameIndexDelta,
    limits: IndexLimits,
) -> io::Result<MergeOutcome> {
    merge_delta_measured(index, delta, limits, |_, _| {})
}

fn merge_delta_measured(
    mut index: SessionNameIndex,
    delta: &SessionNameIndexDelta,
    limits: IndexLimits,
    mut observe: impl FnMut(FlushStage, Duration),
) -> io::Result<MergeOutcome> {
    let merge_started = Instant::now();
    let mut applied_mutations = 0usize;
    for mutation in &delta.mutations {
        let bucket = index
            .projects
            .entry(mutation.project_key.clone())
            .or_default();
        if bucket.get(&mutation.file_name) == mutation.base.as_ref() {
            bucket.insert(mutation.file_name.clone(), mutation.replacement.clone());
            applied_mutations += 1;
        }
    }

    let mut cleaned_entries = 0usize;
    for directory in &delta.directories {
        let Some(bucket) = index.projects.get_mut(&directory.project_key) else {
            continue;
        };
        if bucket != &directory.base_bucket {
            continue;
        }
        let before = bucket.len();
        bucket.retain(|file_name, _| directory.live_file_names.contains(file_name));
        cleaned_entries += before.saturating_sub(bucket.len());
    }
    index.projects.retain(|_, bucket| !bucket.is_empty());
    observe(FlushStage::Merge, merge_started.elapsed());

    let serialize_started = Instant::now();
    let mut serialized = serde_json::to_vec(&index).map_err(io::Error::other)?;
    observe(FlushStage::Serialize, serialize_started.elapsed());
    let mut evicted_entries = 0usize;
    if delta.request_compaction || serialized.len() as u64 > limits.soft_bytes {
        while serialized.len() as u64 > limits.target_bytes {
            let compaction_started = Instant::now();
            let mut candidates = index
                .projects
                .iter()
                .flat_map(|(project_key, bucket)| {
                    bucket.iter().map(move |(file_name, entry)| {
                        (entry.cached_at_ms, project_key.clone(), file_name.clone())
                    })
                })
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                observe(FlushStage::Compaction, compaction_started.elapsed());
                break;
            }
            candidates.sort();

            let excess = serialized
                .len()
                .saturating_sub(limits.target_bytes as usize);
            let proportional = candidates
                .len()
                .saturating_mul(excess)
                .div_ceil(serialized.len().max(1));
            let batch_size = proportional.max(32).min(candidates.len());
            for (_, project_key, file_name) in candidates.into_iter().take(batch_size) {
                if let Some(bucket) = index.projects.get_mut(&project_key) {
                    if bucket.remove(&file_name).is_some() {
                        evicted_entries += 1;
                    }
                }
            }
            index.projects.retain(|_, bucket| !bucket.is_empty());
            observe(FlushStage::Compaction, compaction_started.elapsed());
            let serialize_started = Instant::now();
            serialized = serde_json::to_vec(&index).map_err(io::Error::other)?;
            observe(FlushStage::Serialize, serialize_started.elapsed());
        }
    }

    Ok(MergeOutcome {
        index,
        serialized,
        applied_mutations,
        cleaned_entries,
        evicted_entries,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ResolutionStats {
    pub exact_hits: u64,
    pub full_rebuilds: u64,
    pub jsonl_bytes_read: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PendingIndexFlush {
    pub base_raw: RawIndexSnapshot,
    pub delta: SessionNameIndexDelta,
}

#[derive(Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct IndexedResult<T> {
    pub value: T,
    pub pending_flush: Option<PendingIndexFlush>,
    pub stats: ResolutionStats,
}

pub(crate) struct SessionNameResolver {
    snapshot: IndexSnapshot,
    cached_at_ms: u64,
    stats: ResolutionStats,
    delta: SessionNameIndexDelta,
}

impl SessionNameResolver {
    pub(crate) fn new(snapshot: IndexSnapshot, cached_at_ms: u64) -> Self {
        let request_compaction = snapshot.needs_compaction;
        Self {
            snapshot,
            cached_at_ms,
            stats: ResolutionStats::default(),
            delta: SessionNameIndexDelta {
                request_compaction,
                ..SessionNameIndexDelta::default()
            },
        }
    }

    pub(crate) fn resolve(
        &mut self,
        project_dir: &Path,
        path: &Path,
        initial_stamp: FileStamp,
    ) -> NameResolution {
        let project_key = normalized_project_key(project_dir);
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let base = self
            .snapshot
            .index
            .projects
            .get(&project_key)
            .and_then(|bucket| bucket.get(&file_name))
            .cloned();
        let resolution =
            resolve_session_name_at(path, initial_stamp, base.as_ref(), self.cached_at_ms);

        match resolution.kind {
            ResolutionKind::ExactHit => {
                self.stats.exact_hits = self.stats.exact_hits.saturating_add(1);
            }
            ResolutionKind::FullRebuild => {
                self.stats.full_rebuilds = self.stats.full_rebuilds.saturating_add(1);
            }
        }
        self.stats.jsonl_bytes_read = self
            .stats
            .jsonl_bytes_read
            .saturating_add(resolution.jsonl_bytes_read);

        if let Some(replacement) = resolution.replacement.clone() {
            self.delta.mutations.push(IndexMutation {
                project_key,
                file_name,
                path: path.to_path_buf(),
                base,
                replacement,
            });
        }
        resolution
    }

    pub(crate) fn record_directory(
        &mut self,
        project_dir: &Path,
        live_file_names: BTreeSet<String>,
        complete: bool,
    ) {
        if !complete {
            return;
        }
        let project_key = normalized_project_key(project_dir);
        let Some(base_bucket) = self.snapshot.index.projects.get(&project_key) else {
            return;
        };
        if base_bucket
            .keys()
            .all(|file_name| live_file_names.contains(file_name))
        {
            return;
        }
        self.delta.directories.push(DirectorySnapshot {
            project_key,
            base_bucket: base_bucket.clone(),
            live_file_names,
        });
    }

    pub(crate) fn stats(&self) -> ResolutionStats {
        self.stats
    }

    pub(crate) fn finish(self) -> Option<PendingIndexFlush> {
        if self.delta.is_empty() {
            return None;
        }
        Some(PendingIndexFlush {
            base_raw: self.snapshot.raw,
            delta: self.delta,
        })
    }
}

fn normalized_project_key(project_dir: &Path) -> String {
    crate::store::normalize_path_str(&project_dir.to_string_lossy())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FullNameParse {
    pub name: String,
    pub jsonl_bytes_read: u64,
}

pub(crate) fn apply_session_name_value(
    json: &serde_json::Value,
    custom_title: &mut Option<String>,
    first_user_message: &mut Option<String>,
) {
    if json.get("type").and_then(|value| value.as_str()) == Some("custom-title") {
        if let Some(title) = json.get("customTitle").and_then(|value| value.as_str()) {
            *custom_title = Some(title.to_string());
        }
    }

    if json.get("type").and_then(|value| value.as_str()) != Some("user")
        || first_user_message.is_some()
    {
        return;
    }
    let Some(content) = json
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
    else {
        return;
    };
    let is_meta = json
        .get("isMeta")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    if is_meta || content.trim_start().starts_with('<') {
        return;
    }

    let truncated = content.chars().take(50).collect::<String>();
    *first_user_message = if content.chars().count() > 50 {
        Some(format!("{truncated}..."))
    } else {
        Some(content.to_string())
    };
}

pub(crate) fn parse_session_name_full(path: &Path) -> io::Result<FullNameParse> {
    let file = File::open(path)?;
    let mut reader = io::BufReader::new(file);
    let mut line = Vec::new();
    let mut bytes_read = 0u64;
    let mut custom_title = None;
    let mut first_user_message = None;

    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        bytes_read = bytes_read.saturating_add(read as u64);
        let Ok(text) = std::str::from_utf8(&line) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            continue;
        };
        apply_session_name_value(&value, &mut custom_title, &mut first_user_message);
    }

    Ok(FullNameParse {
        name: custom_title
            .or(first_user_message)
            .unwrap_or_else(|| "Unnamed session".to_string()),
        jsonl_bytes_read: bytes_read,
    })
}

pub(crate) fn resolve_session_name_at(
    path: &Path,
    initial_stamp: FileStamp,
    cached: Option<&SessionNameEntry>,
    cached_at_ms: u64,
) -> NameResolution {
    let stamp_before = FileStamp::read(path).ok();
    if stamp_before == Some(initial_stamp) {
        if let Some(cached) = cached.filter(|entry| entry.stamp() == initial_stamp) {
            return NameResolution {
                name: cached.name.clone(),
                kind: ResolutionKind::ExactHit,
                jsonl_bytes_read: 0,
                replacement: None,
            };
        }
    }

    let Ok(parsed) = parse_session_name_full(path) else {
        return NameResolution {
            name: "Unnamed session".to_string(),
            kind: ResolutionKind::FullRebuild,
            jsonl_bytes_read: 0,
            replacement: None,
        };
    };
    let replacement =
        (FileStamp::read(path).ok() == Some(initial_stamp)).then(|| SessionNameEntry {
            name: parsed.name.clone(),
            observed_length: initial_stamp.observed_length,
            modified_secs: initial_stamp.modified_secs,
            modified_nanos: initial_stamp.modified_nanos,
            cached_at_ms,
        });

    NameResolution {
        name: parsed.name,
        kind: ResolutionKind::FullRebuild,
        jsonl_bytes_read: parsed.jsonl_bytes_read,
        replacement,
    }
}
