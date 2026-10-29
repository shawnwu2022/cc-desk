//! 限定资格入口；不是 E08 admission、旧 OwnedJob 修复或远控授权。
pub use portable_pty::{JobSpawnError as NativePipeError, JobSpawnPhase as NativePipePhase};
use std::{
    ffi::{OsStr, OsString},
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    time::Instant,
};
fn preparation() -> NativePipeError {
    NativePipeError::KnownNotStarted {
        phase: NativePipePhase::Preparation,
    }
}
/// 后端显式 recipe；不从 std::Command 私有状态反推，不实现 Debug/Serialize。
pub struct BackendPipeLaunch {
    command: portable_pty::CommandBuilder,
}
impl BackendPipeLaunch {
    pub fn from_backend_recipe(
        program: PathBuf,
        cwd: PathBuf,
        args: Vec<OsString>,
        env: Vec<(OsString, OsString)>,
    ) -> Result<Self, NativePipeError> {
        if !program.is_absolute() || !cwd.is_absolute() || args.len() > 64 || env.len() > 64 {
            return Err(preparation());
        }
        let mut units = 0;
        fn bounded(value: &OsStr, units: &mut usize) -> Result<(), NativePipeError> {
            let mut length = 0;
            for unit in value.encode_wide() {
                if unit == 0 {
                    return Err(preparation());
                }
                length += 1;
                if length > 4096 {
                    return Err(preparation());
                }
            }
            *units = units.checked_add(length).ok_or_else(preparation)?;
            if *units > 16384 {
                return Err(preparation());
            }
            Ok(())
        }
        bounded(program.as_os_str(), &mut units)?;
        bounded(cwd.as_os_str(), &mut units)?;
        for arg in &args {
            bounded(arg, &mut units)?;
        }
        let mut names = std::collections::HashSet::new();
        for (k, v) in &env {
            bounded(k, &mut units)?;
            bounded(v, &mut units)?;
            let name = k.to_str().ok_or_else(preparation)?;
            if name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || !names.insert(name.to_ascii_uppercase())
            {
                return Err(preparation());
            }
        }
        let mut command = portable_pty::CommandBuilder::new(program);
        command.env_clear();
        command.cwd(cwd);
        command.args(args);
        for (k, v) in env {
            command.env(k, v);
        }
        Ok(Self { command })
    }
}
/// 只证明确切 root 已退出，不证明子孙/旧 IO 全部退休。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeRootExitOnly {
    pub exit_code: u32,
}
pub struct NativeOwnedPipeRun {
    child: portable_pty::NativePipeChild,
    job: Option<crate::windows_job::Job>,
}
impl NativeOwnedPipeRun {
    pub fn launch(recipe: BackendPipeLaunch) -> Result<Self, NativePipeError> {
        let job = crate::windows_job::Job::new().map_err(|_| preparation())?;
        let child = portable_pty::spawn_pipe_in_job(recipe.command, job.borrowed())?;
        Ok(Self {
            child,
            job: Some(job),
        })
    }
    pub fn take_stdin(&mut self) -> Option<portable_pty::NativePipeInput> {
        self.child.take_stdin()
    }
    pub fn take_stdout(&mut self) -> Option<portable_pty::NativePipeOutput> {
        self.child.take_stdout()
    }
    pub fn take_stderr(&mut self) -> Option<portable_pty::NativePipeOutput> {
        self.child.take_stderr()
    }
    pub fn try_wait_root(&self) -> Result<Option<NativeRootExitOnly>, NativePipeError> {
        self.child
            .try_wait_root()
            .map(|r| r.map(|exit_code| NativeRootExitOnly { exit_code }))
    }
    pub fn wait_root_until(&self, end: Instant) -> Result<NativeRootExitOnly, NativePipeError> {
        self.child
            .wait_root_until(end)
            .map(|exit_code| NativeRootExitOnly { exit_code })
    }
    pub fn stop(&mut self, end: Instant) -> Result<NativeRootExitOnly, NativePipeError> {
        drop(self.job.take());
        self.child
            .stop_root_until(end)
            .map(|exit_code| NativeRootExitOnly { exit_code })
    }
}
impl Drop for NativeOwnedPipeRun {
    fn drop(&mut self) {
        drop(self.job.take()); /* child Drop 只请求 exact root 终止，不等待/不宣称 join。 */
    }
}
