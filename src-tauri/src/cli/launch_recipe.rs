//! 冻结调用的参数归属；只使用已有证明，不发现、执行或授权新的程序。
use super::profiles::error;
use super::snapshot::LaunchSnapshot;
use super::types::{CliKind, LaunchRequest, SafeError};
use std::ffi::OsString;
use std::path::Path;

/// 借用 snapshot 同时保留其 transport/句柄生命周期；不序列化或复制证明。
pub(crate) struct LaunchRecipe<'a> {
    snapshot: &'a LaunchSnapshot,
    selected_program: &'a Path,
    selected_runner: Option<&'a Path>,
    action: Vec<OsString>,
    hooks: HookDelivery<'a>,
}

enum HookDelivery<'a> {
    PreservedOuter(&'a [OsString]),
    DirectCore(&'a [OsString]),
    #[cfg(windows)]
    NpmPreload(&'a super::codex_cmd::ProvenNpmEntry),
}

impl std::fmt::Debug for LaunchRecipe<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LaunchRecipe(<redacted>)")
    }
}

impl<'a> LaunchRecipe<'a> {
    pub(super) fn from_snapshot(snapshot: &'a LaunchSnapshot) -> Result<Self, SafeError> {
        let action = if snapshot.raw_args().is_some() {
            Vec::new()
        } else {
            if snapshot
                .legacy_default_args()
                .is_some_and(|v| !v.is_empty())
            {
                return Err(error("LEGACY_ARGUMENTS_REQUIRE_MIGRATION"));
            }
            match snapshot.request().cli {
                CliKind::Claude => super::claude::action_args(snapshot)?,
                CliKind::Codex => super::codex::action_args(snapshot)?,
                CliKind::Opencode => super::opencode::action_args(snapshot)?,
                CliKind::Shell => super::shell::action_args(snapshot)?,
            }
        };
        let hooks = if snapshot.codex_direct_core() {
            HookDelivery::DirectCore(snapshot.identity_args())
        } else {
            HookDelivery::PreservedOuter(snapshot.identity_args())
        };
        #[cfg(windows)]
        let hooks = if let Some(entry) = snapshot.codex_npm_entry() {
            // 已有 npm 计划必须独占投递；冲突不得靠丢弃外层参数隐式修复。
            if !snapshot.identity_args().is_empty() {
                return Err(error("REQUEST_SNAPSHOT_MISMATCH"));
            }
            HookDelivery::NpmPreload(entry)
        } else {
            hooks
        };
        Ok(Self {
            snapshot,
            selected_program: snapshot.program(),
            selected_runner: snapshot.runner(),
            action,
            hooks,
        })
    }

    pub(crate) fn program(&self) -> &Path {
        self.selected_program
    }

    pub(crate) fn runner(&self) -> Option<&Path> {
        self.selected_runner
    }

    fn check(&self, request: &LaunchRequest) -> Result<(), SafeError> {
        if request != self.snapshot.request() {
            return Err(error("REQUEST_SNAPSHOT_MISMATCH"));
        }
        request.validate()
    }

    /// 未证明路线保留原参数顺序，不把 defaultArgs 中的脚本猜成入口。
    fn command_tail(&self) -> Vec<OsString> {
        let mut args = self.action.clone();
        args.extend_from_slice(self.snapshot.default_args());
        if let Some(plugin) = self.snapshot.observer_plugin() {
            args.extend(["--plugin-dir".into(), plugin.as_os_str().to_owned()]);
        }
        args.extend_from_slice(self.snapshot.extra_args());
        args
    }

    pub(crate) fn arguments(&self, request: &LaunchRequest) -> Result<Vec<OsString>, SafeError> {
        self.check(request)?;
        if let Some(raw) = self.snapshot.raw_args() {
            return Ok(raw.to_vec());
        }
        // npm 的 hooks 由已有 loader 在文件入口后投递；外层 shim 只拿 CLI tail。
        let mut args = match &self.hooks {
            HookDelivery::PreservedOuter(args) | HookDelivery::DirectCore(args) => args.to_vec(),
            #[cfg(windows)]
            HookDelivery::NpmPreload(_) => Vec::new(),
        };
        args.extend(self.command_tail());
        Ok(args)
    }

    #[cfg(windows)]
    pub(crate) fn npm_entry(&self) -> Option<&super::codex_cmd::ProvenNpmEntry> {
        match &self.hooks {
            HookDelivery::NpmPreload(entry) => Some(entry),
            _ => None,
        }
    }

    /// 仅描述已证明入口的计划参数，不选择 Node，也不表示实际 hooks 已就绪。
    #[allow(dead_code)]
    pub(crate) fn planned_entry_argv(
        &self,
        request: &LaunchRequest,
    ) -> Result<Option<Vec<OsString>>, SafeError> {
        self.check(request)?;
        #[cfg(windows)]
        if let Some(entry) = self.npm_entry() {
            let mut args = entry.interpreter_prefix().to_vec();
            args.push(entry.entry().as_os_str().to_owned());
            args.extend_from_slice(entry.hook_args());
            args.extend(self.command_tail());
            return Ok(Some(args));
        }
        Ok(None)
    }
}
