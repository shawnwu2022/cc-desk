//! 默认 Windows 原生包只声明所选入口的参数归属，不认证发行者或新增权限。
use super::{codex_cmd, profiles::error, types::SafeError};
use serde::Deserialize;
use std::{fs::File, path::Path};

pub(crate) struct NativePackageEntry {
    _guards: Vec<File>,
}

#[derive(Deserialize)]
struct Manifest {
    variant: String,
    entrypoint: String,
    // 未消费字段仍经有界 JSON Value 解析，保留深度限制而不拒绝协议扩展。
    #[serde(flatten)]
    _extra: std::collections::BTreeMap<String, serde_json::Value>,
}

pub(crate) fn prepare(program: &Path) -> Result<Option<NativePackageEntry>, SafeError> {
    // 精确布局与元数据共同声明入口；不能只按可执行文件名称推断 parser。
    if program.file_name().is_none_or(|v| v != "codex.exe")
        || program
            .parent()
            .and_then(Path::file_name)
            .is_none_or(|v| v != "bin")
    {
        return Ok(None);
    }
    let metadata = program
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| error("OBSERVER_PREPARE_FAILED"))?
        .join("codex-package.json");
    match std::fs::symlink_metadata(&metadata) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(error("OBSERVER_PREPARE_FAILED")),
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            return Err(error("OBSERVER_PREPARE_FAILED"))
        }
        Ok(_) => {}
    }
    // 复用 held file/parent、最终路径和 file ID 前后复核；不改变选中拼写。
    let mut guards = codex_cmd::hold_selected_target(program)?;
    let text = codex_cmd::held_text(&metadata, &mut guards)?;
    let manifest: Manifest =
        serde_json::from_str(&text).map_err(|_| error("OBSERVER_PREPARE_FAILED"))?;
    if manifest.variant != "codex" || manifest.entrypoint != "bin/codex.exe" {
        return Err(error("OBSERVER_PREPARE_FAILED"));
    }
    // entrypoint 是固定相对形状，恰为已持有所选文件；不解析任意路径或命令。
    Ok(Some(NativePackageEntry { _guards: guards }))
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn NativePackage_MalformedMetadataCannotAuthorizeEntry_130() {
        for text in [
            "{}".to_string(),
            r#"{"variant":"codex","variant":"codex","entrypoint":"bin/codex.exe"}"#.into(),
            r#"{"variant":"codex","entrypoint":"bin/codex.exe","entrypoint":"bin/codex.exe"}"#
                .into(),
            r#"{"variant":"codex","entrypoint":5}"#.into(),
            "[".repeat(200),
            " ".repeat(128 * 1024 + 1),
            format!(
                "{{\"variant\":\"codex\",\"entrypoint\":\"bin/codex.exe\",\"extra\":{}0{}}}",
                "[".repeat(200),
                "]".repeat(200)
            ),
        ]
        .into_iter()
        .chain(
            [
                "../codex.exe",
                "bin/../bin/codex.exe",
                "bin/python.exe",
                "C:/bin/codex.exe",
                "//host/x",
                "bin/codex.exe:stream",
                "bin/codex.exe\0",
            ]
            .map(|entrypoint| {
                serde_json::json!({"variant":"codex","entrypoint":entrypoint}).to_string()
            }),
        )
        .chain([
            serde_json::json!({"variant":"codex-app-server","entrypoint":"bin/codex.exe"})
                .to_string(),
        ]) {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("bin")).unwrap();
            let program = root.path().join("bin/codex.exe");
            std::fs::write(&program, "not executed").unwrap();
            std::fs::write(root.path().join("codex-package.json"), text).unwrap();
            assert!(prepare(&program).is_err());
            // 失败释放守卫，不遗留对自有 fixture 的写锁。
            std::fs::write(program, "released").unwrap();
        }
    }

    #[test]
    fn NativePackage_AbsentOrInterpreterLayoutStaysOpaque_131() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("bin")).unwrap();
        for name in ["codex.exe", "python.exe", "pwsh.exe", "node.exe"] {
            let program = root.path().join("bin").join(name);
            std::fs::write(&program, "not executed").unwrap();
            assert!(prepare(&program).unwrap().is_none());
        }
        std::fs::write(
            root.path().join("codex-package.json"),
            r#"{"variant":"codex","entrypoint":"bin/codex.exe"}"#,
        )
        .unwrap();
        for name in ["python.exe", "pwsh.exe", "node.exe"] {
            assert!(prepare(&root.path().join("bin").join(name))
                .unwrap()
                .is_none());
        }
        let program = root.path().join("bin/codex.exe");
        assert!(prepare(&root.path().join("bin/../bin/codex.exe")).is_err());
        let held = prepare(&program).unwrap().unwrap();
        assert!(std::fs::rename(&program, root.path().join("moved.exe")).is_err());
        assert!(std::fs::rename(root.path().join("bin"), root.path().join("moved")).is_err());
        assert!(std::fs::rename(
            root.path().join("codex-package.json"),
            root.path().join("moved.json")
        )
        .is_err());
        drop(held);
        std::fs::rename(program, root.path().join("moved.exe")).unwrap();
    }
}
