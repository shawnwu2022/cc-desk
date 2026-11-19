//! Explicit verification feature only. No raw path/argv or synchronous file hashing.
use std::{
    ffi::{c_void, OsStr},
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
};
static IDENTITY_VERIFIED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
pub(super) fn identity_verified() -> bool {
    IDENTITY_VERIFIED.load(Ordering::Acquire)
}
fn emit(stage: u8, code: u8, a: u32, b: u32) {
    let _ = writeln!(
        std::io::stderr().lock(),
        "CC_CANONICAL_DIAG stage={stage} code={code} a={a} b={b} elapsed_ms=0"
    );
}
fn expected() -> Option<String> {
    std::env::var("CC_DESK_DIAGNOSTIC_EXPECTED_PROGRAM")
        .ok()
        .filter(|s| !s.is_empty())
}
fn matches(value: &OsStr, expected: &str) -> bool {
    value.to_str().is_some_and(|s| {
        s.replace('/', "\\")
            .eq_ignore_ascii_case(&expected.replace('/', "\\"))
    })
}
pub(super) fn launch(command: &portable_pty::CommandBuilder) {
    let Some(expected) = expected() else {
        return;
    };
    static SEEN: AtomicBool = AtomicBool::new(false);
    if SEEN.swap(true, Ordering::Relaxed) {
        return;
    }
    let argv = command.get_argv();
    let program = argv.first().is_some_and(|v| matches(v, &expected));
    let args = ["--sandbox", "read-only", "--ask-for-approval", "on-request"];
    let args_match = argv.len() == 5 && argv[1..].iter().zip(args).all(|(a, b)| a == OsStr::new(b));
    emit(8, 0, u32::from(program), u32::from(args_match));
}
pub(super) fn created(handle: *mut c_void) {
    let Some(expected) = expected() else {
        return;
    };
    static SEEN: AtomicBool = AtomicBool::new(false);
    if SEEN.swap(true, Ordering::Relaxed) {
        return;
    }
    let result = (|| -> Result<(u32, u32), u8> {
        if handle.is_null() {
            return Err(1);
        }
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        // The caller still owns this original creation handle. Never reopen by PID.
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut size) } == 0 {
            return Err(2);
        }
        let image = String::from_utf16(&path[..size as usize]).map_err(|_| 3)?;
        let pid = unsafe { GetProcessId(handle) };
        if pid == 0 {
            return Err(4);
        }
        let exited = match unsafe { WaitForSingleObject(handle, 0) } {
            0 => true,
            258 => false,
            _ => return Err(5),
        };
        // b bit0=image path equals the prehashed supply; bit1=already exited.
        Ok((
            pid,
            u32::from(matches(OsStr::new(&image), &expected)) | (u32::from(exited) << 1),
        ))
    })();
    match result {
        Ok((pid, flags)) => {
            IDENTITY_VERIFIED.store(flags == 1, Ordering::Release);
            emit(9, 0, pid, flags);
        }
        Err(code) => emit(9, code, 0, 0),
    }
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryFullProcessImageNameW(
        handle: *mut c_void,
        flags: u32,
        name: *mut u16,
        size: *mut u32,
    ) -> i32;
    fn GetProcessId(handle: *mut c_void) -> u32;
    fn WaitForSingleObject(handle: *mut c_void, millis: u32) -> u32;
}
