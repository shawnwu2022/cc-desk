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

// 0.164.0 的入口与 shim 字节和 0.163.0 相同，且已验证 session-flags hook 元数据。
// 版本与字节必须同时命中；未来或自定义包装仍保留普通启动降级。
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

/// 可选原生能力探测只执行这个保持打开且身份核实过的直接路径。
pub(crate) fn hold_probe_target(path: &Path) -> Result<Vec<File>, SafeError> {
    let canonical = literal_path(path)?;
    let mut guards = Vec::new();
    hold_parent(&canonical, &mut guards)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|_| unavailable())?;
    if !file.metadata().map_err(|_| unavailable())?.is_file()
        || !dos_path(path)?.eq_ignore_ascii_case(&dos_path(&final_path(&file)?)?)
    {
        return Err(unavailable());
    }
    let current = literal_path(path)?;
    let check = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&current)
        .map_err(|_| unavailable())?;
    if canonical != current || identity(&file)? != identity(&check)? {
        return Err(unavailable());
    }
    guards.push(file);
    Ok(guards)
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
    if manifest["name"] != "@openai/codex" || manifest["bin"]["codex"] != "bin/codex.js" {
        return Err(unavailable());
    }
    // Package versions also gate hook protocol admission; matching wrapper bytes
    // alone never grants an unreviewed native core the authenticated environment.
    if !matches!(
        manifest["version"].as_str(),
        Some("0.163.0" | "0.164.0" | "0.165.0")
    ) {
        let mut failure = unavailable();
        failure.field = Some("codexPackageVersion".into());
        return Err(failure);
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

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn CodexIdentity_Npm164PreservesAuthenticatedTransport_082() {
        npm_transport_replay("0.164.0");
    }

    #[test]
    fn CodexIdentity_Npm165PreservesAuthenticatedTransport_083() {
        npm_transport_replay("0.165.0");
    }

    fn npm_fixture(version: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(root.path()).unwrap();
        let npm = PathBuf::from(dos_path(&canonical).unwrap()).join("npm");
        let package = npm.join("node_modules/@openai/codex");
        std::fs::create_dir_all(package.join("bin")).unwrap();
        let program = npm.join("codex.cmd");
        std::fs::write(
            &program,
            include_str!("../../../tests/fixtures/native-cli/codex-npm-0.163.0/codex.cmd"),
        )
        .unwrap();
        std::fs::write(
            package.join("bin/codex.js"),
            include_str!("../../../tests/fixtures/native-cli/codex-npm-0.163.0/codex.js"),
        )
        .unwrap();
        std::fs::write(
            package.join("package.json"),
            serde_json::to_vec(&serde_json::json!({"name":"@openai/codex","version":version,"bin":{"codex":"bin/codex.js"}})).unwrap(),
        )
        .unwrap();
        (root, program, package)
    }

    fn npm_transport_replay(version: &str) {
        let (_root, program, package) = npm_fixture(version);
        let mut environment = EnvMap::new();
        let args = [
            "-c",
            "hooks.SessionStart=[]",
            "-c",
            "hooks.UserPromptSubmit=[]",
            "-c",
            "hooks.Stop=[]",
            "-c",
            "hooks.state={}",
        ]
        .map(OsString::from);
        let transport = prepare(&program, &args, &mut environment)
            .expect("已审核的相同 npm 包装升级后仍应传输认证身份 hooks");
        assert!(lookup(&environment, OsStr::new("CC_DESK_CODEX_ARGV")).is_some());
        assert!(lookup(&environment, OsStr::new("NODE_OPTIONS")).is_some());
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
        let output = std::process::Command::new("node")
            .env_clear()
            .envs(&environment)
            .args(["-e", "process.stdout.write(JSON.stringify({argv:process.argv.slice(2),options:process.env.NODE_OPTIONS??null,transport:process.env.CC_DESK_CODEX_ARGV??null}))"])
            .arg(package.join("bin/codex.js"))
            .arg("resume")
            .arg("exact-thread")
            .output()
            .unwrap();
        assert!(output.status.success());
        let observed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut expected = args.iter().map(|v| v.to_str().unwrap()).collect::<Vec<_>>();
        expected.extend(["resume", "exact-thread"]);
        assert_eq!(observed["argv"], serde_json::json!(expected));
        assert!(observed["options"].is_null());
        assert!(observed["transport"].is_null());
        drop(transport);
        // 相同包装不代表任意未来版本都已验证。
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","version":"0.166.0","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        assert!(prepare(&program, &args, &mut EnvMap::new()).is_err());
    }

    fn fixture_args() -> [OsString; 8] {
        [
            "-c",
            "hooks.SessionStart=[]",
            "-c",
            "hooks.UserPromptSubmit=[]",
            "-c",
            "hooks.Stop=[]",
            "-c",
            "hooks.state={}",
        ]
        .map(OsString::from)
    }

    #[test]
    fn CodexIdentity_NpmUnknownVersionHasFixedReason_085() {
        let (_root, program, _package) = npm_fixture("0.166.0");
        let failure = prepare(&program, &fixture_args(), &mut EnvMap::new())
            .err()
            .expect("unreviewed version must fail closed");
        assert_eq!(failure.code, "OBSERVER_PREPARE_FAILED");
        assert_eq!(failure.field.as_deref(), Some("codexPackageVersion"));
    }

    #[test]
    fn CodexIdentity_NpmKnownVersionRejectsTampering_084() {
        for target in ["shim", "entry"] {
            let (_root, program, package) = npm_fixture("0.164.0");
            drop(
                prepare(&program, &fixture_args(), &mut EnvMap::new())
                    .expect("known-version control must pass before tamper"),
            );
            let path = if target == "shim" {
                program.clone()
            } else {
                package.join("bin/codex.js")
            };
            let mut bytes = std::fs::read(&path).unwrap();
            bytes.extend_from_slice(b"\nchanged bytes\n");
            std::fs::write(path, bytes).unwrap();
            let mut env = EnvMap::new();
            let failure = prepare(&program, &fixture_args(), &mut env)
                .err()
                .expect("changed audited bytes must be rejected");
            assert_eq!(failure.code, "OBSERVER_PREPARE_FAILED");
            assert!(failure.field.is_none());
            assert!(env.is_empty());
        }
        for (field, value) in [
            ("name", serde_json::json!("@other/codex")),
            ("bin", serde_json::json!({"codex":"bin/other.js"})),
            ("bin", serde_json::json!("bin/codex.js")),
            ("bin", serde_json::json!({"codex":5})),
        ] {
            let (_root, program, package) = npm_fixture("0.164.0");
            drop(
                prepare(&program, &fixture_args(), &mut EnvMap::new())
                    .expect("known-version control must pass before manifest mutation"),
            );
            let mut manifest = serde_json::json!({"name":"@openai/codex", "version":"0.164.0", "bin":{"codex":"bin/codex.js"}});
            manifest[field] = value;
            std::fs::write(
                package.join("package.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            let mut env = EnvMap::new();
            let failure = prepare(&program, &fixture_args(), &mut env)
                .err()
                .expect("manifest identity must remain exact");
            assert_eq!(failure.code, "OBSERVER_PREPARE_FAILED");
            assert!(failure.field.is_none());
            assert!(env.is_empty());
        }
        let (_root, program, _package) = npm_fixture("0.164.0");
        drop(
            prepare(&program, &fixture_args(), &mut EnvMap::new())
                .expect("known-version control must pass before NODE_OPTIONS mutation"),
        );
        let mut env = EnvMap::from([("node_options".into(), "--require user-owned.cjs".into())]);
        let before = env.clone();
        let failure = prepare(&program, &fixture_args(), &mut env)
            .err()
            .expect("nonempty NODE_OPTIONS must remain user-owned");
        assert!(failure.field.is_none());
        assert!(env == before);
    }
}
