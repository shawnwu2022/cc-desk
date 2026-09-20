//! Windows process bootstrap: verify and pin app-local ConPTY before portable-pty
//! can resolve its lazy loader. No PATH/CWD lookup, download, or silent fallback.
use serde::Deserialize;
use std::ffi::{c_void, OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
#[link(name = "bcrypt")]
extern "system" {
    fn BCryptOpenAlgorithmProvider(
        handle: *mut *mut c_void,
        algorithm: *const u16,
        implementation: *const u16,
        flags: u32,
    ) -> i32;
    fn BCryptHash(
        handle: *mut c_void,
        secret: *mut u8,
        secret_len: u32,
        input: *mut u8,
        input_len: u32,
        output: *mut u8,
        output_len: u32,
    ) -> i32;
    fn BCryptCloseAlgorithmProvider(handle: *mut c_void, flags: u32) -> i32;
}
#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(owner: *mut c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    files: Vec<PinnedFile>,
}
#[derive(Deserialize)]
struct PinnedFile {
    name: String,
    bytes: u64,
    sha256: String,
}
struct Module(usize);
impl Drop for Module {
    fn drop(&mut self) {
        // One owned LoadLibraryEx reference; retained for the process lifetime.
        unsafe { FreeLibrary(self.0 as *mut c_void) };
    }
}
struct Runtime {
    module: Module,
    _files: Vec<File>, // Deny writes/deletes between verification and host spawn.
    directory: PathBuf,
    version: String,
}
static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(Some(0)).collect()
}
fn hash(data: &[u8]) -> Result<String, String> {
    let len = u32::try_from(data.len()).map_err(|_| "Runtime file is too large")?;
    let mut provider = null_mut();
    let algorithm = wide(OsStr::new("SHA256"));
    // BCryptHash is supported from Windows 10, below this app's ConPTY minimum.
    let status = unsafe { BCryptOpenAlgorithmProvider(&mut provider, algorithm.as_ptr(), null(), 0) };
    if status < 0 {
        return Err(format!("Cannot open SHA-256 provider: {status:#x}"));
    }
    let mut out = [0u8; 32];
    // Microsoft documents pbInput as read-only despite its mutable C pointer.
    let status = unsafe {
        BCryptHash(provider, null_mut(), 0, data.as_ptr().cast_mut(), len, out.as_mut_ptr(), 32)
    };
    unsafe { BCryptCloseAlgorithmProvider(provider, 0) };
    if status < 0 {
        return Err(format!("SHA-256 failed: {status:#x}"));
    }
    Ok(out.iter().map(|byte| format!("{byte:02x}")).collect())
}
fn verify_file(dir: &Path, expected: &PinnedFile) -> Result<File, String> {
    let path = dir.join(&expected.name);
    let canonical = path.canonicalize().map_err(|e| format!("Missing {}: {e}", expected.name))?;
    if canonical != path {
        return Err(format!("Runtime file must be in the application directory: {}", expected.name));
    }
    let mut file = OpenOptions::new().read(true).share_mode(1).open(&path)
        .map_err(|e| format!("Cannot lock {} for verification: {e}", expected.name))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size != expected.bytes || size > 4 * 1024 * 1024 {
        return Err(format!("Wrong size: {}", expected.name));
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if hash(&bytes)? != expected.sha256 {
        return Err(format!("SHA-256 mismatch: {}", expected.name));
    }
    if expected.name.ends_with(".dll") || expected.name.ends_with(".exe") {
        let offset = bytes.get(60..64).map(|p| u32::from_le_bytes(p.try_into().unwrap()) as usize);
        let valid = offset.and_then(|off| bytes.get(off..off.checked_add(6)?))
            .is_some_and(|header| &header[..4] == b"PE\0\0" && header[4..] == [0x64, 0x86]);
        if !bytes.starts_with(b"MZ") || !valid {
            return Err(format!("Not an x64 PE file: {}", expected.name));
        }
    }
    Ok(file)
}
fn module_path(module: *mut c_void) -> Result<PathBuf, String> {
    let mut name = vec![0u16; 32768];
    let size = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) } as usize;
    if size == 0 || size >= name.len() {
        return Err("Cannot establish loaded ConPTY path".into());
    }
    PathBuf::from(OsString::from_wide(&name[..size])).canonicalize().map_err(|e| e.to_string())
}
fn load_from(directory: &Path) -> Result<Runtime, String> {
    if !cfg!(target_arch = "x86_64") {
        return Err("Bundled ConPTY supports Windows x64 only".into());
    }
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let manifest: Manifest = serde_json::from_str(include_str!("../conpty/manifest.json"))
        .map_err(|e| e.to_string())?;
    let files = manifest.files.iter().map(|file| verify_file(&directory, file)).collect::<Result<Vec<_>, _>>()?;
    let dll = directory.join("conpty.dll");
    let basename = wide(OsStr::new("conpty.dll"));
    let prior = unsafe { GetModuleHandleW(basename.as_ptr()) };
    if !prior.is_null() && module_path(prior)? != dll {
        return Err("An unexpected ConPTY module is already loaded; restart from the installed application".into());
    }
    let absolute = wide(dll.as_os_str());
    // Absolute DLL path + only its directory and System32 for dependencies.
    // Do not change the process-wide DLL search policy (WebView compatibility).
    let handle = unsafe { LoadLibraryExW(absolute.as_ptr(), null_mut(), 0x100 | 0x1000) };
    if handle.is_null() {
        return Err(format!("Cannot load bundled ConPTY: {}", std::io::Error::last_os_error()));
    }
    let module = Module(handle as usize);
    for export in [b"CreatePseudoConsole\0".as_slice(), b"ResizePseudoConsole\0", b"ClosePseudoConsole\0"] {
        if unsafe { GetProcAddress(handle, export.as_ptr()) }.is_null() {
            return Err("Bundled ConPTY is missing a required export".into());
        }
    }
    // The basename lookup portable-pty uses must resolve our pinned reference.
    if unsafe { GetModuleHandleW(basename.as_ptr()) } != handle || module_path(handle)? != dll {
        return Err("ConPTY loader resolved an unexpected module".into());
    }
    Ok(Runtime { module, _files: files, directory, version: manifest.version })
}
fn runtime() -> Result<&'static Runtime, String> {
    RUNTIME.get_or_init(|| {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        load_from(exe.parent().ok_or("No application directory")?)
    }).as_ref().map_err(Clone::clone)
}
pub fn initialize() -> Result<(), String> {
    runtime().map(|_| ())
}
pub fn show_error(error: &str) {
    let text = wide(OsStr::new(&format!(
        "CC Desk 无法加载配套控制台运行库，已停止启动，未回退到系统后端。\n请重新安装完整的 CC Desk 软件包，不要单独移动 EXE。\n\n{error}"
    )));
    let title = wide(OsStr::new("CC Desk — ConPTY"));
    unsafe { MessageBoxW(null_mut(), text.as_ptr(), title.as_ptr(), 0x10) };
}
/// Non-interactive installer/CI probe; no user configuration, CLI or API calls.
pub fn check_report(destination: &Path) -> i32 {
    let result = (|| -> Result<serde_json::Value, String> {
        let runtime = runtime()?;
        use portable_pty::{native_pty_system, PtySize};
        let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        pair.master.resize(PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 }).map_err(|e| e.to_string())?;
        drop(pair);
        let actual = module_path(runtime.module.0 as *mut c_void)?;
        if actual != runtime.directory.join("conpty.dll") { return Err("ConPTY identity changed".into()); }
        Ok(serde_json::json!({"ok":true,"backend":"bundled","version":runtime.version,
            "dll":actual,"host":runtime.directory.join("OpenConsole.exe"),
            "build":env!("CC_DESK_BUILD_SHA"),"ptyLifecycle":true}))
    })();
    let code = if result.is_ok() { 0 } else { 2 };
    let report = result.unwrap_or_else(|error| serde_json::json!({"ok":false,"error":error}));
    if std::fs::write(destination, serde_json::to_vec_pretty(&report).unwrap()).is_err() { return 3; }
    code
}

#[cfg(test)]
#[allow(non_snake_case)]
#[path = "tests/conpty_runtime.rs"]
mod tests;
