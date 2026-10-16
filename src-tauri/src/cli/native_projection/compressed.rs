//! 在已有目录能力下有界解压，不把原生历史写回磁盘。
use super::*;
use zstd::stream::raw::{Decoder, Operation};

pub(super) enum Mode {
    Full,
    Prefix { window: usize },
}

pub(super) fn is_zstd_history(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

impl Root {
    pub(super) fn read_zstd(
        &self,
        path: &Path,
        budget: &mut Budget,
        mode: Mode,
        observe: Option<&mut HistoryObserver<'_>>,
    ) -> ReadResult<Option<(Vec<u8>, bool)>> {
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let metadata = match self.dir.symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(read_error(error)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("SOURCE_NOT_REGULAR");
        }
        if budget.bytes_left == 0 {
            return Err("SOURCE_TOO_LARGE");
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let file = self.dir.open_with(path, &options).map_err(read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let result = decode(&file, before.len(), budget, mode, observe)?;
        let after = file.metadata().map_err(read_error)?;
        if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
            return Err("SOURCE_CHANGED");
        }
        budget.checkpoint()?;
        self.current()?;
        budget.history_metadata_incomplete |= result.1;
        Ok(Some(result))
    }
}

fn decode(
    mut file: &cap_std::fs::File,
    length: u64,
    budget: &mut Budget,
    mode: Mode,
    mut observe: Option<&mut HistoryObserver<'_>>,
) -> ReadResult<(Vec<u8>, bool)> {
    let mut decoder = Decoder::new().map_err(|_| "SOURCE_READ_FAILED")?;
    // 解压输出受字节预算保护，回溯窗口也必须单独有界，不能信任 frame 声明。
    decoder
        .set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(23))
        .map_err(|_| "SOURCE_READ_FAILED")?;
    let mut input = [0u8; 8192];
    let mut output = [0u8; 8192];
    let mut loaded = 0usize;
    let mut used = 0usize;
    let mut encoded = 0usize;
    let mut bytes = Vec::new();
    let mut complete_record = false;
    loop {
        budget.checkpoint()?;
        if used == loaded && (encoded as u64) < length {
            let next = input
                .len()
                .min(budget.file_bytes.saturating_sub(encoded))
                .min(budget.bytes_left);
            if next == 0 {
                return Err("SOURCE_TOO_LARGE");
            }
            loaded = file.read(&mut input[..next]).map_err(read_error)?;
            budget.bytes_left -= loaded;
            encoded += loaded;
            used = 0;
            if loaded == 0 {
                return Err("SOURCE_CHANGED");
            }
        }
        let prefix_left = match &mode {
            Mode::Prefix { window } if complete_record => window.saturating_sub(bytes.len()),
            _ => usize::MAX,
        };
        let next = output
            .len()
            .min(prefix_left)
            .min(budget.file_bytes.saturating_sub(bytes.len()))
            .min(budget.bytes_left);
        let status = match decoder.run_on_buffers(&input[used..loaded], &mut output[..next]) {
            Ok(status) => status,
            Err(_) => {
                // 库在失败时不返回已解压长度，保守计入本块上限，省略条目不能退还工作预算。
                budget.bytes_left -= next;
                return Err("SOURCE_INVALID");
            }
        };
        used += status.bytes_read;
        budget.bytes_left -= status.bytes_written;
        bytes.extend_from_slice(&output[..status.bytes_written]);
        complete_record |= output[..status.bytes_written].contains(&b'\n');
        let input_eof = used == loaded && encoded as u64 == length;
        if status.remaining == 0 && input_eof {
            return Ok((bytes, false));
        }
        // 已耗尽整个压缩输入且输出缓冲没有填满：缺少 frame 尾部，不能伪装成
        // 正常的前缀观察。大型流仅观察到前缀时不对未读取的尾部作完整性断言。
        if input_eof && status.remaining != 0 && status.bytes_written < next {
            return Err("SOURCE_INVALID");
        }
        if let Mode::Prefix { window } = mode {
            let observed = if let Some(observer) = observe.as_mut() {
                observer(&bytes)?
            } else {
                complete_record
            };
            if !bytes.is_empty()
                && (observed
                    || (complete_record && bytes.len() >= window)
                    || bytes.len() == budget.file_bytes)
            {
                return Ok((bytes, true));
            }
        }
        if next == 0 {
            return Err("SOURCE_TOO_LARGE");
        }
        if status.bytes_read == 0 && status.bytes_written == 0 {
            return Err("SOURCE_INVALID");
        }
    }
}
