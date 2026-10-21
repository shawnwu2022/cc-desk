//! Resolve only backend-owned launch environment. An empty/invalid explicit root is not a default.
use super::scoped_fs::{ReadResult, Root};
use crate::cli::environment::{lookup, EnvMap};
use crate::cli::profiles::{Dialect, Launcher};
use crate::cli::types::CliKind;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

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
    pub sqlite_file: Option<String>,
}
impl Locations {
    pub(crate) fn open_root(&self, cli: CliKind) -> ReadResult<Root> {
        if cli == CliKind::Opencode {
            Root::open_sqlite(
                &self.root,
                self.sqlite_file.as_deref().ok_or("SCOPE_UNKNOWN")?,
            )
        } else {
            Root::open(&self.root)
        }
    }
    pub(crate) fn with_probed_database(&self, output: &str) -> ReadResult<Self> {
        if self.sqlite_file.is_some() {
            return Err("SCOPE_UNKNOWN");
        }
        let output = output.trim_end_matches(['\r', '\n']);
        if output.contains(['\r', '\n']) {
            return Err("SCOPE_UNKNOWN");
        }
        let database = Path::new(output);
        valid(database)?;
        let file = database
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or("SCOPE_UNKNOWN")?;
        let channel = file
            .strip_prefix("opencode-")
            .and_then(|name| name.strip_suffix(".db"));
        if file != "opencode.db"
            && !channel.is_some_and(|value| {
                !value.is_empty()
                    && value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
            })
        {
            return Err("SCOPE_UNKNOWN");
        }
        let parent = database.parent().ok_or("SCOPE_UNKNOWN")?;
        if Root::open(parent)?.key() != Root::open(&self.root)?.key() {
            return Err("SCOPE_UNKNOWN");
        }
        let mut selected = self.clone();
        selected.sqlite_file = Some(file.to_owned());
        Ok(selected)
    }
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
            sqlite_file: None,
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
            sqlite_file: None,
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
    let explicit = lookup(env, OsStr::new("OPENCODE_DB"))
        .map(|value| value.to_str().ok_or("SCOPE_UNKNOWN"))
        .transpose()?
        .filter(|value| !value.is_empty());
    let database = match explicit {
        Some(":memory:") => return Err("SCOPE_UNKNOWN"),
        Some(value) => {
            let value = Path::new(value);
            if value.is_absolute() {
                value.to_path_buf()
            } else {
                root.join(value)
            }
        }
        None if lookup(env, OsStr::new("OPENCODE_DISABLE_CHANNEL_DB"))
            .is_some_and(|value| value == "1" || value == "true") =>
        {
            root.join("opencode.db")
        }
        None => {
            return Ok(Locations {
                root,
                user_config: None,
                sqlite_file: None,
            })
        }
    };
    valid(&database)?;
    let file = match database.file_name().and_then(OsStr::to_str) {
        Some(name)
            if !name.is_empty()
                && (!cfg!(windows) || !name.contains(':'))
                && Path::new(name)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))) =>
        {
            name.to_owned()
        }
        _ => return Err("SCOPE_UNKNOWN"),
    };
    let parent = database.parent().ok_or("SCOPE_UNKNOWN")?.to_path_buf();
    valid(&parent)?;
    Ok(Locations {
        root: parent,
        user_config: None,
        sqlite_file: Some(file),
    })
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
