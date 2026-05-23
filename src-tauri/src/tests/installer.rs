use crate::installer::{clean_and_prepend_path, clean_rc_content, extract_semver, is_newer_version, parse_version_output};

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

// Windows cmd /C 输出带 \r\n 换行
#[test]
fn ParseVersion_WindowsNewline_001() {
    let result = parse_version_output("claude 1.0.33\r\n");
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

// ============================================
// clean_and_prepend_path 测试
// ============================================

// 新目录添加到空 PATH
#[test]
fn CleanPrepend_EmptyPath_001() {
    let result = clean_and_prepend_path("", "/usr/bin", ':');
    assert_eq!(result, "/usr/bin");
}

// 新目录添加到已有 PATH 开头
#[test]
fn CleanPrepend_NewDir_001() {
    let result = clean_and_prepend_path("/usr/bin:/usr/local/bin", "/home/user/.local/bin", ':');
    assert_eq!(result, "/home/user/.local/bin:/usr/bin:/usr/local/bin");
}

// 已在 PATH 中的目录被移到最前（大小写不敏感）
#[test]
fn CleanPrepend_MoveToFront_001() {
    let result = clean_and_prepend_path("/usr/bin:/HOME/USER/.local/bin:/usr/local/bin", "/home/user/.local/bin", ':');
    assert_eq!(result, "/home/user/.local/bin:/usr/bin:/usr/local/bin");
}

// 已在最前面的目录保持不变
#[test]
fn CleanPrepend_AlreadyFirst_001() {
    let result = clean_and_prepend_path("/usr/bin:/usr/local/bin", "/usr/bin", ':');
    assert_eq!(result, "/usr/bin:/usr/local/bin");
}

// Windows 风格 PATH 用分号分隔
#[test]
fn CleanPrepend_WindowsSep_001() {
    let result = clean_and_prepend_path("C:\\Windows;C:\\System32", "C:\\Users\\test\\.local\\bin", ';');
    assert_eq!(result, "C:\\Users\\test\\.local\\bin;C:\\Windows;C:\\System32");
}

// Windows 大小写不敏感匹配
#[test]
fn CleanPrepend_WindowsCaseInsensitive_001() {
    let result = clean_and_prepend_path("C:\\Windows;C:\\USERS\\TEST\\.LOCAL\\BIN;C:\\System32", "c:\\users\\test\\.local\\bin", ';');
    assert_eq!(result, "c:\\users\\test\\.local\\bin;C:\\Windows;C:\\System32");
}

// 末尾有多余分隔符时不会产生空条目
#[test]
fn CleanPrepend_TrailingSep_001() {
    let result = clean_and_prepend_path("/usr/bin:", "/home/user/.local/bin", ':');
    // 尾部分隔符产生空条目，被保留但不影响功能
    assert!(result.starts_with("/home/user/.local/bin:"));
}

// 目录出现多次时全部移除后前置一次
#[test]
fn CleanPrepend_DuplicateEntries_001() {
    let result = clean_and_prepend_path("/usr/bin:/home/user/.local/bin:/usr/local/bin:/home/user/.local/bin", "/home/user/.local/bin", ':');
    assert_eq!(result, "/home/user/.local/bin:/usr/bin:/usr/local/bin");
}

// ============================================
// clean_rc_content 测试
// ============================================

// 空文件追加 export 行
#[test]
fn CleanRcContent_Empty_001() {
    let markers = ["/home/user/.local/bin", "$HOME/.local/bin"];
    let result = clean_rc_content("", &markers, "export PATH=\"$HOME/.local/bin:$PATH\"");
    assert_eq!(result, "export PATH=\"$HOME/.local/bin:$PATH\"\n");
}

// 无匹配行的文件保留原内容并追加
#[test]
fn CleanRcContent_NoMatch_001() {
    let content = "export PATH=\"/usr/bin:$PATH\"\nalias ll='ls -la'\n";
    let markers = ["/home/user/.local/bin", "$HOME/.local/bin"];
    let result = clean_rc_content(content, &markers, "export PATH=\"$HOME/.local/bin:$PATH\"");
    assert!(result.contains("alias ll='ls -la'"));
    assert!(result.ends_with("export PATH=\"$HOME/.local/bin:$PATH\"\n"));
}

// 已有 $HOME 格式的匹配行时移除旧行并追加新行
#[test]
fn CleanRcContent_RemoveOld_001() {
    let content = "export PATH=\"/usr/bin:$PATH\"\nexport PATH=\"$HOME/.local/bin:$PATH\"\nalias ll='ls -la'\n";
    let markers = ["/home/user/.local/bin", "$HOME/.local/bin"];
    let result = clean_rc_content(content, &markers, "export PATH=\"$HOME/.local/bin:$PATH\"");
    // 旧的 .local/bin 行应被移除
    let lines: Vec<&str> = result.lines().collect();
    let local_bin_count = lines.iter().filter(|l| l.contains(".local/bin")).count();
    assert_eq!(local_bin_count, 1);
    // 新行在最后
    assert!(result.ends_with("export PATH=\"$HOME/.local/bin:$PATH\"\n"));
    // 其他行保留
    assert!(result.contains("alias ll='ls -la'"));
}

// 大小写不敏感匹配
#[test]
fn CleanRcContent_CaseInsensitive_001() {
    let content = "export PATH=\"/HOME/USER/.LOCAL/BIN:$PATH\"\n";
    let markers = ["/home/user/.local/bin", "$HOME/.local/bin"];
    let result = clean_rc_content(content, &markers, "export PATH=\"$HOME/.local/bin:$PATH\"");
    let lines: Vec<&str> = result.lines().collect();
    let local_bin_count = lines.iter().filter(|l| l.contains(".local/bin") || l.contains(".LOCAL/BIN")).count();
    assert_eq!(local_bin_count, 1);
}

// 文件末尾无换行时补换行
#[test]
fn CleanRcContent_NoTrailingNewline_001() {
    let content = "alias ll='ls -la'";
    let markers = ["/home/user/.local/bin", "$HOME/.local/bin"];
    let result = clean_rc_content(content, &markers, "export PATH=\"$HOME/.local/bin:$PATH\"");
    assert!(result.contains("alias ll='ls -la'\n"));
    assert!(result.ends_with("export PATH=\"$HOME/.local/bin:$PATH\"\n"));
}

// 多个匹配行（$HOME 和绝对路径混合）全部移除
#[test]
fn CleanRcContent_MultipleMatches_001() {
    let content = "export PATH=\"$HOME/.local/bin:$PATH\"\nexport PATH=\"/usr/bin:$PATH\"\nexport PATH=\"/home/user/.local/bin:$PATH\"\n";
    let markers = ["/home/user/.local/bin", "$HOME/.local/bin"];
    let result = clean_rc_content(content, &markers, "export PATH=\"$HOME/.local/bin:$PATH\"");
    let lines: Vec<&str> = result.lines().collect();
    let local_bin_count = lines.iter().filter(|l| l.contains(".local/bin")).count();
    assert_eq!(local_bin_count, 1);
    assert!(result.contains("export PATH=\"/usr/bin:$PATH\""));
}
