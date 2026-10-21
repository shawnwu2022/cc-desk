//! Read-only directory capabilities. Ambient paths are accepted only at backend root admission.
//! Every descendant open is relative to the retained cap-std handle, never path.join + fs::read.
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

#[path = "compressed.rs"]
mod compressed;
use compressed::{is_zstd_history, Mode};
#[path = "rename_fs.rs"]
mod rename_fs;
pub(crate) use rename_fs::{PinnedTranscript, PrivateTranscript};

pub(crate) type ReadResult<T> = Result<T, &'static str>;
type HistoryObserver<'a> = dyn FnMut(&[u8]) -> ReadResult<bool> + 'a;
pub(crate) type TitleTail = (Vec<u8>, Vec<u8>, bool);
/// Codex 头窗口观察结果：头字节、尾字节、是否只观察到文件前缀。
pub(crate) type CodexHead = (Vec<u8>, Vec<u8>, bool);
/// Codex 身份与首条用户消息的头窗口上限。实测本机 2131 个 rollout 在 192KiB
/// 窗口下可覆盖 1769 条（83%）用户的标题来源，总头窗口 IO 约 150MiB。
const CODEX_HEAD_WINDOW: usize = 192 * 1024;
/// Claude 有界标题尾窗。CLI 把 `ai-title`/`custom-title` 写在转录末尾附近；
/// 实测本机 627 个转录中 37 个带 ai-title，距 EOF 中位 20KiB、p90 33KiB。
/// 64KiB 覆盖其中 35 个（95%）。窗口越大越准，但冷扫描 IO 随之上升；首次
/// 该比例是标题覆盖证据，不是扫描耗时收益；Claude 每次读取仍观察头部和尾部。
const CLAUDE_TITLE_TAIL: usize = 64 * 1024;
#[derive(Clone, Copy)]
pub(crate) struct Limits {
    pub file_bytes: usize,
    pub total_bytes: usize,
    pub entries: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            file_bytes: 2 * 1024 * 1024,
            // 真实 Codex 根（约 2130 个 rollout，含 archived_sessions）一次完整
            // 元数据采样：头窗口最深 192KiB，跳过注入上下文后多数文件需读到窗口
            // 上限附近，实测约 340–366MiB。512MiB 留出余量；Codex 缓存命中后
            // 仍需重读当前身份，Claude 没有这层显示缓存。120 秒协作截止与
            // entries 上限仍是硬护栏。
            total_bytes: 512 * 1024 * 1024,
            entries: 16384,
        }
    }
}
pub(crate) struct Budget {
    file_bytes: usize,
    bytes_left: usize,
    entries_left: usize,
    started: Instant,
    history_metadata_incomplete: bool,
    history_read_failures: std::collections::BTreeSet<&'static str>,
    history_omitted_sessions: std::collections::BTreeSet<uuid::Uuid>,
    history_anonymous_omission: bool,
}
impl Budget {
    pub(crate) fn new(limits: Limits) -> Self {
        let maximum = Limits::default();
        Self {
            file_bytes: limits.file_bytes.min(maximum.file_bytes),
            bytes_left: limits.total_bytes.min(maximum.total_bytes),
            entries_left: limits.entries.min(maximum.entries),
            started: Instant::now(),
            history_metadata_incomplete: false,
            history_read_failures: Default::default(),
            history_omitted_sessions: Default::default(),
            history_anonymous_omission: false,
        }
    }
    pub(crate) fn checkpoint(&self) -> ReadResult<()> {
        // Cooperative deadline between operations; not a promise to interrupt OS filesystem I/O.
        // 首次冷扫描（真实 ~/.codex，2130 个 rollout，含 192KiB 头窗口富化）实测约
        // 40 秒，属于旧实现的历史记录；当前缓存身份重读策略不能复用该计时。
        // 512 MiB / 16384 entries 与 120 秒仍是独立硬护栏。
        if self.started.elapsed() > Duration::from_secs(120) {
            return Err("SOURCE_BUDGET_EXCEEDED");
        }
        Ok(())
    }
    pub(crate) fn history_metadata_incomplete(&self) -> bool {
        self.history_metadata_incomplete
    }
    /// 缓存命中不打开文件；行内恢复的「观察不完整」事实由此回到来源级标记。
    pub(crate) fn retain_history_metadata_incomplete(&mut self) {
        self.history_metadata_incomplete = true;
    }
    pub(crate) fn optional_title_available(&self, header_bytes: usize, tail_bytes: usize) -> bool {
        // 可选标题读写不设短时门槛：真实冷扫描必然远超任何秒级窗口，过早放弃
        // 会让全部行的 CLI 标题静默失效（实测即为此前 ai-title 永不可见的根因）。
        // 代价由聚合字节预算与 entries 上限约束，协作截止仍由 checkpoint 兜底。
        self.entries_left > 0
            && header_bytes.saturating_add(tail_bytes) <= self.bytes_left
            && header_bytes.saturating_mul(2).saturating_add(tail_bytes) <= self.file_bytes
    }
    pub(crate) fn history_read_failures(&self) -> Vec<&'static str> {
        self.history_read_failures.iter().copied().collect()
    }
    // Only content/read failures of an individual history entry are isolatable.
    // Scope, identity, permission, links, ambiguity and aggregate limits still fail closed.
    pub(crate) fn omit_history_entry(&mut self, code: &'static str) -> bool {
        self.omit_history_session(code, None)
    }
    /// 身份证据仅供本次后端改名准入；匿名省略不能被后续已知身份覆盖。
    pub(crate) fn omit_history_session(&mut self, code: &'static str, id: Option<&str>) -> bool {
        if (code == "SOURCE_TOO_LARGE" && self.bytes_left == 0)
            || !matches!(
                code,
                "SOURCE_UNSUPPORTED"
                    | "SOURCE_INVALID"
                    | "SOURCE_INVALID_TEXT"
                    | "SOURCE_TOO_LARGE"
                    | "SOURCE_READ_FAILED"
            )
        {
            return false;
        }
        self.history_metadata_incomplete = true;
        self.history_read_failures.insert(code);
        if let Some(id) = id.and_then(|id| uuid::Uuid::parse_str(id).ok()) {
            self.history_omitted_sessions.insert(id);
        } else {
            self.history_anonymous_omission = true;
        }
        true
    }
    pub(crate) fn rename_target_may_be_omitted(&self, id: &str) -> bool {
        self.history_anonymous_omission
            || uuid::Uuid::parse_str(id)
                .map(|id| self.history_omitted_sessions.contains(&id))
                .unwrap_or(true)
    }
    fn entry(&mut self) -> ReadResult<()> {
        self.checkpoint()?;
        self.entries_left = self
            .entries_left
            .checked_sub(1)
            .ok_or("SOURCE_TOO_MANY_ENTRIES")?;
        Ok(())
    }
}
pub(crate) struct Root {
    dir: Dir,
    selected: PathBuf,
    identity: String,
    scope_key: String,
    sqlite_file: Option<String>,
}
impl std::fmt::Debug for Root {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Root(<redacted>)")
    }
}
pub(crate) struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_file: bool,
}
impl Root {
    pub(crate) fn selected_path(&self) -> &Path {
        &self.selected
    }
    /// 延续证明单独复核路径入口；普通历史热路径仍使用现有 held-cap 检查。
    pub(crate) fn source_current(&self) -> ReadResult<()> {
        self.current()?;
        let selected = Dir::open_ambient_dir(&self.selected, cap_std::ambient_authority())
            .map_err(|_| "SOURCE_CHANGED")?;
        if directory_key(&selected)? != self.identity {
            return Err("SOURCE_CHANGED");
        }
        Ok(())
    }
    pub(crate) fn clone_source(&self) -> ReadResult<Self> {
        self.source_current()?;
        let copy = Self {
            dir: self.dir.try_clone().map_err(read_error)?,
            selected: self.selected.clone(),
            identity: self.identity.clone(),
            scope_key: self.scope_key.clone(),
            sqlite_file: self.sqlite_file.clone(),
        };
        copy.source_current()?;
        Ok(copy)
    }
    pub(crate) fn open(path: &Path) -> ReadResult<Self> {
        let text = path.to_str().ok_or("SOURCE_INVALID_TEXT")?;
        if !path.is_absolute() || text.len() > 32768 || text.contains('\0') {
            return Err("SCOPE_UNKNOWN");
        }
        let dir = Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|_| "SCOPE_UNKNOWN")?;
        let identity = directory_key(&dir)?;
        let root = Self {
            dir,
            selected: path.to_owned(),
            scope_key: identity.clone(),
            identity,
            sqlite_file: None,
        };
        root.current()?;
        Ok(root)
    }
    pub(crate) fn open_sqlite(path: &Path, file: &str) -> ReadResult<Self> {
        if file.is_empty()
            || file.contains('\0')
            || (cfg!(windows) && file.contains(':'))
            || Path::new(file).components().count() != 1
            || !matches!(
                Path::new(file).components().next(),
                Some(Component::Normal(_))
            )
        {
            return Err("SCOPE_UNKNOWN");
        }
        let mut root = Self::open(path)?;
        let mut digest = Sha256::new();
        digest.update(b"cc-desk-opencode-sqlite-scope-v1\0");
        digest.update(root.identity.as_bytes());
        digest.update(b"\0");
        digest.update(file.as_bytes());
        root.scope_key = format!("{:x}", digest.finalize());
        root.sqlite_file = Some(file.to_owned());
        Ok(root)
    }
    pub(crate) fn sqlite_filename(&self) -> &str {
        self.sqlite_file.as_deref().unwrap_or("opencode.db")
    }
    pub(crate) fn key(&self) -> &str {
        &self.scope_key
    }
    pub(crate) fn descendant(&self, path: &Path) -> ReadResult<String> {
        let path = path
            .strip_prefix(&self.selected)
            .map_err(|_| "SOURCE_PATH_REJECTED")?;
        let components = path
            .components()
            .map(|part| match part {
                Component::Normal(name) => name.to_str().ok_or("SOURCE_INVALID_TEXT"),
                _ => Err("SOURCE_PATH_REJECTED"),
            })
            .collect::<ReadResult<Vec<_>>>()?;
        let relative_path = components.join("/");
        relative(Path::new(&relative_path), false)?;
        Ok(relative_path)
    }

    pub(crate) fn current(&self) -> ReadResult<()> {
        #[cfg(windows)]
        {
            // Windows 目录能力以 deny delete-sharing 打开（held_root_cannot_
            // silently_follow_replacement 的 Windows 分支）：持有句柄期间目录
            // 无法删除或替换，重开路径只能解析到同一目录。直接对持有句柄取
            // 身份 key 即可；check() 按目录×项目调用本函数，重开路径在实时
            // 扫描下是 O(目录数×项目数) 次 CreateFile 的主导成本。
            if directory_key(&self.dir)? != self.identity {
                return Err("SOURCE_CHANGED");
            }
        }
        #[cfg(unix)]
        {
            let now = Dir::open_ambient_dir(&self.selected, cap_std::ambient_authority())
                .map_err(|_| "SOURCE_CHANGED")?;
            if directory_key(&now)? != self.identity {
                return Err("SOURCE_CHANGED");
            }
        }
        #[cfg(not(any(windows, unix)))]
        {
            let now = Dir::open_ambient_dir(&self.selected, cap_std::ambient_authority())
                .map_err(|_| "SOURCE_CHANGED")?;
            if directory_key(&now)? != self.identity {
                return Err("SOURCE_CHANGED");
            }
        }
        Ok(())
    }
    pub(crate) fn read(&self, path: &Path, budget: &mut Budget) -> ReadResult<Option<Vec<u8>>> {
        if is_zstd_history(path) {
            return Ok(self
                .read_zstd(path, budget, Mode::Full, None)?
                .map(|(bytes, _)| bytes));
        }
        Ok(self
            .read_bounded(path, budget, false, 64 * 1024, None)?
            .map(|(bytes, _)| bytes))
    }
    #[cfg(test)]
    pub(crate) fn history_prefix(
        &self,
        path: &Path,
        budget: &mut Budget,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        self.read_bounded(path, budget, true, 64 * 1024, None)
    }
    pub(crate) fn history_header(
        &self,
        path: &Path,
        budget: &mut Budget,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        if is_zstd_history(path) {
            return self.read_zstd(path, budget, Mode::Prefix { window: usize::MAX }, None);
        }
        // Codex stores identity/cwd in its first complete session_meta record.
        // Sample a bounded title only when it is in the same observed chunks.
        self.read_bounded(path, budget, true, 4 * 1024, None)
    }
    /// Codex `session_meta` 是第一行（本机实测约 23KiB，内嵌系统提示），而第一条
    /// 用户消息位于其后的 developer 环境上下文之后（实测中位约 75KiB，p90 约
    /// 118KiB）。一次打开内读头窗口直到观察者确认「身份 + 首条用户消息」或到
    /// 窗口上限，再读尾部取最近原生时间戳。窗口未覆盖用户消息时标题未知、行照常
    /// 列出，绝不因取标题失败而丢弃整行。
    pub(crate) fn history_codex_metadata(
        &self,
        path: &Path,
        budget: &mut Budget,
        observe: &mut HistoryObserver<'_>,
    ) -> ReadResult<Option<CodexHead>> {
        if is_zstd_history(path) {
            return self
                .read_zstd(
                    path,
                    budget,
                    Mode::Prefix {
                        window: CODEX_HEAD_WINDOW,
                    },
                    Some(observe),
                )
                .map(|result| {
                    result.map(|(mut head, mut incomplete)| {
                        // 压缩流没有可随机定位的尾窗；时间只能取自已观察的完整记录。
                        if let Some(end) = head.iter().rposition(|byte| *byte == b'\n') {
                            incomplete |= end + 1 < head.len();
                            head.truncate(end + 1);
                        }
                        budget.history_metadata_incomplete |= incomplete;
                        let mut start = head.len().saturating_sub(4096);
                        if start > 0 {
                            start += head[start..]
                                .iter()
                                .position(|byte| *byte == b'\n')
                                .map_or(head.len() - start, |offset| offset + 1);
                        }
                        let tail = head[start..].to_vec();
                        (head, tail, incomplete)
                    })
                });
        }
        relative(path, false)?;
        budget.entry()?;
        budget.checkpoint()?;
        self.current()?;
        let metadata = match self.dir.symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(read_error(e)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let cap = budget.file_bytes.min(budget.bytes_left);
        if cap == 0 {
            return Err("SOURCE_TOO_LARGE");
        }
        // 尾部观察最多再读 4KiB，必须先从本次上限预留，头窗口与尾部的合计才不
        // 会突破剩余聚合预算；否则 saturating 扣减会把超额读取伪装成预算恰好
        // 耗尽（实测 115 字节预算下 233 字节文件仍被完整读出）。
        let readable = cap.saturating_sub((metadata.len() as usize).min(4096));
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            // A file swapped for a FIFO must not indefinitely block the reader.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self.dir.open_with(path, &options).map_err(read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }

        // 阶段一：读完整的第一条记录（身份）。真实 session_meta 可达 ~230KiB，
        // 因此这一步不受头窗口上限约束，只受单文件字节上限保护。
        let mut head = Vec::new();
        let chunk = 8 * 1024;
        while head.iter().all(|byte| *byte != b'\n') {
            budget.checkpoint()?;
            let next = chunk.min(readable.saturating_sub(head.len()));
            if next == 0 {
                break;
            }
            let previous = head.len();
            let read = (&file).take(next as u64).read_to_end(&mut head);
            budget.bytes_left = budget.bytes_left.saturating_sub(head.len() - previous);
            read.map_err(read_error)?;
            if head.len() == previous {
                break;
            }
        }
        // 首条记录已完整（缓冲含换行）。阶段一可能多读至块边界，这些字节必须
        // 保留：游标已越过它们，若在此截断缓冲，阶段二会从游标处继续追加，
        // 中间的记录被永久跳过（实测曾因此丢失 200 个 rollout 并切断多字节字符）。
        if !head.contains(&b'\n') {
            return Err("SOURCE_TOO_LARGE");
        }

        // 阶段二：继续读到首条用户消息（标题来源）或头窗口上限；观察者确认后即停，
        // 把本机真实根的头窗口 IO 从约 366MiB 降到约 102MiB。
        let head_cap = readable.min(CODEX_HEAD_WINDOW);
        while head.len() < head_cap {
            budget.checkpoint()?;
            let next = chunk.min(head_cap - head.len());
            if next == 0 {
                break;
            }
            let previous = head.len();
            let read = (&file).take(next as u64).read_to_end(&mut head);
            budget.bytes_left = budget.bytes_left.saturating_sub(head.len() - previous);
            read.map_err(read_error)?;
            if head.len() == previous {
                break;
            }
            if observe(&head)? {
                break;
            }
        }
        // 只保留完整记录：解析路径要求以换行结尾。
        if let Some(end) = head.iter().rposition(|byte| *byte == b'\n') {
            head.truncate(end + 1);
        }

        let tail_bytes = before.len().min(4096);
        let mut tail = Vec::new();
        if tail_bytes > 0 {
            (&file)
                .seek(SeekFrom::End(-(tail_bytes as i64)))
                .map_err(read_error)?;
            let read = (&file).take(tail_bytes).read_to_end(&mut tail);
            budget.bytes_left = budget.bytes_left.saturating_sub(tail.len());
            read.map_err(read_error)?;
            // 尾部按字节截取可能起始于多字节字符中间；丢弃首个不完整行，保证
            // tail 是完整行序列，否则 CJK 内容会让整个 tail 无法通过 UTF-8 校验，
            // 最近时间戳整体丢失。
            if let Some(position) = tail.iter().position(|byte| *byte == b'\n') {
                tail.drain(..=position);
            }
        }
        let after = file.metadata().map_err(read_error)?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err("SOURCE_CHANGED");
        }
        budget.checkpoint()?;
        self.current()?;
        let incomplete = (head.len() as u64) < before.len();
        if incomplete {
            budget.history_metadata_incomplete = true;
        }
        Ok(Some((head, tail, incomplete)))
    }
    /// 无打开的开销：仅取大小与修改时间，供元数据缓存命中判定使用。
    pub(crate) fn stat(
        &self,
        path: &Path,
    ) -> ReadResult<Option<(u64, Option<std::time::SystemTime>)>> {
        relative(path, false)?;
        match self.dir.symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(Some((
                metadata.len(),
                metadata.modified().ok().map(|t| t.into_std()),
            ))),
            Ok(_) => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(read_error(e)),
        }
    }
    /// 校验 SQLite 来源文件；必须区分缺失与链接/非普通文件。
    pub(crate) fn sqlite_file_present(&self, path: &Path) -> ReadResult<bool> {
        relative(path, false)?;
        self.current()?;
        match self.dir.symlink_metadata(path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => Ok(true),
            Ok(_) => Err("SOURCE_NOT_REGULAR"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(read_error(error)),
        }
    }
    /// Windows SQLite VFS 不对 WAL 使用 NOFOLLOW；复制前只交出能力目录内的普通文件句柄。
    #[cfg(windows)]
    pub(crate) fn sqlite_source_file(&self, path: &Path) -> ReadResult<Option<std::fs::File>> {
        use cap_std::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        relative(path, false)?;
        self.current()?;
        let metadata = match self.dir.symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(read_error(error)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0);
        let file = self.dir.open_with(path, &options).map_err(read_error)?;
        if !file.metadata().map_err(read_error)?.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        self.current()?;
        Ok(Some(file.into_std()))
    }
    pub(crate) fn history_until(
        &self,
        path: &Path,
        budget: &mut Budget,
        observe: &mut HistoryObserver<'_>,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        if is_zstd_history(path) {
            return self.read_zstd(
                path,
                budget,
                Mode::Prefix { window: usize::MAX },
                Some(observe),
            );
        }
        // The observer sees charged bytes from one held file. It may ask for
        // another chunk but cannot expand the original file/aggregate caps.
        self.read_bounded(path, budget, true, 4 * 1024, Some(observe))
    }
    pub(crate) fn history_title_tail(
        &self,
        path: &Path,
        header_bytes: usize,
        budget: &mut Budget,
    ) -> ReadResult<Option<TitleTail>> {
        if !budget.optional_title_available(header_bytes, CLAUDE_TITLE_TAIL) {
            return Ok(None);
        }
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let optional_read_error = |error: io::Error| {
            if error.kind() == io::ErrorKind::NotFound {
                "SOURCE_CHANGED"
            } else {
                read_error(error)
            }
        };
        let metadata = self
            .dir
            .symlink_metadata(path)
            .map_err(optional_read_error)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self
            .dir
            .open_with(path, &options)
            .map_err(optional_read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        if before.len() < header_bytes as u64 {
            return Err("SOURCE_CHANGED");
        }
        let mut header = Vec::new();
        let read = (&file).take(header_bytes as u64).read_to_end(&mut header);
        budget.bytes_left = budget.bytes_left.saturating_sub(header.len());
        read.map_err(read_error)?;
        budget.checkpoint()?;
        // 尾窗按 CLAUDE_TITLE_TAIL 读取；CLI 的 ai-title/custom-title 写在末尾附近，
        // 4KiB 不足以覆盖实测 p90（33KiB）。调用方已在 optional_title_available
        // 里按同一常量预付预算，这里不再重复校验。
        let tail_bytes = before.len().min(CLAUDE_TITLE_TAIL as u64);
        (&file)
            .seek(SeekFrom::End(-(tail_bytes as i64)))
            .map_err(read_error)?;
        let mut tail = Vec::new();
        let read = (&file).take(tail_bytes).read_to_end(&mut tail);
        budget.bytes_left = budget.bytes_left.saturating_sub(tail.len());
        read.map_err(read_error)?;
        let after = file.metadata().map_err(read_error)?;
        if before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || header.len() != header_bytes
            || tail.len() as u64 != tail_bytes
        {
            return Err("SOURCE_CHANGED");
        }
        budget.checkpoint()?;
        self.current()?;
        Ok(Some((header, tail, before.len() > tail_bytes)))
    }
    fn read_bounded(
        &self,
        path: &Path,
        budget: &mut Budget,
        history_prefix: bool,
        chunk_bytes: usize,
        mut observe: Option<&mut HistoryObserver<'_>>,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let metadata = match self.dir.symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(read_error(e)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let cap = budget.file_bytes.min(budget.bytes_left);
        if (history_prefix && cap == 0) || (!history_prefix && metadata.len() > cap as u64) {
            return Err("SOURCE_TOO_LARGE");
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            // A file swapped for a FIFO must not indefinitely block the reader.
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self.dir.open_with(path, &options).map_err(read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        if !history_prefix && before.len() > cap as u64 {
            return Err("SOURCE_TOO_LARGE");
        }
        let mut bytes = Vec::new();
        let mut limit = if history_prefix {
            cap.min(chunk_bytes)
        } else {
            cap + 1
        };
        let read = (&file).take(limit as u64).read_to_end(&mut bytes);
        // A failed read can already have consumed bytes. Debit before returning
        // so an omitted history entry cannot reset the aggregate I/O budget.
        budget.bytes_left = budget.bytes_left.saturating_sub(bytes.len());
        read.map_err(read_error)?;
        // Codex needs a complete first record; Claude may need to pass leading
        // metadata before observing a complete supported cwd record.
        let mut complete_record = if let Some(observe) = observe.as_mut() {
            observe(&bytes)?
        } else {
            history_prefix && bytes.contains(&b'\n')
        };
        while history_prefix && !complete_record && bytes.len() == limit && limit < cap {
            budget.checkpoint()?;
            let next = (cap - limit).min(chunk_bytes);
            let previous = bytes.len();
            let read = (&file).take(next as u64).read_to_end(&mut bytes);
            budget.bytes_left = budget.bytes_left.saturating_sub(bytes.len() - previous);
            read.map_err(read_error)?;
            // The earlier chunks have already been checked. A long first record
            // must not rescan the entire accumulated prefix on every extension.
            complete_record = if let Some(observe) = observe.as_mut() {
                observe(&bytes)?
            } else {
                bytes[previous..].contains(&b'\n')
            };
            limit += next;
        }
        if bytes.len() > cap {
            return Err("SOURCE_TOO_LARGE");
        }
        let after = file.metadata().map_err(read_error)?;
        let expected = if history_prefix {
            after.len().min(limit as u64)
        } else {
            after.len()
        };
        if before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || bytes.len() as u64 != expected
        {
            return Err("SOURCE_CHANGED");
        }
        let incomplete = bytes.len() as u64 != after.len();
        budget.history_metadata_incomplete |= history_prefix && incomplete;
        budget.checkpoint()?;
        self.current()?;
        Ok(Some((bytes, incomplete)))
    }
    pub(crate) fn list(&self, path: &Path, budget: &mut Budget) -> ReadResult<Vec<Entry>> {
        relative(path, true)?;
        budget.checkpoint()?;
        self.current()?;
        let path = if path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            path
        };
        let entries = match self.dir.read_dir(path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(read_error(e)),
        };
        let mut result = Vec::new();
        for entry in entries {
            budget.entry()?;
            let entry = entry.map_err(read_error)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "SOURCE_INVALID_TEXT")?;
            relative(Path::new(&name), false)?;
            let kind = entry.file_type().map_err(read_error)?;
            result.push(Entry {
                name,
                is_dir: kind.is_dir() && !kind.is_symlink(),
                is_file: kind.is_file() && !kind.is_symlink(),
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        self.current()?;
        Ok(result)
    }
}
fn relative(path: &Path, allow_empty: bool) -> ReadResult<()> {
    let text = path.to_str().ok_or("SOURCE_INVALID_TEXT")?;
    if text.len() > 4096
        || text.contains(['\0', ':', '\\'])
        || (!allow_empty && text.is_empty())
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("SOURCE_PATH_REJECTED");
    }
    Ok(())
}
fn read_error(error: io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "SOURCE_READ_FORBIDDEN",
        _ => "SOURCE_READ_FAILED",
    }
}
fn directory_key(dir: &Dir) -> ReadResult<String> {
    let file = dir.try_clone().map_err(read_error)?.into_std_file();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata().map_err(read_error)?;
        Ok(format!("local:unix:{}:{}", m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // The owned directory handle and output buffer remain live for this synchronous call.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(|_| "SCOPE_UNKNOWN")?;
        if info.nFileIndexHigh == 0 && info.nFileIndexLow == 0 {
            return Err("SCOPE_UNKNOWN");
        }
        Ok(format!(
            "local:windows:{}:{}:{}",
            info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err("SOURCE_UNSUPPORTED")
    }
}
#[cfg(test)]
#[path = "../../tests/native_cli_scoped_fs.rs"]
mod tests;
