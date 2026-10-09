//! Size validation for the pinned portable-pty Windows command-line serializer.
use crate::cli::environment::EnvMap;
use crate::cli::types::SafeError;
use std::ffi::OsString;
use std::path::Path;

/// No truncation: reject a command that the selected parser cannot represent.
pub(super) fn validate(
    program: &Path,
    args: &[OsString],
    environment: &EnvMap,
    cmd: bool,
) -> Result<(), SafeError> {
    #[cfg(windows)]
    {
        use crate::cli::profiles::error;
        use std::os::windows::ffi::OsStrExt;

        // Includes argv[0], separating spaces, CRT quoting, and the final NUL.
        let length = args.iter().fold(
            quoted_length(program.as_os_str()).saturating_add(1),
            |total, arg| total.saturating_add(1).saturating_add(quoted_length(arg)),
        );
        let maximum = if cmd { 8191 } else { 32767 };
        if length > maximum {
            return Err(error("ARG_NOT_REPRESENTABLE"));
        }
        if cmd {
            // cmd can silently ignore oversized inherited values. The stricter
            // complete entry bound is conservative and must not affect Native.
            for (name, value) in environment {
                let length = name
                    .encode_wide()
                    .count()
                    .saturating_add(value.encode_wide().count())
                    .saturating_add(2);
                if length > 8191 {
                    return Err(error("ENV_NOT_REPRESENTABLE"));
                }
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (program, args, environment, cmd);
    Ok(())
}

/// Count the same quoting used by portable-pty 0.8.1 without creating or logging
/// a printable command line. Count UTF-16 code units, not UTF-8 bytes/characters.
#[cfg(windows)]
fn quoted_length(value: &std::ffi::OsStr) -> usize {
    use std::os::windows::ffi::OsStrExt;
    let quoted = value.is_empty()
        || value
            .encode_wide()
            .any(|unit| matches!(unit, 0x20 | 0x09 | 0x0a | 0x0b | 0x22));
    if !quoted {
        return value.encode_wide().count();
    }
    let mut length: usize = 2;
    let mut backslashes: usize = 0;
    for unit in value.encode_wide() {
        if unit == 0x5c {
            backslashes = backslashes.saturating_add(1);
        } else {
            length = length.saturating_add(if unit == 0x22 {
                backslashes.saturating_mul(2).saturating_add(2)
            } else {
                backslashes.saturating_add(1)
            });
            backslashes = 0;
        }
    }
    length.saturating_add(backslashes.saturating_mul(2))
}
