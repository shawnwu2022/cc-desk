//! 删除观察只绑定所选 UUID 的全部原始 rollout，不受显示折叠或其它根删除影响。
use super::*;

impl Root {
    pub(crate) fn codex_delete_observation(
        &self,
        native_id: &str,
        cwd: &str,
        archived: bool,
        budget: &mut Budget,
    ) -> ReadResult<String> {
        let target = uuid::Uuid::parse_str(native_id).map_err(|_| "SESSION_ORIGIN_AMBIGUOUS")?;
        let mut files = Vec::new();
        self.delete_rollouts("sessions", 4, budget, &mut files)?;
        self.delete_rollouts("archived_sessions", 4, budget, &mut files)?;
        files.sort();
        let mut digest = Sha256::new();
        let mut matched = 0usize;
        for path in files {
            let relative = Path::new(&path);
            let bytes = self
                .history_until(relative, budget, &mut |bytes| Ok(bytes.contains(&b'\n')))?
                .ok_or("SOURCE_CHANGED")?
                .0;
            let line = bytes
                .split(|byte| *byte == b'\n')
                .next()
                .ok_or("SOURCE_INVALID")?;
            let meta: serde_json::Value =
                serde_json::from_slice(line).map_err(|_| "SOURCE_INVALID")?;
            if meta["type"] != "session_meta" {
                return Err("SOURCE_INVALID");
            }
            let id = meta["payload"]["id"]
                .as_str()
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
                .ok_or("SESSION_ORIGIN_AMBIGUOUS")?;
            if id != target {
                continue;
            }
            // 先核实际头身份，再处理显示会过滤掉的自动会话或其它项目 rollout。
            if path.starts_with("archived_sessions/") != archived
                || meta["payload"]["cwd"].as_str() != Some(cwd)
            {
                return Err("SESSION_ORIGIN_AMBIGUOUS");
            }
            let fingerprint = self.delete_file_fingerprint(relative, budget)?;
            digest.update(serde_json::to_vec(&(path, fingerprint)).map_err(|_| "SOURCE_INVALID")?);
            matched += 1;
        }
        if matched == 0 {
            return Err("SESSION_NOT_FOUND");
        }
        self.current()?;
        Ok(format!("{:x}", digest.finalize()))
    }

    fn delete_rollouts(
        &self,
        path: &str,
        depth: usize,
        budget: &mut Budget,
        files: &mut Vec<String>,
    ) -> ReadResult<()> {
        for entry in self.list(Path::new(path), budget)? {
            let next = format!("{path}/{}", entry.name);
            if entry.is_dir {
                if depth == 0 {
                    return Err("SOURCE_TOO_MANY_ENTRIES");
                }
                self.delete_rollouts(&next, depth - 1, budget, files)?;
            } else if entry.name.ends_with(".jsonl") || entry.name.ends_with(".jsonl.zst") {
                if !entry.is_file {
                    return Err("SOURCE_NOT_REGULAR");
                }
                files.push(next);
            } else if !entry.is_dir && !entry.is_file {
                return Err("SOURCE_NOT_REGULAR");
            }
        }
        Ok(())
    }

    fn delete_file_fingerprint(&self, path: &Path, budget: &mut Budget) -> ReadResult<String> {
        relative(path, false)?;
        budget.entry()?;
        self.current()?;
        let check = || -> ReadResult<()> {
            let metadata = self.dir.symlink_metadata(path).map_err(read_error)?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err("SOURCE_NOT_REGULAR");
            }
            Ok(())
        };
        check()?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
        }
        let mut file = self.dir.open_with(path, &options).map_err(read_error)?;
        let before = file.metadata().map_err(read_error)?;
        if !before.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let identity = rename_fs::identity(&file)?;
        if before.len() > budget.bytes_left as u64 {
            return Err("SOURCE_TOO_LARGE");
        }
        let mut digest = Sha256::new();
        digest.update(identity.as_bytes());
        let mut buffer = [0u8; 64 * 1024];
        let mut read = 0u64;
        loop {
            budget.checkpoint()?;
            let count = file.read(&mut buffer).map_err(read_error)?;
            if count == 0 {
                break;
            }
            budget.bytes_left = budget
                .bytes_left
                .checked_sub(count)
                .ok_or("SOURCE_BUDGET_EXCEEDED")?;
            read += count as u64;
            digest.update(&buffer[..count]);
        }
        let after = file.metadata().map_err(read_error)?;
        check()?;
        let current = self.dir.open_with(path, &options).map_err(read_error)?;
        if read != before.len()
            || before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || rename_fs::identity(&current)? != identity
        {
            return Err("SOURCE_CHANGED");
        }
        self.current()?;
        Ok(format!("{:x}", digest.finalize()))
    }
}
