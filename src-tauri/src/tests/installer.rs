use crate::installer::{extract_semver, is_newer_version, parse_version_output};

// 版本号 "1.0.33" 直接提取
#[test]
fn ParseVersion_Simple_001() {
    let result = parse_version_output("1.0.33");
    assert_eq!(result, Some("1.0.33".to_string()));
}

// "claude 1.0.33" 格式提取版本号
#[test]
fn ParseVersion_WithPrefix_001() {
    let result = parse_version_output("claude 1.0.33");
    assert_eq!(result, Some("1.0.33".to_string()));
}

// "@anthropic-ai/claude-code 1.0.33" 格式提取版本号
#[test]
fn ParseVersion_NpmStyle_001() {
    let result = parse_version_output("@anthropic-ai/claude-code 1.0.33");
    assert_eq!(result, Some("1.0.33".to_string()));
}

// 空字符串返回 None
#[test]
fn ParseVersion_Empty_001() {
    let result = parse_version_output("");
    assert_eq!(result, None);
}

// 无版本号的字符串返回 None
#[test]
fn ParseVersion_NoVersion_001() {
    let result = parse_version_output("no version here");
    assert_eq!(result, None);
}

// 四段版本号提取前三段
#[test]
fn ParseVersion_FourPart_001() {
    let result = parse_version_output("1.0.33.1");
    assert_eq!(result, Some("1.0.33".to_string()));
}

// extract_semver 从纯版本号提取
#[test]
fn ExtractSemver_Pure_001() {
    let result = extract_semver("1.2.3");
    assert_eq!(result, Some("1.2.3".to_string()));
}

// extract_semver 嵌入在字符串中
#[test]
fn ExtractSemver_Embedded_001() {
    let result = extract_semver("v1.2.3-beta");
    // "v" 不匹配数字开头，但 "1.2.3" 可以
    assert_eq!(result, Some("1.2.3".to_string()));
}

// extract_semver 无版本号
#[test]
fn ExtractSemver_None_001() {
    let result = extract_semver("hello");
    assert_eq!(result, None);
}

// extract_semver 只有两段数字不算版本号
#[test]
fn ExtractSemver_TwoParts_001() {
    let result = extract_semver("1.2");
    assert_eq!(result, None);
}

// is_newer_version: 1.0.33 > 1.0.30
#[test]
fn IsNewer_Patch_001() {
    assert!(is_newer_version("1.0.33", "1.0.30"));
}

// is_newer_version: 1.1.0 > 1.0.33
#[test]
fn IsNewer_Minor_001() {
    assert!(is_newer_version("1.1.0", "1.0.33"));
}

// is_newer_version: 2.0.0 > 1.9.9
#[test]
fn IsNewer_Major_001() {
    assert!(is_newer_version("2.0.0", "1.9.9"));
}

// is_newer_version: 相同版本返回 false
#[test]
fn IsNewer_Same_001() {
    assert!(!is_newer_version("1.0.33", "1.0.33"));
}

// is_newer_version: 旧版本不比新版本新
#[test]
fn IsNewer_Older_001() {
    assert!(!is_newer_version("1.0.30", "1.0.33"));
}

// is_newer_version: 带 v 前缀
#[test]
fn IsNewer_VPrefix_001() {
    assert!(is_newer_version("v1.0.33", "v1.0.30"));
}

// is_newer_version: 段数不同
#[test]
fn IsNewer_DifferentLength_001() {
    assert!(is_newer_version("1.0.33", "1.0"));
}
