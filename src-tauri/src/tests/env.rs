use std::collections::HashMap;

// ==================== claude_env_vars 配置数据 ====================

// claude_env_vars 序列化/反序列化正确
#[test]
fn EnvVars_ConfigRoundTrip_001() {
    let mut env_vars = HashMap::new();
    env_vars.insert("LANG".to_string(), "en_US.UTF-8".to_string());
    env_vars.insert("CLAUDE_CODE_NO_FLICKER".to_string(), "1".to_string());

    let config = crate::store::AppConfig {
        claude_env_vars: Some(env_vars.clone()),
        ..Default::default()
    };

    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json["claudeEnvVars"]["LANG"].as_str(), Some("en_US.UTF-8"));
    assert_eq!(
        json["claudeEnvVars"]["CLAUDE_CODE_NO_FLICKER"].as_str(),
        Some("1")
    );

    let deserialized: crate::store::AppConfig = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.claude_env_vars, Some(env_vars));
}

// 空 AppConfig 的 claude_env_vars 为 None
#[test]
fn EnvVars_EmptyConfig_001() {
    let config = crate::store::AppConfig::default();
    assert!(config.claude_env_vars.is_none());
}

// 含特殊字符的值能正确 round-trip
#[test]
fn EnvVars_SpecialChars_001() {
    let mut env_vars = HashMap::new();
    env_vars.insert(
        "PATH_EXTRA".to_string(),
        "/usr/bin:/home/user/bin".to_string(),
    );
    env_vars.insert("VAR_WITH_SPACES".to_string(), "hello world".to_string());

    let config = crate::store::AppConfig {
        claude_env_vars: Some(env_vars.clone()),
        ..Default::default()
    };

    let json = serde_json::to_value(&config).unwrap();
    let deserialized: crate::store::AppConfig = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.claude_env_vars.unwrap(), env_vars);
}

// merge_json_values 对 claudeEnvVars 的更新行为
#[test]
fn EnvVars_MergeConfig_001() {
    let base = serde_json::json!({
        "theme": "dark",
        "claudeEnvVars": {
            "LANG": "en_US.UTF-8"
        }
    });
    let updates = serde_json::json!({
        "claudeEnvVars": {
            "LANG": "zh_CN.UTF-8",
            "NEW_VAR": "new_value"
        }
    });

    let merged = crate::store::merge_json_values(base, updates);
    let env = merged["claudeEnvVars"].as_object().unwrap();
    assert_eq!(env["LANG"].as_str(), Some("zh_CN.UTF-8"));
    assert_eq!(env["NEW_VAR"].as_str(), Some("new_value"));
}

// ==================== language 配置 ====================

// language 字段序列化/反序列化 round-trip
#[test]
fn Language_RoundTrip_001() {
    let config = crate::store::AppConfig {
        language: Some("zh".to_string()),
        ..Default::default()
    };

    let json = serde_json::to_value(&config).unwrap();
    assert_eq!(json["language"].as_str(), Some("zh"));

    let deserialized: crate::store::AppConfig = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.language, Some("zh".to_string()));
}

// 默认 AppConfig 的 language 为 None
#[test]
fn Language_DefaultNone_001() {
    let config = crate::store::AppConfig::default();
    assert!(config.language.is_none());
}

// JSON 中缺少 language 字段时反序列化为 None
#[test]
fn Language_MissingField_001() {
    let json = serde_json::json!({
        "theme": "dark"
    });
    let config: crate::store::AppConfig = serde_json::from_value(json).unwrap();
    assert!(config.language.is_none());
}
