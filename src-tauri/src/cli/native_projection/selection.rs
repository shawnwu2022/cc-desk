//! Resolve only backend-owned launch environment. An empty/invalid explicit root is not a default.
use super::scoped_fs::ReadResult;
use crate::cli::environment::{lookup, EnvMap};
use crate::cli::types::CliKind;
use std::path::PathBuf;
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Locations {
    pub root: PathBuf,
    pub user_config: Option<PathBuf>,
}
pub(crate) fn locations(cli: CliKind, env: &EnvMap) -> ReadResult<Locations> {
    use std::ffi::OsStr;
    let (key, default) = match cli {
        CliKind::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
        CliKind::Codex => ("CODEX_HOME", ".codex"),
        CliKind::Shell => return Err("SCOPE_UNKNOWN"),
    };
    let explicit = lookup(env, OsStr::new(key));
    let home = || -> ReadResult<PathBuf> {
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let value = lookup(env, OsStr::new(key)).ok_or("SCOPE_UNKNOWN")?;
        let path = PathBuf::from(value);
        valid(&path)?;
        Ok(path)
    };
    let result = if let Some(root) = explicit {
        Locations {
            root: PathBuf::from(root),
            user_config: None,
        }
    } else {
        let home = home()?;
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
