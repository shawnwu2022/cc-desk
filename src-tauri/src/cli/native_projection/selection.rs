//! Resolve only backend-owned launch environment. An empty/invalid explicit root is not a default.
use super::scoped_fs::ReadResult;
use crate::cli::environment::{lookup, EnvMap};
use crate::cli::profiles::{Dialect, Launcher};
use crate::cli::types::CliKind;
use std::ffi::OsStr;
use std::path::PathBuf;

// This admits a read observation of the backend-selected root, not proof of
// effective CLI state or launcher certification. Explicit Cmd shims (including
// npm launchers) do not themselves change that observation's authority.
pub(crate) fn read_scope_known<I, A>(cli: CliKind, launcher: &Launcher, args: I) -> bool
where
    I: IntoIterator<Item = A>,
    A: AsRef<OsStr>,
{
    if cli == CliKind::Shell
        || !matches!(
            launcher,
            Launcher::Native
                | Launcher::Shim {
                    dialect: Dialect::Cmd,
                    ..
                }
        )
    {
        return false;
    }
    // Complete, zero-value flags only: no guessing unknown flags, option values,
    // positional prompts, config overrides, or cwd-changing syntax.
    let mut permission_flag = None;
    for arg in args {
        match (cli, arg.as_ref().to_str()) {
            (CliKind::Codex, Some("--no-alt-screen")) => {}
            (
                CliKind::Codex,
                Some(flag @ ("--full-auto" | "--dangerously-bypass-approvals-and-sandbox")),
            ) => {
                if permission_flag.is_some_and(|old| old != flag) {
                    return false;
                }
                // Store only which known flag was observed, never user argv.
                permission_flag = Some(if flag == "--full-auto" {
                    "--full-auto"
                } else {
                    "--dangerously-bypass-approvals-and-sandbox"
                });
            }
            (CliKind::Claude, Some("--dangerously-skip-permissions")) => {}
            _ => return false,
        }
    }
    true
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Locations {
    pub root: PathBuf,
    pub user_config: Option<PathBuf>,
}
pub(crate) fn locations(cli: CliKind, env: &EnvMap) -> ReadResult<Locations> {
    let (key, default) = match cli {
        CliKind::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
        CliKind::Codex => ("CODEX_HOME", ".codex"),
        // opencode 数据根由 XDG 决定，覆盖键就是 XDG_DATA_HOME 本身。
        CliKind::Opencode => return opencode_locations(env),
        CliKind::Shell => return Err("SCOPE_UNKNOWN"),
    };
    let explicit = lookup(env, OsStr::new(key));
    let result = if let Some(root) = explicit {
        Locations {
            root: PathBuf::from(root),
            user_config: None,
        }
    } else {
        let home = home(env)?;
        Locations {
            root: home.join(default),
            user_config: if cli == CliKind::Claude {
                Some(home)
            } else {
                None
            },
        }
    };
    valid(&result.root)?;
    Ok(result)
}
fn home(env: &EnvMap) -> ReadResult<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let value = lookup(env, OsStr::new(key)).ok_or("SCOPE_UNKNOWN")?;
    let path = PathBuf::from(value);
    valid(&path)?;
    Ok(path)
}
fn opencode_locations(env: &EnvMap) -> ReadResult<Locations> {
    let base = match lookup(env, OsStr::new("XDG_DATA_HOME")) {
        Some(xdg) => PathBuf::from(xdg),
        None => home(env)?.join(".local").join("share"),
    };
    let root = base.join("opencode");
    valid(&root)?;
    if let Some(database) = lookup(env, OsStr::new("OPENCODE_DB")) {
        let database = database.to_str().ok_or("SCOPE_UNKNOWN")?;
        // 上游非空 override 可以是内存库或数据根相对/绝对文件；本 reader
        // 目前只授权固定 basename。不能拿默认库冒充自定义库的历史。
        if !database.is_empty() && database != "opencode.db" {
            let candidate = PathBuf::from(database);
            if !candidate.is_absolute() || candidate != root.join("opencode.db") {
                return Err("SCOPE_UNKNOWN");
            }
        }
    }
    Ok(Locations { root, user_config: None })
}
fn valid(path: &std::path::Path) -> ReadResult<()> {
    let value = path.to_str().ok_or("SCOPE_UNKNOWN")?;
    if !path.is_absolute() || value.is_empty() || value.len() > 32768 || value.contains('\0') {
        return Err("SCOPE_UNKNOWN");
    }
    Ok(())
}
#[cfg(test)]
#[path = "../../tests/native_cli_projection_selection.rs"]
mod tests;
