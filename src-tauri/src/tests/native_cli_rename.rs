#![allow(non_snake_case)]
use super::*;
use serde_json::json;

#[test]
fn NativeRename_OfficialProtocolConfirmsWithoutTurn_001() {
    let mut sent = Vec::new();
    let mut replies = std::collections::VecDeque::from([
        json!({"id":1,"result":{}}),
        json!({"id":2,"result":{"thread":{"id":"fixture","name":null}}}),
        json!({"id":3,"result":{}}),
        json!({"id":4,"result":{"thread":{"id":"fixture","name":"New title"}}}),
    ]);
    exchange(
        "fixture",
        "New title",
        &mut |message| {
            sent.push(message);
            Ok(())
        },
        &mut |id| {
            let result = replies.pop_front().unwrap();
            assert_eq!(result["id"], id);
            Ok(result)
        },
        &|thread| {
            assert_eq!(thread["id"], "fixture");
            Ok(())
        },
    )
    .unwrap();
    let methods: Vec<_> = sent
        .iter()
        .map(|message| message["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "initialize",
            "initialized",
            "thread/read",
            "thread/name/set",
            "thread/read"
        ]
    );
    assert_eq!(
        sent[3]["params"],
        json!({"threadId":"fixture","name":"New title"})
    );
}

#[test]
fn NativeRename_ReadbackMismatchNeverConfirms_002() {
    let mut replies = std::collections::VecDeque::from([
        json!({"id":1,"result":{}}),
        json!({"id":2,"result":{"thread":{"id":"fixture"}}}),
        json!({"id":3,"result":{}}),
        json!({"id":4,"result":{"thread":{"id":"fixture","name":"old"}}}),
    ]);
    assert!(exchange(
        "fixture",
        "new",
        &mut |_| Ok(()),
        &mut |_| Ok(replies.pop_front().unwrap()),
        &|_| Ok(())
    )
    .is_err());
}

#[test]
fn NativeRename_RevokedBeforeWriteNeverSendsMutation_003() {
    let mut sent = Vec::new();
    let mut replies = std::collections::VecDeque::from([
        json!({"id":1,"result":{}}),
        json!({"id":2,"result":{"thread":{}}}),
    ]);
    let result = exchange(
        "fixture",
        "new",
        &mut |message| {
            sent.push(message);
            Ok(())
        },
        &mut |_| Ok(replies.pop_front().unwrap()),
        &|_| Err(super::super::profiles::error("SCOPE_REVOKED")),
    );
    assert_eq!(result.unwrap_err().code, "SCOPE_REVOKED");
    assert!(!sent
        .iter()
        .any(|message| message["method"] == "thread/name/set"));
}

#[test]
fn NativeRename_WriteReplyLostRemainsUnknown_004() {
    let mut replies = std::collections::VecDeque::from([
        json!({"id":1,"result":{}}),
        json!({"id":2,"result":{"thread":{}}}),
    ]);
    let result = exchange(
        "fixture",
        "new",
        &mut |_| Ok(()),
        &mut |_| {
            replies
                .pop_front()
                .ok_or_else(|| super::super::profiles::error("RENAME_FAILED"))
        },
        &|_| Ok(()),
    );
    assert_eq!(result.unwrap_err().code, "RENAME_OUTCOME_UNKNOWN");
}

#[test]
fn NativeRename_UnknownMethodCannotConfirm_005() {
    let result = exchange(
        "fixture",
        "new",
        &mut |_| Ok(()),
        &mut |id| Ok(json!({"id":id,"error":{"code":-32601,"message":"private path"}})),
        &|_| Ok(()),
    );
    let failure = result.unwrap_err();
    assert_eq!(failure.code, "RENAME_FAILED");
    assert!(!serde_json::to_string(&failure)
        .unwrap()
        .contains("private path"));
}

#[test]
fn NativeRename_PrivateLinkPinsInodeAndCleansOnlyNamespace_012() {
    use crate::cli::native_projection::scoped_fs::Root;
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("projects/p")).unwrap();
    let id = "00000000-0000-4000-8000-000000000012";
    let relative = format!("projects/p/{id}.jsonl");
    let source = temp.path().join(&relative);
    let original = format!(
        "{}\n",
        json!({"type":"user","sessionId":id,"cwd":"/fixture","message":{"content":"fixture"}})
    );
    std::fs::write(&source, &original).unwrap();
    let root = Root::open(temp.path()).unwrap();
    let pinned = root.pin_transcript(&relative).unwrap();
    let private = root.private_transcript(&pinned, id).unwrap();
    let private_path = private.path.clone();
    let linked = private.path.join(format!("projects/authorized/{id}.jsonl"));
    std::fs::rename(&source, temp.path().join("original-renamed.jsonl")).unwrap();
    std::fs::write(&source, "replacement bytes").unwrap();
    let mut writer = std::fs::OpenOptions::new()
        .append(true)
        .open(&linked)
        .unwrap();
    writer.write_all(b"authorized inode\n").unwrap();
    drop(writer);
    assert_eq!(
        std::fs::read_to_string(&source).unwrap(),
        "replacement bytes"
    );
    assert_eq!(
        std::fs::read_to_string(temp.path().join("original-renamed.jsonl")).unwrap(),
        format!("{original}authorized inode\n")
    );
    assert!(pinned.current(&root).is_err());
    drop(private);
    assert!(!private_path.exists());
    assert!(source.exists());
}

#[test]
fn NativeRename_CompressedSourceAndMissingFrozenNodeFailClosed_013() {
    use crate::cli::native_projection::scoped_fs::Root;
    let temp = tempfile::tempdir().unwrap();
    let root = Root::open(temp.path()).unwrap();
    assert_eq!(
        root.pin_transcript("session.jsonl.zst").err().unwrap(),
        "NATIVE_RENAME_UNSUPPORTED"
    );
    let environment = crate::cli::environment::EnvMap::new();
    assert_eq!(
        claude::node_program(&environment).unwrap_err().code,
        "NODE_RUNTIME_UNAVAILABLE"
    );
}

#[test]
fn NativeRename_ClaudeHelperUsesSamePhysicalNamespaceThroughAlias_015() {
    use std::io::Write;
    let temp = tempfile::tempdir().unwrap();
    let physical = temp.path().join("physical");
    let alias = temp.path().join("alias");
    std::fs::create_dir_all(physical.join("projects/authorized")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&physical, &alias).unwrap();
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd.exe");
        crate::platform::configure_command(&mut command);
        assert!(command
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&alias)
            .arg(&physical)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success());
    }
    let id = "00000000-0000-4000-8000-000000000015";
    let transcript = physical.join(format!("projects/authorized/{id}.jsonl"));
    std::fs::write(&transcript, format!("{}\n", json!({"type":"user","sessionId":id,"cwd":"/fixture","message":{"content":"private fixture"}}))).unwrap();
    std::fs::write(physical.join("bridge.cjs"), claude::embedded_bridge()).unwrap();
    let environment = std::env::vars_os().collect();
    let mut child = claude::command(&environment, &alias.join("bridge.cjs"), &alias)
        .unwrap()
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(json!({"nativeSessionId":id,"expectedCwd":"/fixture","title":"Physical namespace title"}).to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "private SDK helper must load and rename through the alias"
    );
    let responses: Vec<serde_json::Value> = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["state"], "ready");
    assert_eq!(responses[1]["state"], "renamed");
    assert_eq!(responses[1]["title"], "Physical namespace title");
    let rows: Vec<serde_json::Value> = std::fs::read_to_string(transcript)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["message"]["content"], "private fixture");
    assert_eq!(rows[1]["customTitle"], "Physical namespace title");
    #[cfg(unix)]
    std::fs::remove_file(alias).unwrap();
    #[cfg(windows)]
    std::fs::remove_dir(alias).unwrap();
}

#[test]
fn NativeRename_ClaudeHelperRejectsBridgeOutsideNamespace_016() {
    let temp = tempfile::tempdir().unwrap();
    let namespace = temp.path().join("namespace");
    std::fs::create_dir(&namespace).unwrap();
    let bridge = temp.path().join("bridge.cjs");
    std::fs::write(&bridge, b"module.exports=1;").unwrap();
    let environment = std::env::vars_os().collect();
    assert_eq!(
        claude::command(&environment, &bridge, &namespace)
            .unwrap_err()
            .code,
        "NATIVE_RENAME_UNSUPPORTED"
    );
}
