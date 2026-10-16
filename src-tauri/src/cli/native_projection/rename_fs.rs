//! 固定转录文件身份，并为 SDK 建立单一文件的私有命名空间。
use super::*;

pub(crate) struct PinnedTranscript {
    file: cap_std::fs::File,
    path: String,
    identity: String,
}
pub(crate) struct PrivateTranscript {
    pub(crate) path: PathBuf,
    parent: Dir,
    directory: Option<Dir>,
    name: String,
    file: String,
    #[cfg(windows)]
    private: Option<crate::version_history::windows::files::PrivateDirectory>,
}
impl PrivateTranscript {
    pub(crate) fn install_bridge(&self, bytes: &[u8]) -> ReadResult<PathBuf> {
        self.directory
            .as_ref()
            .ok_or("SOURCE_CHANGED")?
            .write("bridge.cjs", bytes)
            .map_err(read_error)?;
        Ok(self.path.join("bridge.cjs"))
    }
}
impl Drop for PrivateTranscript {
    fn drop(&mut self) {
        if let Some(directory) = self.directory.take() {
            let _ = directory.remove_file(&self.file);
            let _ = directory.remove_file("bridge.cjs");
            let _ = directory.remove_dir("projects/authorized");
            let _ = directory.remove_dir("projects");
            drop(directory);
        }
        #[cfg(windows)]
        drop(self.private.take());
        let _ = self.parent.remove_dir(&self.name);
    }
}
fn identity(file: &cap_std::fs::File) -> ReadResult<String> {
    let file = file.try_clone().map_err(read_error)?.into_std();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = file.metadata().map_err(read_error)?;
        Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
        };
        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
            .map_err(|_| "SOURCE_CHANGED")?;
        Ok(format!(
            "{}:{}:{}",
            information.dwVolumeSerialNumber, information.nFileIndexHigh, information.nFileIndexLow
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err("NATIVE_RENAME_UNSUPPORTED")
    }
}
impl Root {
    pub(crate) fn find_transcript(&self, id: &str) -> ReadResult<PinnedTranscript> {
        let mut budget = Budget::new(Limits::default());
        let mut found = None;
        for project in self.list(Path::new("projects"), &mut budget)? {
            if !project.is_dir {
                continue;
            }
            let directory = format!("projects/{}", project.name);
            for entry in self.list(Path::new(&directory), &mut budget)? {
                if entry.name == format!("{id}.jsonl") && entry.is_file {
                    if found.is_some() {
                        return Err("SESSION_ORIGIN_AMBIGUOUS");
                    }
                    found = Some(self.pin_transcript(&format!("{directory}/{}", entry.name))?);
                }
            }
        }
        found.ok_or("SESSION_NOT_FOUND")
    }
    pub(crate) fn pin_transcript(&self, path: &str) -> ReadResult<PinnedTranscript> {
        relative(Path::new(path), false)?;
        if !path.ends_with(".jsonl") {
            return Err("NATIVE_RENAME_UNSUPPORTED");
        }
        self.current()?;
        let metadata = self.dir.symlink_metadata(path).map_err(read_error)?;
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
        let file = self.dir.open_with(path, &options).map_err(read_error)?;
        if !file.metadata().map_err(read_error)?.is_file() {
            return Err("SOURCE_NOT_REGULAR");
        }
        let result = PinnedTranscript {
            identity: identity(&file)?,
            file,
            path: path.into(),
        };
        result.current(self)?;
        Ok(result)
    }
    pub(crate) fn private_transcript(
        &self,
        source: &PinnedTranscript,
        id: &str,
    ) -> ReadResult<PrivateTranscript> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err("SOURCE_INVALID");
        }
        source.current(self)?;
        let name = format!(".ccdesk-rename-{}", uuid::Uuid::new_v4());
        #[cfg(windows)]
        let (directory, private, path) = {
            use crate::version_history::windows::{
                files::{ComponentName, Directory, PrivateDirectory},
                security::CurrentUser,
            };
            let parent = Directory::open_absolute(&self.selected)
                .map_err(|_| "NATIVE_RENAME_UNSUPPORTED")?;
            if directory_key(&parent.capability().map_err(read_error)?)? != self.identity {
                return Err("SOURCE_CHANGED");
            }
            let user = CurrentUser::capture().map_err(|_| "NATIVE_RENAME_UNSUPPORTED")?;
            let private = PrivateDirectory::create_new(
                parent,
                ComponentName::new(std::ffi::OsStr::new(&name)).map_err(read_error)?,
                &user,
            )
            .map_err(|_| "NATIVE_RENAME_UNSUPPORTED")?;
            let path = self.selected.join(&name);
            self.source_current()?;
            (
                private.directory().capability().map_err(read_error)?,
                Some(private),
                path,
            )
        };
        #[cfg(not(windows))]
        let (directory, path) = {
            let mut options = cap_std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use cap_std::fs::DirBuilderExt;
                options.mode(0o700);
            }
            self.dir
                .create_dir_with(&name, &options)
                .map_err(|_| "NATIVE_RENAME_UNSUPPORTED")?;
            (
                self.dir.open_dir(&name).map_err(read_error)?,
                self.selected.join(&name),
            )
        };
        let result = PrivateTranscript {
            parent: self.dir.try_clone().map_err(read_error)?,
            directory: Some(directory),
            name,
            file: format!("projects/authorized/{id}.jsonl"),
            path,
            #[cfg(windows)]
            private,
        };
        let directory = result.directory.as_ref().ok_or("SOURCE_CHANGED")?;
        directory.create_dir("projects").map_err(read_error)?;
        directory
            .create_dir("projects/authorized")
            .map_err(read_error)?;
        self.dir
            .hard_link(&source.path, directory, &result.file)
            .map_err(|_| "NATIVE_RENAME_UNSUPPORTED")?;
        let linked = directory.open(&result.file).map_err(read_error)?;
        if identity(&linked)? != source.identity {
            return Err("SOURCE_CHANGED");
        }
        source.current(self)?;
        Ok(result)
    }
}
impl PinnedTranscript {
    pub(crate) fn current(&self, root: &Root) -> ReadResult<()> {
        root.current()?;
        let now = root.dir.symlink_metadata(&self.path).map_err(read_error)?;
        if !now.is_file() || now.file_type().is_symlink() {
            return Err("SOURCE_CHANGED");
        }
        let file = root.dir.open(&self.path).map_err(read_error)?;
        if identity(&file)? != self.identity || identity(&self.file)? != self.identity {
            return Err("SOURCE_CHANGED");
        }
        root.current()
    }
    pub(crate) fn verify(&self, id: &str, cwd: &str, title: Option<&str>) -> ReadResult<()> {
        let mut file = self.file.try_clone().map_err(read_error)?;
        file.seek(SeekFrom::Start(0)).map_err(read_error)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        let mut found_cwd = false;
        for line in bytes
            .split_inclusive(|byte| *byte == b'\n')
            .filter(|line| line.ends_with(b"\n"))
        {
            let value: serde_json::Value =
                serde_json::from_slice(line).map_err(|_| "SOURCE_INVALID")?;
            if value
                .get("sessionId")
                .is_some_and(|value| value.as_str() != Some(id))
            {
                return Err("SOURCE_AMBIGUOUS");
            }
            if let Some(observed) = value["cwd"].as_str() {
                if observed != cwd {
                    return Err("SOURCE_AMBIGUOUS");
                }
                found_cwd = true;
            }
        }
        if !found_cwd {
            return Err("SOURCE_AMBIGUOUS");
        }
        if let Some(title) = title {
            let length = file.metadata().map_err(read_error)?.len();
            file.seek(SeekFrom::Start(length.saturating_sub(65536)))
                .map_err(read_error)?;
            let mut tail = Vec::new();
            (&mut file)
                .take(131072)
                .read_to_end(&mut tail)
                .map_err(read_error)?;
            let last = tail
                .split(|byte| *byte == b'\n')
                .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
                .rfind(|value| {
                    value["type"] == "custom-title" && value["sessionId"].as_str() == Some(id)
                });
            if last
                .as_ref()
                .and_then(|value| value["customTitle"].as_str())
                != Some(title)
            {
                return Err("RENAME_VERIFICATION_FAILED");
            }
        }
        Ok(())
    }
}
