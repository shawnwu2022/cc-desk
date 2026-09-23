//! Explicit, backend-only platform launches. No CLI selection or global environment mutation.
#![allow(dead_code)] // Connected to application run ownership in D11.

use crate::cli::environment::EnvMap;
use crate::cli::invocation::CliInvocation;
use crate::cli::profiles::{error, Dialect, Launcher};
use crate::cli::types::SafeError;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Own every input so dropping or editing the source profile cannot change a launch.
/// `program` is the selected executable/runner; `target` is checked separately for wrappers.
pub(crate) struct ProcessLaunchSpec {
    program: PathBuf,
    args: Vec<OsString>,
    cwd: PathBuf,
    environment: EnvMap,
    target: Option<PathBuf>,
}

impl std::fmt::Debug for ProcessLaunchSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProcessLaunchSpec(<redacted>)")
    }
}

fn require_file(path: &Path, code: &str, executable: bool) -> Result<(), SafeError> {
    if !path.is_absolute() {
        return Err(SafeError::invalid("programPath"));
    }
    let metadata = std::fs::metadata(path).map_err(|_| error(code))?;
    if !metadata.is_file() {
        return Err(error(code));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if executable && metadata.permissions().mode() & 0o111 == 0 {
            return Err(error(code));
        }
    }
    #[cfg(not(unix))]
    let _ = executable;
    Ok(())
}

impl ProcessLaunchSpec {
    /// Build immediately before spawn. This rejects missing selected paths/cwd;
    /// it does not pin file bytes or eliminate external filesystem races.
    pub(crate) fn command(&self) -> Result<CommandBuilder, SafeError> {
        if !self.cwd.is_absolute() || !self.cwd.is_dir() {
            return Err(error("WORKING_DIRECTORY_UNAVAILABLE"));
        }
        require_file(
            &self.program,
            if self.target.is_some() {
                "RUNNER_UNAVAILABLE"
            } else {
                "PROGRAM_UNAVAILABLE"
            },
            true,
        )?;
        if let Some(target) = &self.target {
            require_file(target, "PROGRAM_UNAVAILABLE", false)?;
        }
        let mut command = CommandBuilder::new(&self.program);
        // CommandBuilder::new imports a base environment. The snapshot is a
        // COMPLETE map: applying it as an overlay would resurrect deletions.
        command.env_clear();
        for (name, value) in &self.environment {
            command.env(name, value);
        }
        command.args(&self.args);
        command.cwd(&self.cwd);
        Ok(command)
    }
}

pub(crate) struct SpawnedProcess {
    pub(crate) master: Box<dyn MasterPty + Send>,
    pub(crate) child: Box<dyn Child + Send + Sync>,
}

fn has_extension(path: &Path, names: &[&str]) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|value| names.iter().any(|name| value.eq_ignore_ascii_case(name)))
}

fn text(value: &OsStr) -> Result<&str, SafeError> {
    value.to_str().ok_or_else(|| error("ARG_NOT_REPRESENTABLE"))
}

/// Normalize only a selected executable/script path for the Bash interpreter.
/// Never apply this conversion to caller argv values.
fn bash_path(path: &Path) -> Result<OsString, SafeError> {
    if cfg!(windows) {
        let value = text(path.as_os_str())?;
        if value.starts_with("\\\\?\\") || value.starts_with("\\\\.\\") {
            return Err(error("ARG_NOT_REPRESENTABLE"));
        }
        Ok(value.replace('\\', "/").into())
    } else {
        Ok(path.as_os_str().to_owned())
    }
}

/// Only hex bytes occur inside the ANSI-C literal. MSYS's incoming single-quote
/// parser never sees a caller apostrophe; decoded bytes remain one shell word.
fn bash_literal(value: &OsStr) -> Result<String, SafeError> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut literal = String::from("$'");
    for byte in text(value)?.bytes() {
        literal.push_str("\\x");
        literal.push(HEX[(byte >> 4) as usize] as char);
        literal.push(HEX[(byte & 15) as usize] as char);
    }
    literal.push('\'');
    Ok(literal)
}

fn bash_args(
    invocation: &CliInvocation<'_>,
    runner: &Path,
    shim: bool,
) -> Result<Vec<OsString>, SafeError> {
    let mut forwarded = Vec::new();
    if shim {
        forwarded.push(bash_path(runner)?);
        forwarded.extend(["--noprofile".into(), "--norc".into()]);
    }
    forwarded.push(bash_path(invocation.program())?);
    forwarded.extend_from_slice(invocation.args());
    let mut args: Vec<OsString> = ["--noprofile", "--norc", "-c"]
        .into_iter()
        .map(OsString::from)
        .collect();
    if cfg!(windows) {
        // CRT quoting alone is not MSYS argv quoting. Encode each word without
        // eval or external decoders; never expose caller bytes as shell syntax.
        let mut script = String::from("export MSYS2_ARG_CONV_EXCL='*'; exec");
        for value in &forwarded {
            script.push(' ');
            script.push_str(&bash_literal(value)?);
        }
        args.push(script.into());
    } else {
        args.extend(["exec \"$@\"".into(), "cc-desk".into()]);
        args.extend(forwarded);
    }
    Ok(args)
}

/// Data-only encoding for PowerShell's command source. No caller string is
/// interpolated as a PowerShell literal (including Unicode smart quote syntax).
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(a >> 2) as usize] as char);
        output.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}

fn powershell_args(invocation: &CliInvocation<'_>) -> Result<Vec<OsString>, SafeError> {
    if cfg!(windows) && has_extension(invocation.program(), &["cmd", "bat"]) {
        return Err(error("ARG_NOT_REPRESENTABLE"));
    }
    let args = invocation
        .args()
        .iter()
        .map(|arg| text(arg))
        .collect::<Result<Vec<_>, _>>()?;
    let data = serde_json::to_vec(&serde_json::json!({
        "program": text(invocation.program().as_os_str())?,
        "args": args,
    }))
    .map_err(|_| error("ARG_NOT_REPRESENTABLE"))?;
    let encoded = base64(&data);
    let script = format!(
        "if ($PSVersionTable.PSVersion -lt [version]'7.3') {{ \
         [Console]::Error.WriteLine('LAUNCHER_VERSION_UNSUPPORTED'); exit 125 }}; \
         $ErrorActionPreference='Stop'; $PSNativeCommandArgumentPassing='Standard'; \
         $PSNativeCommandUseErrorActionPreference=$false; \
         try {{ $d=ConvertFrom-Json -InputObject \
         ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{encoded}'))); \
         $a=@($d.args); $global:LASTEXITCODE=0; & $d.program @a; exit $LASTEXITCODE }} \
         catch {{ [Console]::Error.WriteLine('PROCESS_START_FAILED'); exit 126 }}"
    );
    Ok(vec![
        "-NoLogo".into(),
        "-NoProfile".into(),
        "-Command".into(),
        script.into(),
    ])
}

fn cmd_args(invocation: &CliInvocation<'_>) -> Result<Vec<OsString>, SafeError> {
    if !cfg!(windows) {
        return Err(error("UNSUPPORTED_LAUNCHER"));
    }
    // CALL undergoes an extra expansion pass. Reject anything outside the
    // tested literal subset; do not guess escaping for arbitrary batch syntax.
    for value in std::iter::once(invocation.program().as_os_str())
        .chain(invocation.args().iter().map(OsString::as_os_str))
    {
        if text(value)?.chars().any(|c| {
            c.is_control() || matches!(c, '"' | '%' | '!' | '^' | '&' | '|' | '<' | '>' | '(' | ')')
        }) {
            return Err(error("ARG_NOT_REPRESENTABLE"));
        }
    }
    let mut args: Vec<OsString> = ["/D", "/V:OFF", "/C", "call"]
        .into_iter()
        .map(OsString::from)
        .collect();
    args.push(invocation.program().as_os_str().to_owned());
    args.extend_from_slice(invocation.args());
    Ok(args)
}

pub(crate) fn resolve_process(
    invocation: &CliInvocation<'_>,
) -> Result<ProcessLaunchSpec, SafeError> {
    let (program, args, target) = match invocation.launcher() {
        Launcher::Native => {
            if cfg!(windows) && has_extension(invocation.program(), &["cmd", "bat", "ps1"]) {
                return Err(error("ARG_NOT_REPRESENTABLE"));
            }
            (
                invocation.program().to_owned(),
                invocation.args().to_vec(),
                None,
            )
        }
        Launcher::Shell { dialect, .. } | Launcher::Shim { dialect, .. } => {
            let runner = invocation
                .runner()
                .ok_or_else(|| SafeError::invalid("launcher"))?;
            let args = match dialect {
                Dialect::Bash => bash_args(
                    invocation,
                    runner,
                    matches!(invocation.launcher(), Launcher::Shim { .. }),
                )?,
                Dialect::PowerShell => powershell_args(invocation)?,
                Dialect::Cmd => cmd_args(invocation)?,
            };
            (
                runner.to_owned(),
                args,
                Some(invocation.program().to_owned()),
            )
        }
    };
    let cmd = matches!(
        invocation.launcher(),
        Launcher::Shell { dialect: Dialect::Cmd, .. }
            | Launcher::Shim { dialect: Dialect::Cmd, .. }
    );
    super::launch_limits::validate(&program, &args, invocation.environment(), cmd)?;
    Ok(ProcessLaunchSpec {
        program,
        args,
        cwd: invocation.cwd().to_owned(),
        environment: invocation.environment().clone(),
        target,
    })
}

/// Caller transfers these handles to the run registry/waiter. No global locks,
/// retries, process-name scans, CLI hooks, or output decoding occur here.
pub(crate) fn spawn_process(
    spec: &ProcessLaunchSpec,
    size: PtySize,
) -> Result<SpawnedProcess, SafeError> {
    if size.rows == 0 || size.cols == 0 {
        return Err(SafeError::invalid("terminalSize"));
    }
    let command = spec.command()?;
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|_| error("HOST_PTY_UNAVAILABLE"))?;
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|_| error("PROCESS_START_FAILED"))?;
    // Retaining the parent slave would prevent EOF on Unix.
    drop(pair.slave);
    Ok(SpawnedProcess {
        master: pair.master,
        child,
    })
}
