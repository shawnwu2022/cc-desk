//! 已审核 npm 包装的按次 argv 转发；不改变用户选择的 cmd/Node/原生 CLI。
use super::environment::{lookup, EnvMap};
use super::profiles::error;
use super::types::SafeError;
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

pub(crate) const ENV_NAMES: &[&str] = &["CC_DESK_CODEX_ARGV", "CC_DESK_CODEX_NODE_OPTIONS"];

// 只支持实际验收过的 npm 0.163.0 包装；换版或自定义包装保留普通启动降级。
const SHIM_HASH: &str = "0fd52614bcc23d6bf2ef74408a948452157d6ab77c02db200a351a499a3df5e6";
const ENTRY_HASH: &str = "61b0194f3bb6534439c8d26a3ed57d0805f84b884588b761795323eeb92fcf70";

pub(crate) struct Transport {
    // 先释放禁止写删的句柄，再清理临时目录；跟随实际 owned snapshot 生命周期。
    _guards: Vec<File>,
    _temp: tempfile::TempDir,
}

fn unavailable() -> SafeError {
    error("OBSERVER_PREPARE_FAILED")
}

fn dos_path(path: &Path) -> Result<String, SafeError> {
    let path = path.to_str().ok_or_else(unavailable)?.replace('/', "\\");
    Ok(if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    })
}

fn literal_path(path: &Path) -> Result<PathBuf, SafeError> {
    let canonical = std::fs::canonicalize(path).map_err(|_| unavailable())?;
    // 不跟随可独立替换的 junction/短名别名；不据此改选用户启动目标。
    if !dos_path(path)?.eq_ignore_ascii_case(&dos_path(&canonical)?) {
        return Err(unavailable());
    }
    Ok(canonical)
}

fn identity(file: &File) -> Result<(u32, u32, u32), SafeError> {
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
    };
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // 句柄与输出缓冲区在同步调用期间保持有效，不将文件身份发布到前端。
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut information) }
        .map_err(|_| unavailable())?;
    Ok((
        information.dwVolumeSerialNumber,
        information.nFileIndexHigh,
        information.nFileIndexLow,
    ))
}

fn final_path(file: &File) -> Result<PathBuf, SafeError> {
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{GetFinalPathNameByHandleW, FILE_NAME_NORMALIZED},
    };
    let mut buffer = vec![0u16; 32768];
    // 从已持有对象读取最终路径，不通过可被 ABA 替换的 lexical 路径间接推断。
    let length = unsafe {
        GetFinalPathNameByHandleW(
            HANDLE(file.as_raw_handle()),
            &mut buffer,
            FILE_NAME_NORMALIZED,
        )
    } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(unavailable());
    }
    Ok(PathBuf::from(
        String::from_utf16(&buffer[..length]).map_err(|_| unavailable())?,
    ))
}

fn hold_parent(path: &Path, guards: &mut Vec<File>) -> Result<(), SafeError> {
    // Windows 禁止删除的子目录句柄也阻止祖先重命名；不额外要求读取用户根目录。
    guards.push(
        OpenOptions::new()
            .read(true)
            .share_mode(3) // READ | WRITE，禁止替换目录路径。
            .custom_flags(0x02000000) // FILE_FLAG_BACKUP_SEMANTICS
            .open(path.parent().ok_or_else(unavailable)?)
            .map_err(|_| unavailable())?,
    );
    Ok(())
}

fn held_text(path: &Path, guards: &mut Vec<File>) -> Result<String, SafeError> {
    let canonical = literal_path(path)?;
    hold_parent(&canonical, guards)?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1) // FILE_SHARE_READ，审核到执行之间不得改写或替换。
        .open(path)
        .map_err(|_| unavailable())?;
    if !file.metadata().map_err(|_| unavailable())?.is_file() {
        return Err(unavailable());
    }
    if !dos_path(path)?.eq_ignore_ascii_case(&dos_path(&final_path(&file)?)?) {
        return Err(unavailable());
    }
    // 持有目录/文件后重新核对 lexical 路径和实际打开对象，拒绝检查间隙替换。
    let current = literal_path(path)?;
    let check = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&current)
        .map_err(|_| unavailable())?;
    if canonical != current || identity(&file)? != identity(&check)? {
        return Err(unavailable());
    }
    let mut text = String::new();
    (&mut file)
        .take(128 * 1024 + 1)
        .read_to_string(&mut text)
        .map_err(|_| unavailable())?;
    if text.len() > 128 * 1024 {
        return Err(unavailable());
    }
    guards.push(file);
    Ok(text)
}

fn matches_hash(text: &str, expected: &str) -> bool {
    format!("{:x}", Sha256::digest(text.replace("\r\n", "\n"))) == expected
}

pub(crate) fn prepare(
    program: &Path,
    args: &[OsString],
    environment: &mut EnvMap,
) -> Result<Transport, SafeError> {
    // Node 权限等选项可在执行 CJS 前拒绝读取；不猜测、重解析或覆盖用户选项。
    let original = lookup(environment, OsStr::new("NODE_OPTIONS"));
    if original.is_some_and(|value| !value.is_empty())
        || program
            .file_name()
            .is_none_or(|name| !name.eq_ignore_ascii_case(OsStr::new("codex.cmd")))
    {
        return Err(unavailable());
    }
    let original_empty = original.is_some();
    let npm = program.parent().ok_or_else(unavailable)?;
    let package = npm.join("node_modules/@openai/codex");
    let entry = package.join("bin/codex.js");
    let mut guards = Vec::new();
    // 同时持有用户选择的拼写与实际文件祖先，防止链接/目录替换改变入口。
    hold_parent(program, &mut guards)?;
    if !matches_hash(&held_text(program, &mut guards)?, SHIM_HASH)
        || !matches_hash(&held_text(&entry, &mut guards)?, ENTRY_HASH)
    {
        return Err(unavailable());
    }
    let manifest: serde_json::Value =
        serde_json::from_str(&held_text(&package.join("package.json"), &mut guards)?)
            .map_err(|_| unavailable())?;
    if manifest["name"] != "@openai/codex"
        || manifest["version"] != "0.163.0"
        || manifest["bin"]["codex"] != "bin/codex.js"
    {
        return Err(unavailable());
    }
    let args = args
        .iter()
        .map(|arg| arg.to_str().ok_or_else(unavailable))
        .collect::<Result<Vec<_>, _>>()?;
    let canonical_entry = std::fs::canonicalize(&entry).map_err(|_| unavailable())?;
    let payload =
        serde_json::json!({"entry":canonical_entry.to_str().ok_or_else(unavailable)?, "args":args});
    let temp = tempfile::Builder::new()
        .prefix("cc-desk-codex-")
        .tempdir()
        .map_err(|_| unavailable())?;
    let mut transport = Transport {
        _guards: guards,
        _temp: temp,
    };
    let loader = std::fs::canonicalize(transport._temp.path())
        .map_err(|_| unavailable())?
        .join("argv.cjs");
    hold_parent(&loader, &mut transport._guards)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(1)
        .open(&loader)
        .map_err(|_| unavailable())?;
    file.write_all(include_bytes!("codex_cmd_preload.cjs"))
        .map_err(|_| unavailable())?;
    file.flush().map_err(|_| unavailable())?;
    transport._guards.push(file);
    // Node 的 --require 不接受转成 //?/ 的 Win32 扩展前缀；只转换已持有的私有文件。
    let loader = dos_path(&loader)?.replace('\\', "/");
    if loader.chars().any(|c| c.is_control() || c == '"') {
        return Err(unavailable());
    }
    environment.retain(|name, _| !super::environment::same_name(name, OsStr::new("NODE_OPTIONS")));
    environment.insert(
        "NODE_OPTIONS".into(),
        format!("--require \"{loader}\"").into(),
    );
    environment.insert(ENV_NAMES[0].into(), payload.to_string().into());
    environment.insert(
        ENV_NAMES[1].into(),
        if original_empty { "empty" } else { "unset" }.into(),
    );
    Ok(transport)
}
