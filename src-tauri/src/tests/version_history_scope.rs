//! Scope observations, not installation or snapshot authority.
use crate::cli::{
    environment::EnvMap,
    profiles::{Override, Profile},
    storage::WorkspaceDocument,
    types::CliKind,
};
use crate::version_history::windows::scope::{
    configured_candidates, ConfigScopeInputs, ObservedPath, PathKind, ProjectsScopeInputs,
    ScopeBlock, ScopeInputs,
};
use std::ffi::OsString;

// 检查缺省Legacy配置和固定shell传输可形成完整静态候选，不把整个home当成辅助配置根。
#[test]
fn HistoryScope_DefaultInputs_001() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let env = EnvMap::from([(OsString::from("USERPROFILE"), home.as_os_str().to_owned())]);
    let config = ConfigScopeInputs::decode(b"{}").unwrap();
    let projects = ProjectsScopeInputs::decode(b"{}").unwrap();
    let candidates = configured_candidates(
        home,
        &env,
        &WorkspaceDocument::default(),
        &config,
        &projects,
    )
    .unwrap();
    assert!(candidates
        .iter()
        .any(|c| c.path() == home.join(".claude") && c.kind() == PathKind::Directory));
    assert!(candidates.iter().any(|c| c.path() == home.join(".codex")));
    assert!(candidates
        .iter()
        .any(|c| c.path() == home.join(".claude.json") && c.kind() == PathKind::File));
    assert!(!candidates.iter().any(|c| c.path() == home));
}

// 检查明确defaultArgs、旧shell文本、无效root覆盖与启动脚本会阻止静态根解释。
#[test]
fn HistoryScope_ConfiguredAmbiguity_002() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let env = EnvMap::from([(OsString::from("USERPROFILE"), home.as_os_str().to_owned())]);
    let projects = ProjectsScopeInputs::decode(b"{}").unwrap();
    for bytes in [
        br#"{"defaultCustomArgs":"--root elsewhere"}"#.as_slice(),
        br#"{"claudeEnvVars":{"CLAUDE_CONFIG_DIR":""}}"#,
        br#"{"claudeEnvVars":{"BASH_ENV":"C:\\custom-startup.sh"}}"#,
    ] {
        let config = ConfigScopeInputs::decode(bytes).unwrap();
        assert!(configured_candidates(
            home,
            &env,
            &WorkspaceDocument::default(),
            &config,
            &projects
        )
        .is_err());
    }
    let mut workspace = WorkspaceDocument::default();
    let mut profile = Profile::new("custom", CliKind::Claude);
    profile.default_args = Override::Set(vec!["--root".into()]);
    workspace.profiles.insert(profile.id.clone(), profile);
    assert!(configured_candidates(
        home,
        &env,
        &workspace,
        &ConfigScopeInputs::decode(b"{}").unwrap(),
        &projects
    )
    .is_err());
}

// 检查所有已持久化项目选择器均保留，损坏或重复字段不能被容错decoder静默忽略。
#[test]
fn HistoryScope_StrictProjects_003() {
    for bytes in [
        br#"{"displayNames":{"C:\\work":7}}"#.as_slice(),
        br#"{"pinnedProjects":["C:\\a"],"pinnedProjects":[]}"#,
        br#"{"sessionRecords":{"id":{"projectPath":"C:\\work"}}}"#,
        br#"{"archivedSessions":false}"#,
    ] {
        assert!(ProjectsScopeInputs::decode(bytes).is_err());
    }
    assert!(ConfigScopeInputs::decode(br#"{"claudeEnvVars":{"X":"a","X":"b"}}"#).is_err());
    assert!(ConfigScopeInputs::decode(b"[]").is_err());
    assert!(ConfigScopeInputs::decode(&vec![b' '; 1024 * 1024 + 1]).is_err());
}

// 检查纯workspace decoder拒绝未知schema，不创建目录或永久lock。
#[test]
fn HistoryScope_NoBootstrapWrites_004() {
    let temporary = tempfile::tempdir().unwrap();
    let desk = temporary.path().join(".cc-box");
    let inputs = ScopeInputs::capture(&desk).unwrap();
    inputs.recheck().unwrap();
    assert!(!desk.exists());
    assert!(crate::cli::storage::decode_workspace(br#"{"schemaVersion":2}"#).is_err());
    assert!(std::fs::read_dir(temporary.path())
        .unwrap()
        .next()
        .is_none());
}

// 检查held parent下的缺失后缀不创建目录；新建首个组件后原absence观察失效。
#[test]
fn HistoryScope_ProspectivePaths_005() {
    let temporary = tempfile::tempdir().unwrap();
    let absent = temporary.path().join("missing").join("nested");
    let observation = ObservedPath::observe(&absent, PathKind::Directory).unwrap();
    observation.recheck().unwrap();
    assert!(!absent.exists());
    std::fs::create_dir(temporary.path().join("missing")).unwrap();
    assert_eq!(observation.recheck().err(), Some(ScopeBlock::InputChanged));
}

// 检查Windows大小写和同父缺失后缀的ancestor碰撞；辅助file叶子不会误伤兄弟Desk根。
#[test]
fn HistoryScope_Overlap_006() {
    let temporary = tempfile::tempdir().unwrap();
    let one =
        ObservedPath::observe(&temporary.path().join("Ångström"), PathKind::Directory).unwrap();
    let nested = ObservedPath::observe(
        &temporary.path().join("ångström").join("child"),
        PathKind::Directory,
    )
    .unwrap();
    assert!(one.overlaps(&nested).unwrap());
    let desk =
        ObservedPath::observe(&temporary.path().join(".cc-box"), PathKind::Directory).unwrap();
    let config =
        ObservedPath::observe(&temporary.path().join(".claude.json"), PathKind::File).unwrap();
    assert!(!desk.overlaps(&config).unwrap());
}

// 检查已捕获输入保持原file身份；未提供的文件后来出现会使观察失效。
#[test]
fn HistoryScope_InputDrift_007() {
    let temporary = tempfile::tempdir().unwrap();
    let desk = temporary.path().join("desk");
    std::fs::create_dir(&desk).unwrap();
    std::fs::write(desk.join("config.json"), b"{}").unwrap();
    let inputs = ScopeInputs::capture(&desk).unwrap();
    assert!(std::fs::write(desk.join("config.json"), b"changed").is_err());
    std::fs::write(desk.join("projects.json"), b"{}").unwrap();
    assert_eq!(inputs.recheck().err(), Some(ScopeBlock::InputChanged));
}

// 检查环境覆盖和指定CLI/runner文件属于精确候选，而不是父目录或动态shell阻塞。
#[test]
fn HistoryScope_ExecutableSelectors_008() {
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let executable = home.join("tools").join("custom.exe");
    let env = EnvMap::from([
        (OsString::from("USERPROFILE"), home.as_os_str().to_owned()),
        (
            OsString::from("CLAUDE_CODE_GIT_BASH_PATH"),
            executable.as_os_str().to_owned(),
        ),
    ]);
    let config = ConfigScopeInputs::decode(&serde_json::to_vec(&serde_json::json!({"claudePath":executable,"claudeEnvVars":{"CLAUDE_CONFIG_DIR":home.join("custom-root")}})).unwrap()).unwrap();
    let candidates = configured_candidates(
        home,
        &env,
        &WorkspaceDocument::default(),
        &config,
        &ProjectsScopeInputs::decode(b"{}").unwrap(),
    )
    .unwrap();
    assert!(candidates
        .iter()
        .any(|c| c.path() == executable && c.kind() == PathKind::File));
    assert!(candidates
        .iter()
        .any(|c| c.path() == home.join("custom-root")));
    assert!(!candidates.iter().any(|c| c.path() == home.join("tools")));
}

struct RegistryFixture {
    path: Vec<u16>,
    user: windows::Win32::System::Registry::HKEY,
    machine: windows::Win32::System::Registry::HKEY,
}
impl RegistryFixture {
    fn new() -> Self {
        use windows::Win32::System::Registry::*;
        let path = format!("Software\\CCDeskScopeTests\\{}", uuid::Uuid::new_v4());
        Self {
            user: Self::key(HKEY_CURRENT_USER, &format!("{path}\\User")),
            machine: Self::key(HKEY_CURRENT_USER, &format!("{path}\\Machine")),
            path: path.encode_utf16().chain(Some(0)).collect(),
        }
    }
    fn key(
        root: windows::Win32::System::Registry::HKEY,
        path: &str,
    ) -> windows::Win32::System::Registry::HKEY {
        use windows::Win32::System::Registry::*;
        use windows_core::PCWSTR;
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                root,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_VOLATILE,
                KEY_ALL_ACCESS,
                None,
                &mut key,
                None,
            )
            .ok()
            .unwrap();
        }
        key
    }
    fn string(&self, machine: bool, key: &str, name: &str, value: &str) {
        use windows::Win32::System::Registry::*;
        use windows_core::PCWSTR;
        let key = Self::key(if machine { self.machine } else { self.user }, key);
        let name: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        let value: Vec<_> = value
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        unsafe {
            RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&value))
                .ok()
                .unwrap();
            RegCloseKey(key).ok().unwrap();
        }
    }
    fn rename(&self, path: &str, new_name: &str) {
        use windows::Win32::System::Registry::*;
        use windows_core::PCWSTR;
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let new_name: Vec<_> = new_name.encode_utf16().chain(Some(0)).collect();
        unsafe {
            RegRenameKey(self.user, PCWSTR(path.as_ptr()), PCWSTR(new_name.as_ptr()))
                .ok()
                .unwrap();
        }
    }
    fn install(&self, directory: &std::path::Path, machine: bool) {
        let root = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk";
        for (name, value) in [
            ("DisplayName", "CC Desk".to_owned()),
            ("Publisher", "shawnwu2022".into()),
            ("MainBinaryName", "cc-desk.exe".into()),
            ("DisplayVersion", env!("CARGO_PKG_VERSION").into()),
            ("InstallLocation", format!("\"{}\"", directory.display())),
            (
                "DisplayIcon",
                format!("\"{}\"", directory.join("cc-desk.exe").display()),
            ),
            (
                "UninstallString",
                format!("\"{}\"", directory.join("uninstall.exe").display()),
            ),
        ] {
            self.string(machine, root, name, &value);
        }
        self.string(
            machine,
            "Software\\shawnwu2022\\CC Desk",
            "",
            directory.to_str().unwrap(),
        );
    }
}
impl Drop for RegistryFixture {
    fn drop(&mut self) {
        use windows::Win32::System::Registry::*;
        use windows_core::PCWSTR;
        unsafe {
            let _ = RegCloseKey(self.user);
            let _ = RegCloseKey(self.machine);
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(self.path.as_ptr()));
        }
    }
}

// 检查实际只读registry枚举将共享HKCU视图视为同一记录，拒绝machine竞争者和后续更改。
#[test]
fn HistoryScope_RegistryViews_009() {
    use crate::version_history::windows::{
        registry::InstallRegistryObservation, scope::fixture_registration_location,
    };
    let temporary = tempfile::tempdir().unwrap();
    let image = temporary.path().join("cc-desk.exe");
    let mut pe = vec![0u8; 512];
    pe[..2].copy_from_slice(b"MZ");
    pe[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
    pe[128..132].copy_from_slice(b"PE\0\0");
    pe[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
    pe[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
    std::fs::write(&image, pe).unwrap();
    let registry = RegistryFixture::new();
    registry.install(temporary.path(), false);
    let observed =
        InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
    fixture_registration_location(&observed, &image).unwrap();
    observed.recheck().unwrap();
    registry.install(temporary.path(), true);
    assert!(observed.recheck().is_err());
    let competing =
        InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
    assert_eq!(
        fixture_registration_location(&competing, &image).err(),
        Some(ScopeBlock::CompetingInstallation)
    );
}

// 检查原注册目录与实际image对象绑定，另一个相同名字的文件不能借注册字符串获准。
#[test]
fn HistoryScope_RelocatedImage_010() {
    use crate::version_history::windows::{
        registry::InstallRegistryObservation, scope::fixture_registration_location,
    };
    let registered = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(registered.path().join("cc-desk.exe"), b"same fixture image").unwrap();
    std::fs::write(elsewhere.path().join("cc-desk.exe"), b"same fixture image").unwrap();
    let registry = RegistryFixture::new();
    registry.install(registered.path(), false);
    let observed =
        InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
    assert_eq!(
        fixture_registration_location(&observed, &elsewhere.path().join("cc-desk.exe")).err(),
        Some(ScopeBlock::Relocated)
    );
}

// 检查ExactProcess helper接受本进程实际held image，拒绝独立复制的相同字节。
#[test]
fn HistoryScope_ExactRunningImage_011() {
    use crate::version_history::windows::{
        files::{ComponentName, Directory, FileAccess},
        process::ExactProcess,
    };
    let process = ExactProcess::capture_observed(std::process::id()).unwrap();
    let current = std::env::current_exe().unwrap();
    let directory = Directory::open_absolute(current.parent().unwrap()).unwrap();
    let file = directory
        .open_file(
            ComponentName::new(current.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    process.verify_held_image(&file).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let copy = temporary.path().join("copy.exe");
    std::fs::copy(&current, &copy).unwrap();
    let parent = Directory::open_absolute(temporary.path()).unwrap();
    let foreign = parent
        .open_file(
            ComponentName::new(copy.file_name().unwrap()).unwrap(),
            FileAccess::Read,
        )
        .unwrap();
    assert!(process.verify_held_image(&foreign).is_err());
}

// 检查Legacy history的首个cwd被纳入保护，忽略agent文件，不保留prompt正文。
#[test]
fn HistoryScope_LegacyProjects_012() {
    use crate::version_history::windows::scope::LegacyProjectInputs;
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join(".claude/projects/project");
    std::fs::create_dir_all(&root).unwrap();
    let project = temporary.path().join("work");
    std::fs::write(
        root.join("session.jsonl"),
        format!(
            "{{\"type\":\"snapshot\"}}\n{}\n",
            serde_json::json!({"cwd": project, "message":"private body"})
        ),
    )
    .unwrap();
    std::fs::write(root.join("agent-sub.jsonl"), b"not a project selector").unwrap();
    let observed = LegacyProjectInputs::capture(temporary.path()).unwrap();
    assert_eq!(observed.paths(), &[project]);
    observed.recheck().unwrap();
    std::fs::write(root.join("new.jsonl"), b"{}").unwrap();
    assert_eq!(observed.recheck().err(), Some(ScopeBlock::InputChanged));
}

// 检查history中的损坏或重复cwd拒绝完整观察，不能跳过后宣称没有项目。
#[test]
fn HistoryScope_LegacyMalformed_013() {
    use crate::version_history::windows::scope::LegacyProjectInputs;
    for bytes in [
        br#"{"cwd":7}"#.as_slice(),
        br#"{"cwd":"C:\\a","cwd":"C:\\b"}"#,
        b"broken json",
    ] {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join(".claude/projects/project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("session.jsonl"), bytes).unwrap();
        assert!(LegacyProjectInputs::capture(temporary.path()).is_err());
    }
}

// 检查HostRef缺失和Windows同名环境变量冲突不能变成默认root。
#[test]
fn HistoryScope_EnvironmentConflict_014() {
    use crate::cli::profiles::EnvValue;
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let env = EnvMap::from([(OsString::from("USERPROFILE"), home.as_os_str().to_owned())]);
    let config = ConfigScopeInputs::decode(
        br#"{"claudeEnvVars":{"CLAUDE_CONFIG_DIR":"C:\\a","claude_config_dir":"C:\\b"}}"#,
    )
    .unwrap();
    let projects = ProjectsScopeInputs::decode(b"{}").unwrap();
    assert!(configured_candidates(
        home,
        &env,
        &WorkspaceDocument::default(),
        &config,
        &projects
    )
    .is_err());
    let mut workspace = WorkspaceDocument::default();
    let mut profile = Profile::new("custom", CliKind::Codex);
    profile.env.insert(
        "CODEX_HOME".into(),
        Override::Set(EnvValue::HostRef {
            name: "MISSING_ROOT_INPUT".into(),
        }),
    );
    workspace.profiles.insert(profile.id.clone(), profile);
    assert!(configured_candidates(
        home,
        &env,
        &workspace,
        &ConfigScopeInputs::decode(b"{}").unwrap(),
        &projects
    )
    .is_err());
}

// 检查持久化registered project的旧object key不能借同一路径的新目录身份继续准入。
#[test]
fn HistoryScope_ProjectIdentity_015() {
    let temporary = tempfile::tempdir().unwrap();
    let desk = temporary.path().join("desk");
    let project = temporary.path().join("project");
    std::fs::create_dir(&desk).unwrap();
    std::fs::create_dir(&project).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let workspace = serde_json::json!({"schemaVersion":1,"revision":"0","profiles":{},"registeredProjects":{
        (id.clone()): {"projectId":id,"hostId":"local","sourcePathKey":"local:windows:1:2:3","selectedPath":project,"canonicalPath":null}
    }});
    std::fs::write(
        desk.join("cli-workspace.v1.json"),
        serde_json::to_vec(&workspace).unwrap(),
    )
    .unwrap();
    assert_eq!(
        ScopeInputs::capture(&desk).err(),
        Some(ScopeBlock::InputChanged)
    );
}

// 检查多个同产品uninstall key和非NSIS卸载命令拒绝；不会因文件名相同推断安装scope。
#[test]
fn HistoryScope_RegistrationRefusal_016() {
    use crate::version_history::windows::{
        registry::InstallRegistryObservation, scope::fixture_registration_location,
    };
    for duplicate in [false, true] {
        let temporary = tempfile::tempdir().unwrap();
        let image = temporary.path().join("cc-desk.exe");
        std::fs::write(&image, b"not reached when registration is invalid").unwrap();
        let registry = RegistryFixture::new();
        registry.install(temporary.path(), false);
        if duplicate {
            registry.string(
                false,
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OtherCCDesk",
                "DisplayName",
                "CC Desk",
            );
        } else {
            registry.string(
                false,
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk",
                "UninstallString",
                "MsiExec.exe /I{fixture}",
            );
        }
        let observation =
            InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
        assert_eq!(
            fixture_registration_location(&observation, &image).err(),
            Some(if duplicate {
                ScopeBlock::CompetingInstallation
            } else {
                ScopeBlock::UnsupportedRegistration
            })
        );
    }
}

// 检查每个实际解析后的runner覆盖均作为精确文件纳入，包含Legacy、literal和HostRef。
#[test]
fn HistoryScope_ResolvedRunnerOverlays_017() {
    use crate::cli::profiles::EnvValue;
    let temporary = tempfile::tempdir().unwrap();
    let home = temporary.path();
    let inherited = home.join("ambient").join("bash.exe");
    let literal = home.join("literal").join("bash.exe");
    let referenced = home.join("referenced").join("bash.exe");
    let legacy = home.join("legacy").join("bash.exe");
    let env = EnvMap::from([
        (OsString::from("USERPROFILE"), home.as_os_str().to_owned()),
        (
            OsString::from("CLAUDE_CODE_GIT_BASH_PATH"),
            inherited.as_os_str().to_owned(),
        ),
        (
            OsString::from("RUNNER_SOURCE"),
            referenced.as_os_str().to_owned(),
        ),
    ]);
    let mut workspace = WorkspaceDocument::default();
    for (id, value) in [
        (
            "literal",
            EnvValue::Literal {
                value: literal.to_str().unwrap().into(),
                non_secret: true,
            },
        ),
        (
            "referenced",
            EnvValue::HostRef {
                name: "RUNNER_SOURCE".into(),
            },
        ),
    ] {
        let mut profile = Profile::new(id, CliKind::Claude);
        profile
            .env
            .insert("claude_code_git_bash_path".into(), Override::Set(value));
        workspace.profiles.insert(profile.id.clone(), profile);
    }
    let config = ConfigScopeInputs::decode(
        &serde_json::to_vec(&serde_json::json!({
            "claudeEnvVars": {"CLAUDE_CODE_GIT_BASH_PATH": legacy}
        }))
        .unwrap(),
    )
    .unwrap();
    let projects = ProjectsScopeInputs::decode(b"{}").unwrap();
    let candidates = configured_candidates(home, &env, &workspace, &config, &projects).unwrap();
    for executable in [inherited, literal, referenced, legacy] {
        assert!(candidates
            .iter()
            .any(|candidate| candidate.path() == executable && candidate.kind() == PathKind::File));
        assert!(!candidates
            .iter()
            .any(|candidate| Some(candidate.path()) == executable.parent()));
    }
    for (value, expected) in [
        (
            EnvValue::Literal {
                value: "relative/bash.exe".into(),
                non_secret: true,
            },
            ScopeBlock::PathUnsupported,
        ),
        (
            EnvValue::HostRef {
                name: "MISSING_RUNNER".into(),
            },
            ScopeBlock::ConfiguredScopeUnknown,
        ),
    ] {
        let mut invalid = Profile::new("invalid", CliKind::Claude);
        invalid
            .env
            .insert("CLAUDE_CODE_GIT_BASH_PATH".into(), Override::Set(value));
        let mut workspace = WorkspaceDocument::default();
        workspace.profiles.insert(invalid.id.clone(), invalid);
        assert_eq!(
            configured_candidates(home, &env, &workspace, &config, &projects).err(),
            Some(expected)
        );
    }
}

// 检查完整但改名的产品key不能替代固定slot；Windows大小写等价的固定名仍准入。
#[test]
fn HistoryScope_FixedUninstallSlot_018() {
    use crate::version_history::windows::{
        registry::InstallRegistryObservation, scope::fixture_registration_location,
    };
    for (new_name, accepted) in [("Another Product", false), ("cc desk", true)] {
        let temporary = tempfile::tempdir().unwrap();
        let image = temporary.path().join("cc-desk.exe");
        let mut pe = vec![0u8; 512];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
        pe[128..132].copy_from_slice(b"PE\0\0");
        pe[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
        pe[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
        std::fs::write(&image, pe).unwrap();
        let registry = RegistryFixture::new();
        registry.install(temporary.path(), false);
        registry.rename(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk",
            "RenameIntermediary",
        );
        registry.rename(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\RenameIntermediary",
            new_name,
        );
        let observed =
            InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
        assert_eq!(
            fixture_registration_location(&observed, &image).err(),
            if accepted {
                None
            } else {
                Some(ScopeBlock::UnsupportedRegistration)
            }
        );
    }
}

// 检查祖先改名后在原namespace重建相同或不同内容均撤销旧观察，旧树仍保留。
#[test]
fn HistoryScope_RegistryNamespaceRebind_019() {
    use crate::version_history::windows::registry::InstallRegistryObservation;
    for ancestor in ["Microsoft", "shawnwu2022"] {
        for changed in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let registry = RegistryFixture::new();
            registry.install(temporary.path(), false);
            let observed =
                InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
            observed.recheck().unwrap();
            registry.rename(
                &format!("Software\\{ancestor}"),
                &format!("{ancestor}Saved"),
            );
            registry.install(temporary.path(), false);
            if changed {
                registry.string(
                    false,
                    "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\NewCompetitor",
                    "DisplayName",
                    "CC Desk",
                );
            }
            assert!(observed.recheck().is_err());
            // Reopening the saved original does not recreate or remove it.
            use windows::Win32::System::Registry::*;
            use windows_core::PCWSTR;
            let path: Vec<_> = format!("Software\\{ancestor}Saved")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut saved = HKEY::default();
            unsafe {
                RegOpenKeyExW(
                    registry.user,
                    PCWSTR(path.as_ptr()),
                    Some(REG_OPTION_OPEN_LINK.0),
                    KEY_QUERY_VALUE,
                    &mut saved,
                )
                .ok()
                .unwrap();
                RegCloseKey(saved).ok().unwrap();
            }
        }
    }
}

// 检查空或损坏JSON与字节/集合上限各自保留精确诊断，不能靠parser错误字符串分类。
#[test]
fn HistoryScope_TypedInputLimits_020() {
    for bytes in [b"".as_slice(), b"broken", br#"{"X":1,"X":2}"#] {
        assert_eq!(
            ConfigScopeInputs::decode(bytes).err(),
            Some(ScopeBlock::InputMalformed)
        );
    }
    assert_eq!(
        ConfigScopeInputs::decode(&vec![b' '; 1024 * 1024 + 1]).err(),
        Some(ScopeBlock::InputLimit)
    );
    let many = vec!["C:\\same"; 10_001];
    let bytes = serde_json::to_vec(&serde_json::json!({"pinnedProjects":many})).unwrap();
    assert_eq!(
        ProjectsScopeInputs::decode(&bytes).err(),
        Some(ScopeBlock::InputLimit)
    );
    let many: serde_json::Map<_, _> = (0..10_001)
        .map(|index| (format!("field{index}"), serde_json::Value::Null))
        .collect();
    assert_eq!(
        ConfigScopeInputs::decode(&serde_json::to_vec(&many).unwrap()).err(),
        Some(ScopeBlock::InputLimit)
    );
}

// 检查真实目录枚举超出小预算保留InputLimit，正常枚举完成且输入类型错误单独拒绝。
#[test]
fn HistoryScope_TypedEnumerationLimit_021() {
    use crate::version_history::windows::{files::Directory, scope::fixture_history_entry_count};
    let temporary = tempfile::tempdir().unwrap();
    for name in ["one", "two", "three"] {
        std::fs::write(temporary.path().join(name), b"").unwrap();
    }
    let directory = Directory::open_absolute(temporary.path()).unwrap();
    assert_eq!(
        fixture_history_entry_count(&directory, 2).err(),
        Some(ScopeBlock::InputLimit)
    );
    assert_eq!(fixture_history_entry_count(&directory, 3), Ok(3));
    let desk = temporary.path().join("desk");
    std::fs::create_dir(&desk).unwrap();
    std::fs::create_dir(desk.join("config.json")).unwrap();
    assert_eq!(
        ScopeInputs::capture(&desk).err(),
        Some(ScopeBlock::InputUnavailable)
    );
}

// 检查安装scope不借用WebView只读policy别名例外；两种视图下固定树的REG_LINK均拒绝。
#[test]
fn HistoryScope_RegistrationAliasRefusal_022() {
    use crate::version_history::windows::registry::{
        shared_policy_alias, InstallRegistryObservation, RegistryValue, RegistryView,
    };
    use windows::Win32::System::Registry::*;
    use windows_core::PCWSTR;
    let target: Vec<_> = r"\REGISTRY\MACHINE\SOFTWARE\Policies"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let value = RegistryValue {
        kind: REG_LINK.0,
        bytes: target.clone(),
    };
    for view in [RegistryView::View32, RegistryView::View64] {
        for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            assert!(!shared_policy_alias(
                root,
                view,
                "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
                1,
                false,
                &value
            ));
        }
    }
    let registry = RegistryFixture::new();
    let temporary = tempfile::tempdir().unwrap();
    registry.install(temporary.path(), false);
    let observed =
        InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
    let key = RegistryFixture::key(registry.user, "Software\\Microsoft");
    let name: Vec<_> = "SymbolicLinkValue".encode_utf16().chain(Some(0)).collect();
    unsafe {
        RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_LINK, Some(&target))
            .ok()
            .unwrap();
        RegCloseKey(key).ok().unwrap();
    }
    assert!(observed.recheck().is_err());
    assert!(InstallRegistryObservation::fixture_hives(registry.user, registry.machine).is_err());
}

// 检查有效 x64 image 只接受当前编译版本的注册，旧版本注册仍明确拒绝。
#[test]
fn HistoryScope_RegistryVersion_023() {
    use crate::version_history::windows::{
        registry::InstallRegistryObservation, scope::fixture_registration_location,
    };
    let temporary = tempfile::tempdir().unwrap();
    let image = temporary.path().join("cc-desk.exe");
    let mut pe = vec![0u8; 512];
    pe[..2].copy_from_slice(b"MZ");
    pe[0x3c..0x40].copy_from_slice(&128u32.to_le_bytes());
    pe[128..132].copy_from_slice(b"PE\0\0");
    pe[132..134].copy_from_slice(&0x8664u16.to_le_bytes());
    pe[152..154].copy_from_slice(&0x20bu16.to_le_bytes());
    std::fs::write(&image, pe).unwrap();
    let registry = RegistryFixture::new();
    registry.install(temporary.path(), false);
    {
        let observed =
            InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
        assert_eq!(
            fixture_registration_location(&observed, &image).err(),
            None,
            "current-version registration with a valid x64 image must be accepted"
        );
    }
    let wrong_version = if env!("CARGO_PKG_VERSION") == "0.18.0" {
        "0.18.1"
    } else {
        "0.18.0"
    };
    registry.string(
        false,
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk",
        "DisplayVersion",
        wrong_version,
    );
    let observed =
        InstallRegistryObservation::fixture_hives(registry.user, registry.machine).unwrap();
    assert_eq!(
        fixture_registration_location(&observed, &image).err(),
        Some(ScopeBlock::UnsupportedRegistration),
        "a different DisplayVersion must be rejected despite an otherwise valid installation"
    );
}
