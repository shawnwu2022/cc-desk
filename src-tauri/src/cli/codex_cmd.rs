//! 已审核 npm 包装的按次 argv 转发；不改变用户选择的 cmd/Node/原生 CLI。
use super::environment::{lookup, EnvMap};
use super::profiles::error;
use super::types::SafeError;
use std::ffi::{OsStr, OsString};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

pub(crate) const ENV_NAMES: &[&str] = &["CC_DESK_CODEX_ARGV", "CC_DESK_CODEX_NODE_OPTIONS"];

pub(crate) struct Transport {
    // 先释放禁止写删的句柄，再清理临时目录；跟随实际 owned snapshot 生命周期。
    _guards: Vec<File>,
    _temp: tempfile::TempDir,
    entry: ProvenNpmEntry,
}

/// 仅由完整 forwarding/package/held-file 检查建立；不是 CLI 能力或发行认证。
pub(crate) struct ProvenNpmEntry {
    entry: PathBuf,
    adjacent_interpreter: PathBuf,
    fallback_interpreter: OsString,
    interpreter_prefix: Vec<OsString>,
    hooks: Vec<OsString>,
}
impl ProvenNpmEntry {
    pub(crate) fn entry(&self) -> &Path {
        &self.entry
    }
    // 语法证明保留“相邻 Node，否则 PATH node”分派；这里不解析或改选 Node。
    #[allow(dead_code)]
    pub(crate) fn adjacent_interpreter(&self) -> &Path {
        &self.adjacent_interpreter
    }
    #[allow(dead_code)]
    pub(crate) fn fallback_interpreter(&self) -> &OsStr {
        &self.fallback_interpreter
    }
    pub(crate) fn interpreter_prefix(&self) -> &[OsString] {
        &self.interpreter_prefix
    }
    pub(crate) fn hook_args(&self) -> &[OsString] {
        &self.hooks
    }
}
impl Transport {
    pub(crate) fn entry(&self) -> &ProvenNpmEntry {
        &self.entry
    }
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

#[cfg(test)]
pub(crate) fn prepare(
    program: &Path,
    args: &[OsString],
    environment: &mut EnvMap,
) -> Result<Transport, SafeError> {
    prepare_with_original(program, args, environment, None)
}

pub(crate) fn prepare_with_original(
    program: &Path,
    args: &[OsString],
    environment: &mut EnvMap,
    original_args: Option<&[OsString]>,
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
    let mut guards = Vec::new();
    // 同时持有用户选择的拼写与实际文件祖先，防止链接/目录替换改变入口。
    hold_parent(program, &mut guards)?;
    let route = super::codex_cmd_route::prove(&held_text(program, &mut guards)?, |name| {
        lookup(environment, OsStr::new(name)).is_some()
    })
    .ok_or_else(|| SafeError {
        field: Some("codexTransportRoute".into()),
        ..unavailable()
    })?;
    let entry = npm.join(&route.entry);
    let manifest: serde_json::Value =
        serde_json::from_str(&held_text(&package.join("package.json"), &mut guards)?)
            .map_err(|_| unavailable())?;
    let bin = manifest["bin"]
        .as_str()
        .or_else(|| manifest["bin"]["codex"].as_str());
    let relative = route
        .entry
        .strip_prefix("node_modules/@openai/codex/")
        .ok_or_else(unavailable)?;
    if manifest["name"] != "@openai/codex" || bin != Some(relative) {
        return Err(unavailable());
    }
    // The chosen run owns its telemetry. Version/entry bytes do not attest a core.
    held_text(&entry, &mut guards)?;
    let (volume, high, low) = identity(guards.last().ok_or_else(unavailable)?)?;
    let args = args
        .iter()
        .map(|arg| arg.to_str().ok_or_else(unavailable))
        .collect::<Result<Vec<_>, _>>()?;
    let canonical_entry = std::fs::canonicalize(&entry).map_err(|_| unavailable())?;
    let payload = serde_json::json!({"entry":canonical_entry.to_str().ok_or_else(unavailable)?, "args":args,
            "fileId": {"volume": volume.to_string(), "index": ((u64::from(high) << 32) | u64::from(low)).to_string()},
            "run": lookup(environment, OsStr::new("CC_DESK_CODEX_RUN")).and_then(|v| v.to_str()),
            "generation": lookup(environment, OsStr::new("CC_DESK_CODEX_GENERATION")).and_then(|v| v.to_str())});
    let rollback = if let Some(original) = original_args {
        if original.len() > 4096 {
            return Err(unavailable());
        }
        let original = original
            .iter()
            .map(|arg| {
                arg.to_str()
                    .filter(|v| !v.contains('\0'))
                    .ok_or_else(unavailable)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut rollback = payload.clone();
        rollback["args"] = serde_json::to_value(original).map_err(|_| unavailable())?;
        rollback
    } else {
        serde_json::Value::Null
    };
    let rollback = serde_json::to_vec(&rollback).map_err(|_| unavailable())?;
    if rollback.len() > 128 * 1024 {
        return Err(unavailable());
    }
    use base64::Engine;
    let mut target = payload.clone();
    target
        .as_object_mut()
        .ok_or_else(unavailable)?
        .remove("args");
    let preload = format!("const target=JSON.parse(Buffer.from('{}','base64').toString('utf8'));\nconst rollback=JSON.parse(Buffer.from('{}','base64').toString('utf8'));\n{}",
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&target).map_err(|_| unavailable())?),
        base64::engine::general_purpose::STANDARD.encode(rollback), include_str!("codex_cmd_preload.cjs"));
    let temp = tempfile::Builder::new()
        .prefix("cc-desk-codex-")
        .tempdir()
        .map_err(|_| unavailable())?;
    let mut transport = Transport {
        _guards: guards,
        _temp: temp,
        entry: ProvenNpmEntry {
            entry,
            adjacent_interpreter: npm.join("node.exe"),
            fallback_interpreter: "node".into(),
            interpreter_prefix: Vec::new(),
            hooks: args.iter().map(OsString::from).collect(),
        },
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
    file.write_all(preload.as_bytes())
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
    fn CodexAdapter_RuntimePayloadLimitRestoresOriginalPolicy_119() {
        // Construct oversized data inside an owned Node fixture: passing it at
        // CreateProcess would test Windows env limits, not this runtime guard.
        for length in [65536usize, 65537, 131072] {
            let (_root, program, package) = npm_fixture("future-protocol-compatible");
            let entry = package.join("bin/codex.js");
            std::fs::write(&entry, r#"
const data = JSON.parse(process.env.CC_DESK_CODEX_ARGV);
const prefix = 'hooks.SessionStart=';
data.args[1] = prefix + 'x'.repeat(Number(process.env.OWN_TEST_LENGTH) - prefix.length);
process.env.CC_DESK_CODEX_ARGV = JSON.stringify(data);
const receivedLength = JSON.parse(process.env.CC_DESK_CODEX_ARGV).args[1].length;
require(process.env.OWN_TEST_LOADER);
process.stdout.write(JSON.stringify({ argv: process.argv.slice(2), receivedLength,
  authority: ['PORT','RUN','GENERATION','CAPABILITY'].map(key => process.env['CC_DESK_CODEX_' + key] ?? null),
  options: process.env.NODE_OPTIONS ?? null, transport: process.env.CC_DESK_CODEX_ARGV ?? null }));
"#).unwrap();
            let original = [
                "resume",
                "exact-thread",
                "-c",
                "hooks.SessionStart=[{hooks=[{type=\"command\",command=\"user guard\"}]}]",
                "--model",
                "中文 空格 & | < > % ! $() ` 引号\"\n\t",
                "duplicate",
                "duplicate",
            ]
            .map(OsString::from);
            let mut environment = EnvMap::new();
            for (key, value) in [
                ("RUN", "owned-run"),
                ("GENERATION", "1"),
                ("PORT", "1234"),
                ("CAPABILITY", "0123456789abcdef0123456789abcdef"),
            ] {
                environment.insert(format!("CC_DESK_CODEX_{key}").into(), value.into());
            }
            let transport =
                prepare_with_original(&program, &fixture_args(), &mut environment, Some(&original))
                    .unwrap();
            let options = environment.remove(OsStr::new("NODE_OPTIONS")).unwrap();
            let loader = PathBuf::from(
                options
                    .to_str()
                    .unwrap()
                    .strip_prefix("--require \"")
                    .unwrap()
                    .strip_suffix('"')
                    .unwrap(),
            );
            environment.insert("OWN_TEST_LOADER".into(), loader.as_os_str().to_owned());
            environment.insert("OWN_TEST_LENGTH".into(), length.to_string().into());
            if let Some(system_root) = std::env::var_os("SystemRoot") {
                environment.insert("SystemRoot".into(), system_root);
            }
            let output = std::process::Command::new("node")
                .env_clear()
                .envs(&environment)
                .arg(&entry)
                .args(["resume", "exact-thread", "remaining-business"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "owned runtime fixture must finish: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let observed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                observed["receivedLength"], length,
                "runtime input must arrive intact, not be truncated/rejected by OS env assignment"
            );
            assert!(observed["options"].is_null());
            assert!(observed["transport"].is_null());
            if length == 65536 {
                assert_eq!(observed["argv"].as_array().unwrap().len(), 11);
                assert_eq!(observed["argv"][0], "-c");
                assert_eq!(observed["argv"][1].as_str().unwrap().len(), 65536);
                assert_eq!(observed["argv"][8], "resume");
                assert_eq!(observed["argv"][10], "remaining-business");
                assert_eq!(observed["authority"][3], "0123456789abcdef0123456789abcdef");
            } else {
                let expected: Vec<_> = original
                    .iter()
                    .map(|value| value.to_str().unwrap())
                    .collect();
                assert_eq!(observed["argv"], serde_json::json!(expected));
                assert_eq!(
                    observed["authority"],
                    serde_json::json!([null, null, null, null])
                );
            }
            assert!(loader.is_file());
            drop(transport);
            assert!(
                !loader.exists(),
                "private rollback artifact must retire with its transport"
            );
        }
    }

    #[test]
    fn CodexAdapter_VersionIndependentTransport_101() {
        for version in ["0.1.0", "0.166.0", "99.0.0-next", "", "not-semver"] {
            let (_root, program, _package) = npm_fixture(version);
            let mut environment = EnvMap::new();
            let transport = prepare(&program, &fixture_args(), &mut environment);
            assert!(
                transport.is_ok(),
                "the same forwarding protocol must not depend on {version:?}"
            );
            assert!(lookup(&environment, OsStr::new("CC_DESK_CODEX_ARGV")).is_some());
        }
    }

    #[test]
    fn CodexAdapter_ForwardingShapeSurvivesDistributionBytes_102() {
        let (_root, program, package) = npm_fixture("0.164.0");
        let shim = std::fs::read_to_string(&program).unwrap();
        std::fs::write(&program, format!("@REM npm generator annotation\n{shim}")).unwrap();
        // Selected entry contents belong to the selected run; no release-byte admission.
        std::fs::write(
            package.join("bin/codex.js"),
            "// a differently packaged selected entry\n",
        )
        .unwrap();
        assert!(prepare(&program, &fixture_args(), &mut EnvMap::new()).is_ok());
    }

    #[test]
    fn CodexAdapter_TransportRejectsUnprovenEntryMode_103() {
        let (_root, program, package) = npm_fixture("0.164.0");
        let mut environment = EnvMap::new();
        let _transport = prepare(&program, &fixture_args(), &mut environment).unwrap();
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
        environment.insert(
            "CC_DESK_CODEX_CAPABILITY".into(),
            "owned-test-capability".into(),
        );
        let output = std::process::Command::new("node")
            .env_clear().envs(&environment)
            .args(["-e", "process.stdout.write(JSON.stringify({argv:process.argv.slice(2),capability:process.env.CC_DESK_CODEX_CAPABILITY??null,transport:process.env.CC_DESK_CODEX_ARGV??null}))"])
            .arg(package.join("bin/codex.js")).arg("resume").arg("exact-thread")
            .output().unwrap();
        assert!(output.status.success());
        let observed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            observed["argv"],
            serde_json::json!(["resume", "exact-thread"])
        );
        assert!(observed["capability"].is_null());
        assert!(observed["transport"].is_null());
    }

    #[test]
    fn CodexAdapter_RouteRejectsAlteredSetAndEnvironmentCapture_105() {
        for replacement in [
            "SET \"_prog=node \"",
            "SET _prog=node ",
            "SET _prog=node\t",
            "SET\u{a0}\"_prog=node\"",
        ] {
            let (_root, program, _package) = npm_fixture("0.164.0");
            let original = std::fs::read_to_string(&program).unwrap();
            std::fs::write(
                &program,
                original.replace("SET \"_prog=node\"", replacement),
            )
            .unwrap();
            assert!(
                prepare(&program, &fixture_args(), &mut EnvMap::new()).is_err(),
                "changed SET semantics: {replacement:?}"
            );
        }
        let (_root, program, _package) = npm_fixture("0.164.0");
        let original = std::fs::read_to_string(&program).unwrap();
        std::fs::write(&program, original.replace("_prog", "selected_node")).unwrap();
        let mut environment =
            EnvMap::from([("selected_node".into(), "user-owned-environment".into())]);
        assert!(prepare(&program, &fixture_args(), &mut environment).is_err());
    }

    #[test]
    fn CodexAdapter_MissingRunBindingNeverInjects_106() {
        let (_root, program, package) = npm_fixture("0.164.0");
        std::fs::write(
            package.join("bin/codex.js"),
            "process.stdout.write(JSON.stringify(process.argv.slice(2)))",
        )
        .unwrap();
        let mut environment = EnvMap::new();
        let _transport = prepare(&program, &fixture_args(), &mut environment).unwrap();
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
        let output = std::process::Command::new("node")
            .env_clear()
            .envs(&environment)
            .arg(package.join("bin/codex.js"))
            .arg("resume")
            .arg("exact-thread")
            .output()
            .unwrap();
        assert!(output.status.success());
        let observed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(observed, serde_json::json!(["resume", "exact-thread"]));
    }

    #[test]
    fn CodexAdapter_EnvelopeCannotMoveToAnotherRun_117() {
        let (_root, program, package) = npm_fixture("0.164.0");
        std::fs::write(package.join("bin/codex.js"), "process.stdout.write(JSON.stringify({argv:process.argv.slice(2),capability:process.env.CC_DESK_CODEX_CAPABILITY??null}))").unwrap();
        let mut environment = EnvMap::new();
        for (key, value) in [
            ("RUN", "owned-run"),
            ("GENERATION", "1"),
            ("PORT", "1234"),
            ("CAPABILITY", "0123456789abcdef0123456789abcdef"),
        ] {
            environment.insert(format!("CC_DESK_CODEX_{key}").into(), value.into());
        }
        let _transport = prepare(&program, &fixture_args(), &mut environment).unwrap();
        let mut payload: serde_json::Value = serde_json::from_str(
            environment[OsStr::new("CC_DESK_CODEX_ARGV")]
                .to_str()
                .unwrap(),
        )
        .unwrap();
        payload["run"] = "other-run".into();
        payload["generation"] = "2".into();
        environment.insert("CC_DESK_CODEX_ARGV".into(), payload.to_string().into());
        environment.insert("CC_DESK_CODEX_RUN".into(), "other-run".into());
        environment.insert("CC_DESK_CODEX_GENERATION".into(), "2".into());
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
        let output = std::process::Command::new("node")
            .env_clear()
            .envs(&environment)
            .arg(package.join("bin/codex.js"))
            .arg("resume")
            .arg("exact-thread")
            .output()
            .unwrap();
        assert!(output.status.success());
        let observed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            observed["argv"],
            serde_json::json!(["resume", "exact-thread"])
        );
        assert!(observed["capability"].is_null());
    }

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
        std::fs::write(package.join("bin/codex.js"), "process.stdout.write(JSON.stringify({argv:process.argv.slice(2),options:process.env.NODE_OPTIONS??null,transport:process.env.CC_DESK_CODEX_ARGV??null}))").unwrap();
        for (key, value) in [
            ("RUN", "owned-run"),
            ("GENERATION", "1"),
            ("PORT", "1234"),
            ("CAPABILITY", "0123456789abcdef0123456789abcdef"),
        ] {
            environment.insert(format!("CC_DESK_CODEX_{key}").into(), value.into());
        }
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
        // Future versions receive the same plan; only actual authenticated events prove delivery.
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@openai/codex","version":"0.166.0","bin":{"codex":"bin/codex.js"}}"#,
        )
        .unwrap();
        assert!(prepare(&program, &args, &mut EnvMap::new()).is_ok());
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
    fn CodexIdentity_NpmUnprovenRouteHasFixedReason_085() {
        let (_root, program, _package) = npm_fixture("0.166.0");
        std::fs::write(&program, "@echo custom").unwrap();
        let failure = prepare(&program, &fixture_args(), &mut EnvMap::new())
            .err()
            .expect("unproven forwarding must fail closed");
        assert_eq!(failure.code, "OBSERVER_PREPARE_FAILED");
        assert_eq!(failure.field.as_deref(), Some("codexTransportRoute"));
    }

    #[test]
    fn CodexIdentity_NpmKnownVersionRejectsTampering_084() {
        {
            let (_root, program, _package) = npm_fixture("0.164.0");
            drop(
                prepare(&program, &fixture_args(), &mut EnvMap::new())
                    .expect("known-version control must pass before tamper"),
            );
            let path = program.clone();
            let mut bytes = std::fs::read(&path).unwrap();
            bytes.extend_from_slice(b"\nchanged bytes\n");
            std::fs::write(path, bytes).unwrap();
            let mut env = EnvMap::new();
            let failure = prepare(&program, &fixture_args(), &mut env)
                .err()
                .expect("unknown statements must not be ignored by the forwarding proof");
            assert_eq!(failure.code, "OBSERVER_PREPARE_FAILED");
            assert_eq!(failure.field.as_deref(), Some("codexTransportRoute"));
            assert!(env.is_empty());
        }
        for (field, value) in [
            ("name", serde_json::json!("@other/codex")),
            ("bin", serde_json::json!({"codex":"bin/other.js"})),
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
