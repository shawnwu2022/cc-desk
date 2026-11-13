//! 单次 Codex 扫描复用父目录能力；不缓存文件、身份或授权结果。
use super::*;
use std::sync::Arc;

impl Root {
    pub(super) fn retain_history_parent(&self, path: &Path, budget: &mut Budget) -> ReadResult<()> {
        relative(path, false)?;
        budget.checkpoint()?;
        self.current()?;
        #[cfg(windows)]
        {
            use cap_std::fs::{MetadataExt, OpenOptionsExt};
            use windows::Win32::Storage::FileSystem::{
                FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
                FILE_SHARE_WRITE,
            };

            let parent_path = path.parent().ok_or("SOURCE_PATH_REJECTED")?;
            if parent_path.as_os_str().is_empty()
                || budget
                    .history_parents
                    .contains_key(&(self.identity.clone(), parent_path.to_owned()))
            {
                return Ok(());
            }
            // 这是句柄缓存的容量，不是来源准入门槛；装不下时保留原有逐路径读取。
            if budget.history_parents.len() + parent_path.components().count() > 256 {
                return Ok(());
            }
            let mut directory = Arc::new(self.dir.try_clone().map_err(read_error)?);
            let mut relative_parent = PathBuf::new();
            for component in parent_path.components() {
                budget.checkpoint()?;
                relative_parent.push(component.as_os_str());
                let key = (self.identity.clone(), relative_parent.clone());
                if let Some(held) = budget.history_parents.get(&key) {
                    directory = held.clone();
                    continue;
                }
                // 一次仅打开一个相对分量。OPEN_REPARSE_POINT 打开链接本身而不跟随，
                // 对同一已打开句柄检查类型；deny-delete 保证整次观察期间祖先不可替换。
                let mut options = OpenOptions::new();
                options
                    .read(true)
                    .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
                    .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
                let file = match directory.open_with(component.as_os_str(), &options) {
                    Ok(file) => file,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                    Err(error) => return Err(read_error(error)),
                };
                let metadata = file.metadata().map_err(read_error)?;
                if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
                    return Err("SOURCE_NOT_REGULAR");
                }
                directory = Arc::new(Dir::from_std_file(file.into_std()));
                budget.history_parents.insert(key, directory.clone());
            }
        }
        Ok(())
    }

    pub(super) fn history_location<'p>(
        &self,
        path: &'p Path,
        budget: &Budget,
    ) -> (Option<Arc<Dir>>, &'p Path) {
        #[cfg(windows)]
        if let Some(parent) = path.parent().and_then(|parent| {
            budget
                .history_parents
                .get(&(self.identity.clone(), parent.to_owned()))
        }) {
            if let Some(name) = path.file_name() {
                return (Some(parent.clone()), Path::new(name));
            }
        }
        let _ = budget;
        (None, path)
    }
}
