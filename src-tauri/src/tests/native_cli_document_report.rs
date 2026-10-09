use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Evidence {
    pub(super) schema: u32,
    pub(super) mode: String,
    pub(super) target: String,
    pub(super) engine: String,
    pub(super) engine_version: String,
    pub(super) events: Vec<String>,
    pub(super) observations: Vec<(String, String)>,
    pub(super) failure: Option<String>,
}

pub(super) fn expected(mode: &str) -> Vec<(&str, &str)> {
    vec![
        ("start", "ADMITTED"),
        ("query", "LAUNCH_NOT_FOUND"),
        ("missing-proof", "FORBIDDEN"),
        ("wrong-proof", "FORBIDDEN"),
        ("combined-proof", "FORBIDDEN"),
        ("forged-owner", "INVALID_REQUEST"),
        ("json", "RAW_BODY_REQUIRED"),
        ("query-boundary", "LAUNCH_NOT_FOUND"),
        ("query-overflow", "REQUEST_TOO_LARGE"),
        ("peer", "FORBIDDEN"),
        (mode, "FORBIDDEN"),
    ]
}

pub(super) fn verify(report: &Evidence, mode: &str) -> Result<(), &'static str> {
    if !matches!(mode, "reload" | "destroy")
        || report.schema != 1
        || report.mode != mode
        || report.target != "windows-x86_64"
        || report.engine != "tauri-wry-webview2"
        || report.engine_version.is_empty()
        || report.engine_version.len() > 128
        || report.engine_version.trim() != report.engine_version
    {
        return Err("EVIDENCE_IDENTITY");
    }
    if report.failure.is_some() {
        return Err("EVIDENCE_FAILED");
    }
    if !(2..=128).contains(&report.events.len())
        || report.events[0] != "started"
        || report.events[1] != "finished"
        || report
            .events
            .iter()
            .any(|event| !matches!(event.as_str(), "started" | "finished"))
    {
        return Err("EVIDENCE_PAGE_EVENTS");
    }
    let expected = expected(mode);
    if report.observations.len() != expected.len()
        || report
            .observations
            .iter()
            .zip(expected)
            .any(|((name, result), (required, outcome))| name != required || result != outcome)
    {
        return Err("EVIDENCE_CASES");
    }
    Ok(())
}

fn complete(mode: &str) -> Evidence {
    Evidence {
        schema: 1,
        mode: mode.into(),
        target: "windows-x86_64".into(),
        engine: "tauri-wry-webview2".into(),
        engine_version: "152.0.0.1".into(),
        events: vec!["started".into(), "finished".into()],
        observations: expected(mode)
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect(),
        failure: None,
    }
}

// 检查缺少任一原生断言的报告均不能放行。
#[test]
fn D11_Evidence_MissingCase_001() {
    for mode in ["reload", "destroy"] {
        let good = complete(mode);
        for index in 0..good.observations.len() {
            let mut bad = good.clone();
            bad.observations.remove(index);
            assert!(verify(&bad, mode).is_err(), "missing case {index}");
        }
    }
}

// 检查错平台、错模式、缺引擎版本及缺加载事件均被拒绝。
#[test]
fn D11_Evidence_WrongIdentity_002() {
    let good = complete("reload");
    for field in ["schema", "mode", "target", "engine", "version", "events"] {
        let mut bad = good.clone();
        match field {
            "schema" => bad.schema = 2,
            "mode" => bad.mode = "destroy".into(),
            "target" => bad.target = "mock".into(),
            "engine" => bad.engine = "mock".into(),
            "version" => bad.engine_version.clear(),
            "events" => bad.events = vec!["finished".into(), "started".into()],
            _ => unreachable!(),
        }
        assert!(verify(&bad, "reload").is_err(), "invalid field {field}");
    }
}

// 检查重复结果、错误结果和粘性失败标记不能被其他成功覆盖。
#[test]
fn D11_Evidence_FalseSuccess_003() {
    let mut duplicate = complete("reload");
    duplicate.observations[1] = duplicate.observations[0].clone();
    assert!(verify(&duplicate, "reload").is_err());
    let mut wrong = complete("reload");
    wrong.observations[0].1 = "FORBIDDEN".into();
    assert!(verify(&wrong, "reload").is_err());
    let mut failed = complete("reload");
    failed.failure = Some("SCRIPT_FAILURE".into());
    assert!(verify(&failed, "reload").is_err());
}

// 检查两种完整报告都被接受，且不接受未定义的测试模式。
#[test]
fn D11_Evidence_Complete_004() {
    for mode in ["reload", "destroy"] {
        assert!(verify(&complete(mode), mode).is_ok());
    }
    assert!(verify(&complete("other"), "other").is_err());
}
