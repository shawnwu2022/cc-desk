//! 只在新建隔离目录中验证 Windows 分享模式与固定 SDK；不接入生产删除能力。
#![allow(non_snake_case)]
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE,
};

const ID: &str = "27000000-0000-4000-8000-000000000001";
const SDK_HASH: &str = "6b963b3eb871949a14392bb35ff970a55e91e2edbe218c61cd9eed60e46cbe45";
struct Fixture {
    root: tempfile::TempDir,
    configuration: PathBuf,
    project: PathBuf,
    transcript: PathBuf,
    children: PathBuf,
    helper: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("v027-claude-guard-")
            .tempdir()
            .unwrap();
        let configuration = root.path().join("configuration");
        let project = configuration.join("projects/authorized");
        let children = project.join(ID);
        fs::create_dir_all(&children).unwrap();
        let transcript = project.join(format!("{ID}.jsonl"));
        fs::write(
            &transcript,
            format!(
                "{}\n",
                serde_json::json!({"type":"system","sessionId":ID,"cwd":root.path()})
            ),
        )
        .unwrap();
        let helper = root.path().join("helper");
        fs::create_dir(&helper).unwrap();
        // 分片只下载已编译测试程序；固定 SDK 仅嵌入测试，不要求分片安装 node_modules。
        let bytes = include_bytes!("../../../node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs");
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), SDK_HASH);
        fs::write(helper.join("sdk.mjs"), bytes).unwrap();
        Self {
            root,
            configuration,
            project,
            transcript,
            children,
            helper,
        }
    }
    fn contained(&self, path: &Path) {
        assert!(path
            .canonicalize()
            .unwrap()
            .starts_with(self.root.path().canonicalize().unwrap()));
    }
    fn sdk_delete(&self, configuration: &Path, extra_permissions: &[&Path]) -> serde_json::Value {
        self.contained(configuration);
        self.contained(&self.helper);
        for path in extra_permissions {
            self.contained(path);
        }
        let runner = self.helper.join("delete.mjs");
        fs::write(&runner,format!(r#"import {{deleteSession}} from './sdk.mjs';
let success=true,code=null;
try {{await deleteSession({id});}} catch(error) {{success=false;code=['EPERM','EBUSY','EACCES','ENOENT','ENOTDIR','ERR_ACCESS_DENIED'].includes(error?.code)?error.code:'SDK_ERROR';}}
console.log(JSON.stringify({{success,code}}));
"#,id=serde_json::json!(ID))).unwrap();
        let node = Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .unwrap();
        assert!(node.status.success());
        let node = String::from_utf8(node.stdout).unwrap().trim().to_owned();
        let mut command = Command::new(node);
        crate::platform::configure_command(&mut command);
        command.arg("--permission");
        for (flag, path) in [
            ("--allow-fs-read=", self.helper.as_path()),
            ("--allow-fs-read=", configuration),
            ("--allow-fs-write=", configuration),
        ] {
            let mut arg = std::ffi::OsString::from(flag);
            arg.push(path);
            command.arg(arg);
        }
        for path in extra_permissions {
            for flag in ["--allow-fs-read=", "--allow-fs-write="] {
                let mut arg = std::ffi::OsString::from(flag);
                arg.push(path);
                command.arg(arg);
            }
        }
        command
            .arg(&runner)
            .current_dir(self.root.path())
            .env_clear();
        command.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
        command.env("CLAUDE_CONFIG_DIR", configuration);
        for key in [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "TEMP",
            "TMP",
        ] {
            let path = self.root.path().join(key.to_ascii_lowercase());
            fs::create_dir_all(&path).unwrap();
            command.env(key, path);
        }
        // Node 未获 child_process/worker 权限，只能调用这个固定 SDK 的磁盘删除导出。
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut output = child.stdout.take().unwrap();
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            output.by_ref().take(65537).read_to_end(&mut bytes).unwrap();
            bytes
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let status = child.wait().unwrap();
                panic!("SDK fixture timeout: {status}");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let bytes = reader.join().unwrap();
        assert!(status.success(), "isolated SDK runner failed: {status}");
        assert!(bytes.len() <= 65536);
        serde_json::from_slice(&bytes).unwrap()
    }
}
fn file_guard(path: &Path, allow_delete: bool) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | if allow_delete { FILE_SHARE_DELETE.0 } else { 0 })
        .open(path)
}
fn directory_guard(path: &Path, allow_delete: bool) -> std::io::Result<File> {
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | if allow_delete { FILE_SHARE_DELETE.0 } else { 0 })
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)
}
fn writer(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .append(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .open(path)
}
fn junction(link: &Path, target: &Path) {
    let mut command = Command::new("cmd.exe");
    crate::platform::configure_command(&mut command);
    assert!(command
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(target.to_string_lossy().replace('/', "\\"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success());
}
fn rename_delete_handle(path: &Path, target: &Path) -> std::io::Result<()> {
    use windows::Win32::Storage::FileSystem::DELETE;
    let file = OpenOptions::new()
        .access_mode(DELETE.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .open(path)?;
    rename_held_delete_handle(&file, target, false)
}
fn rename_held_delete_handle(file: &File, target: &Path, extended: bool) -> std::io::Result<()> {
    use std::os::windows::{ffi::OsStrExt, io::AsRawHandle};
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FileRenameInfo, FileRenameInfoEx, SetFileInformationByHandle, FILE_RENAME_INFO,
        },
    };
    let name: Vec<u16> = std::ffi::OsStr::new(&target.to_string_lossy().replace('/', "\\"))
        .encode_wide()
        .collect();
    let size = std::mem::size_of::<FILE_RENAME_INFO>() + name.len() * 2;
    let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    unsafe {
        let information = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*information).Anonymous.ReplaceIfExists = false;
        (*information).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(
            name.as_ptr(),
            (*information).FileName.as_mut_ptr(),
            name.len(),
        );
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            if extended {
                FileRenameInfoEx
            } else {
                FileRenameInfo
            },
            information.cast(),
            size as u32,
        )
        .map_err(std::io::Error::from)
    }
}

struct FixtureCreationDeny {
    path: PathBuf,
    snapshot: PathBuf,
    restored: bool,
}
impl FixtureCreationDeny {
    fn new(fixture: &Fixture, path: &Path) -> Self {
        fixture.contained(path);
        let snapshot = fixture
            .root
            .path()
            .join(format!("acl-{}.save", uuid::Uuid::new_v4()));
        assert!(acl_command(path, &["/save"], Some(&snapshot), &["/Q"]));
        let saved = fs::read(&snapshot).unwrap();
        let units: Vec<u16> = saved
            .as_chunks::<2>()
            .0
            .iter()
            .copied()
            .map(u16::from_le_bytes)
            .collect();
        let name = String::from_utf16_lossy(&units)
            .trim_start_matches('\u{feff}')
            .lines()
            .next()
            .unwrap_or("")
            .to_owned();
        println!(
            "{}",
            serde_json::json!({"case":"fixture-acl-snapshot-name","name":name})
        );
        assert!(acl_command(
            path,
            &["/deny", "*S-1-1-0:(WD,AD)", "/Q"],
            None,
            &[]
        ));
        Self {
            path: path.to_owned(),
            snapshot,
            restored: false,
        }
    }
    fn restore(mut self) {
        assert!(self.restore_fixture_deny());
        self.restored = true;
    }
    fn restore_fixture_deny(&self) -> bool {
        let payload = |path: &Path| -> Vec<String> {
            let bytes = fs::read(path).unwrap();
            let units: Vec<u16> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect();
            String::from_utf16_lossy(&units)
                .trim_start_matches('\u{feff}')
                .lines()
                .skip(1)
                .map(str::to_owned)
                .collect()
        };
        let original = payload(&self.snapshot).join("\n");
        // icacls /deny 也会移除相同的显式 grant，/remove:d 不是其逆操作。
        // 仅在自建 fixture 上还原已保存的完整 DACL；不设置 owner/group/SACL，
        // 不申请额外 privilege。随后仍执行严格的原始描述符核对。
        let Some(descriptor) = FixtureDescriptor::new(&original) else {
            return false;
        };
        if !descriptor.restore(&self.path) {
            return false;
        }
        let after = self.snapshot.with_extension("after");
        if !acl_command(&self.path, &["/save"], Some(&after), &["/Q"]) {
            return false;
        }
        restored_fixture_acl_matches(&original, &payload(&after).join("\n"))
    }
}
struct FixtureDescriptor {
    value: windows::Win32::Security::PSECURITY_DESCRIPTOR,
    length: usize,
}
impl FixtureDescriptor {
    fn new(text: &str) -> Option<Self> {
        use windows::{
            core::PCWSTR,
            Win32::Security::{
                Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
                GetSecurityDescriptorLength, IsValidSecurityDescriptor, PSECURITY_DESCRIPTOR,
            },
        };
        if text.len() > 65536 || text.contains('\0') {
            return None;
        }
        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let mut value = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                1,
                &mut value,
                None,
            )
            .ok()?;
            let length = GetSecurityDescriptorLength(value) as usize;
            let descriptor = Self { value, length };
            if IsValidSecurityDescriptor(value).as_bool() && (20..=65536).contains(&length) {
                Some(descriptor)
            } else {
                None
            }
        }
    }
    fn bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.value.0.cast::<u8>(), self.length) }
    }
    fn restore(&self, path: &Path) -> bool {
        use std::os::windows::ffi::OsStrExt;
        use windows::{
            core::{BOOL, PCWSTR},
            Win32::Security::{
                Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT},
                GetSecurityDescriptorControl, GetSecurityDescriptorDacl, ACL,
                DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
                UNPROTECTED_DACL_SECURITY_INFORMATION,
            },
        };
        let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut present = BOOL::default();
        let mut defaulted = BOOL::default();
        let mut dacl: *mut ACL = std::ptr::null_mut();
        let mut control = 0;
        let mut revision = 0;
        unsafe {
            if GetSecurityDescriptorDacl(self.value, &mut present, &mut dacl, &mut defaulted)
                .is_err()
                || !present.as_bool()
                || dacl.is_null()
                || GetSecurityDescriptorControl(self.value, &mut control, &mut revision).is_err()
            {
                return false;
            }
            let information = DACL_SECURITY_INFORMATION
                | if control & SE_DACL_PROTECTED.0 != 0 {
                    PROTECTED_DACL_SECURITY_INFORMATION
                } else {
                    UNPROTECTED_DACL_SECURITY_INFORMATION
                };
            SetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                information,
                None,
                None,
                Some(dacl.cast_const()),
                None,
            )
            .ok()
            .is_ok()
        }
    }
}
impl Drop for FixtureDescriptor {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(self.value.0),
            ));
        }
    }
}
fn restored_fixture_acl_matches(expected: &str, actual: &str) -> bool {
    let (Some(expected), Some(actual)) = (
        FixtureDescriptor::new(expected),
        FixtureDescriptor::new(actual),
    ) else {
        return false;
    };
    // Windows 可在恢复时仅补 AUTO_INHERITED；复用已有的逐字节边界，不能
    // 排序 ACE、移除控制位或忽略 SID/权限差异。只比较任务合成 fixture 的 DACL。
    crate::version_history::windows::security::restored_file_descriptor_matches(
        expected.bytes(),
        actual.bytes(),
    )
}

#[test]
fn ClaudeDelete_FixtureRestorationAllowsOnlyDocumentedInheritanceFlag_011() {
    let original = "D:(A;OICI;FA;;;SY)";
    let inherited = "D:AI(A;OICI;FA;;;SY)";
    assert!(restored_fixture_acl_matches(original, original));
    assert!(restored_fixture_acl_matches(original, inherited));
    assert!(!restored_fixture_acl_matches(inherited, original));
    assert!(!restored_fixture_acl_matches(
        "not a descriptor",
        "not a descriptor"
    ));
    for changed in [
        "D:AI(A;OICI;FR;;;SY)",
        "D:PAI(A;OICI;FA;;;SY)",
        "D:AI(A;OICI;FA;;;BA)",
        "D:AI(A;;FA;;;SY)",
        "D:AI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)",
        "D:AI(A;OICI;FA;;;SY)\0ignored",
        "not a security descriptor",
    ] {
        assert!(!restored_fixture_acl_matches(original, changed));
    }
    assert!(!restored_fixture_acl_matches(original, &"x".repeat(65537)));
}

#[test]
fn ClaudeDelete_FixtureRestorationPreservesExistingExplicitGrants_012() {
    let f = Fixture::new();
    assert!(acl_command(
        &f.project,
        &["/grant", "*S-1-1-0:(WD,AD)", "/Q"],
        None,
        &[],
    ));
    let acl = FixtureCreationDeny::new(&f, &f.project);
    assert!(fs::write(f.project.join("denied-create.jsonl"), "synthetic\n").is_err());
    acl.restore();
    assert!(fs::write(f.project.join("restored-create.jsonl"), "synthetic\n").is_ok());
}
impl Drop for FixtureCreationDeny {
    fn drop(&mut self) {
        if !self.restored && self.path.exists() {
            let restored = self.restore_fixture_deny();
            if !std::thread::panicking() {
                assert!(restored, "isolated fixture ACL restoration failed");
            }
        }
    }
}
fn acl_command(path: &Path, before: &[&str], value: Option<&Path>, after: &[&str]) -> bool {
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    let mut command = Command::new(system.join("System32/icacls.exe"));
    crate::platform::configure_command(&mut command);
    command
        .env_clear()
        .env("SystemRoot", &system)
        .arg(path.to_string_lossy().replace('/', "\\"))
        .args(before);
    if let Some(value) = value {
        command.arg(value);
    }
    let output = command.args(after).output().unwrap();
    if !output.status.success() {
        println!(
            "{}",
            serde_json::json!({"case":"fixture-acl-command-failure","exitCode":output.status.code(),"stderr":String::from_utf8_lossy(&output.stderr)})
        );
    }
    output.status.success()
}
fn relative_create(parent: &File, name: &str) -> Result<File, i32> {
    use std::os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    };
    use windows::{
        core::PWSTR,
        Wdk::{
            Foundation::OBJECT_ATTRIBUTES,
            Storage::FileSystem::{
                NtCreateFile, FILE_CREATE, FILE_NON_DIRECTORY_FILE, FILE_SYNCHRONOUS_IO_NONALERT,
            },
        },
        Win32::{
            Foundation::{HANDLE, OBJ_DONT_REPARSE, UNICODE_STRING},
            Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_WRITE_DATA, SYNCHRONIZE},
            System::IO::IO_STATUS_BLOCK,
        },
    };
    assert!(!name.contains(['/', '\\', ':']));
    let mut units: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    let mut unicode = UNICODE_STRING {
        Length: (units.len() * 2) as u16,
        MaximumLength: (units.len() * 2) as u16,
        Buffer: PWSTR(units.as_mut_ptr()),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: HANDLE(parent.as_raw_handle()),
        ObjectName: &mut unicode,
        Attributes: OBJ_DONT_REPARSE,
        ..Default::default()
    };
    let mut result = HANDLE::default();
    let mut status = IO_STATUS_BLOCK::default();
    let code = unsafe {
        NtCreateFile(
            &mut result,
            FILE_WRITE_DATA | SYNCHRONIZE,
            &attributes,
            &mut status,
            None,
            FILE_ATTRIBUTE_NORMAL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if code.0 != 0 {
        return Err(code.0);
    }
    Ok(unsafe { File::from_raw_handle(result.0) })
}

#[test]
fn ClaudeDelete_FixtureDaclAndOldDirectoryCapabilities_009() {
    use windows::Win32::Storage::FileSystem::{FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY};
    let f = Fixture::new();
    let known = f.children.join("known.jsonl");
    fs::write(&known, "known child\n").unwrap();
    let add_handle = OpenOptions::new()
        .access_mode(FILE_ADD_FILE.0 | FILE_ADD_SUBDIRECTORY.0)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
        .open(&f.children)
        .unwrap();
    let rejected = directory_guard(&f.children, true).err().unwrap();
    assert_eq!(rejected.raw_os_error(), Some(32));
    drop(add_handle);
    let old_read = directory_guard(&f.children, true).unwrap();
    let before = relative_create(&old_read, "before-dacl.jsonl");
    let before_ok = before.is_ok();
    assert!(
        before_ok,
        "the old read-only directory capability must create before the fixture DACL is applied"
    );
    drop(before);
    let _acl = FixtureCreationDeny::new(&f, &f.children);
    let path_create = fs::write(f.children.join("after-dacl.jsonl"), "late child\n");
    let directory_create = fs::create_dir(f.children.join("after-dacl-dir"));
    let relative = relative_create(&old_read, "old-read-relative.jsonl");
    println!(
        "{}",
        serde_json::json!({"case":"fixture-dacl-directory-capabilities","oldAddFileHandleRejectedByGuard":true,"oldReadRelativeCreateBeforeDacl":before_ok,"pathCreateAfterDacl":path_create.is_ok(),"directoryCreateAfterDacl":directory_create.is_ok(),"oldReadRelativeCreateAfterDacl":relative.is_ok(),"relativeNtStatus":relative.as_ref().err()})
    );
    assert!(path_create.is_err());
    assert!(directory_create.is_err());
    assert_eq!(relative.as_ref().err(), Some(&-1073741790));
    let _root = file_guard(&f.transcript, true).unwrap();
    let _known = file_guard(&known, true).unwrap();
    let result = f.sdk_delete(&f.configuration, &[]);
    println!(
        "{}",
        serde_json::json!({"case":"sdk-delete-with-fixture-creation-dacl","result":result,"rootVisible":f.transcript.exists(),"childrenVisible":f.children.exists()})
    );
    assert_eq!(result["success"], true);
    assert!(!f.transcript.exists());
    assert!(!f.children.exists());
}

#[test]
fn ClaudeDelete_OldDeleteHandleCanMoveOutsideProtectedSource_010() {
    use windows::Win32::Storage::FileSystem::DELETE;
    for extended in [false, true] {
        let f = Fixture::new();
        let outside = f.root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let old_delete = OpenOptions::new()
            .access_mode(DELETE.0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .open(&f.transcript)
            .unwrap();
        let writer_guard = file_guard(&f.transcript, true).unwrap();
        let project = directory_guard(&f.project, false).unwrap();
        // 较弱的身份租约仍拒绝目录替换，但允许恢复 ACL 所需的写句柄。
        let _identity = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(&f.project)
            .unwrap();
        let acl = FixtureCreationDeny::new(&f, &f.project);
        let moved = outside.join("moved-original.jsonl");
        let result = rename_held_delete_handle(&old_delete, &moved, extended);
        let new_name = fs::write(&f.transcript, "replacement attempt\n");
        println!(
            "{}",
            serde_json::json!({"case":"old-delete-handle-source-move","extended":extended,"moveSucceeded":result.is_ok(),"moveError":result.as_ref().err().and_then(std::io::Error::raw_os_error),"sourcePresent":f.transcript.exists(),"outsideOriginalPresent":moved.exists(),"newSourceNameAllowed":new_name.is_ok()})
        );
        assert!(new_name.is_err());
        assert!(result.is_ok(),"the observed old DELETE-handle move must remain reproducible for extended={extended}: {result:?}");
        assert!(moved.exists());
        assert!(!f.transcript.exists());
        drop(project);
        drop(writer_guard);
        acl.restore();
        assert!(fs::write(
            f.project.join("restored-create.keep"),
            "restored fixture ACL\n"
        )
        .is_ok());
        println!(
            "{}",
            serde_json::json!({"case":"fixture-acl-restoration-order","extended":extended,"restoredAfterWriterGuardRelease":true,"directoryIdentityLeaseRetained":true})
        );
    }
}

#[test]
fn ClaudeDelete_FileShareRefusesExistingAndNewWriter_001() {
    let f = Fixture::new();
    let mut existing = writer(&f.transcript).unwrap();
    let denied = file_guard(&f.transcript, true).err().unwrap();
    assert_eq!(denied.raw_os_error(), Some(32));
    existing
        .write_all(b"existing writer still owns its file\n")
        .unwrap();
    drop(existing);
    let guard = file_guard(&f.transcript, true).unwrap();
    assert_eq!(
        writer(&f.transcript).err().unwrap().raw_os_error(),
        Some(32)
    );
    assert!(File::open(&f.transcript).is_ok());
    println!(
        "{}",
        serde_json::json!({"case":"file-share","existingWriterBlocksGuard":true,"newWriterDenied":true,"readerAllowed":true})
    );
    drop(guard);
}

#[test]
fn ClaudeDelete_ReadDeleteShareAllowsSdkOriginalRemoval_002() {
    let f = Fixture::new();
    fs::write(f.children.join("child.jsonl"), "synthetic child\n").unwrap();
    let guard = file_guard(&f.transcript, true).unwrap();
    let result = f.sdk_delete(&f.configuration, &[]);
    println!(
        "{}",
        serde_json::json!({"case":"sdk-with-file-guard","result":result,"rootVisibleWhileGuardHeld":f.transcript.exists(),"childrenVisible":f.children.exists()})
    );
    assert_eq!(result["success"], true);
    drop(guard);
    assert!(!f.transcript.exists());
    assert!(!f.children.exists());
}

#[test]
fn ClaudeDelete_DirectoryDenyWriteDoesNotFreezeChildren_003() {
    let f = Fixture::new();
    let guard = directory_guard(&f.children, true).unwrap();
    let created = fs::write(
        f.children.join("late-child.jsonl"),
        "synthetic late child\n",
    )
    .is_ok();
    println!(
        "{}",
        serde_json::json!({"case":"directory-share-hypothesis","newChildCreated":created})
    );
    // 首轮保护假设已实测失败；这里只固化反例，生产能力仍关闭。
    assert!(
        created,
        "expected the previously observed directory sharing counterexample"
    );
    assert!(fs::create_dir(f.children.join("new-subtree")).is_ok());
    drop(guard);
    let _exclusive = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(&f.children)
        .unwrap();
    let exclusive_created = fs::write(
        f.children.join("exclusive-late-child.jsonl"),
        "late child under exclusive directory handle\n",
    )
    .is_ok();
    println!(
        "{}",
        serde_json::json!({"case":"directory-exclusive-share-counterexample","newChildCreated":exclusive_created})
    );
    assert!(exclusive_created);
}

#[test]
fn ClaudeDelete_DeleteShareNamespaceReplacementExperiment_004() {
    let f = Fixture::new();
    let mut original = file_guard(&f.transcript, true).unwrap();
    let _container = directory_guard(&f.project, false).unwrap();
    let saved = f.project.join("original.keep");
    let renamed = rename_delete_handle(&f.transcript, &saved);
    println!(
        "{}",
        serde_json::json!({"case":"delete-only-handle-rename","succeeded":renamed.is_ok(),"error":renamed.as_ref().err().and_then(std::io::Error::raw_os_error)})
    );
    if renamed.is_err() {
        let external = f.sdk_delete(&f.configuration, &[]);
        println!(
            "{}",
            serde_json::json!({"case":"external-unlink-with-file-guard","result":external})
        );
        assert_eq!(external["success"], true);
    }
    let recreated = fs::write(&f.transcript, "replacement with same UUID filename\n");
    println!(
        "{}",
        serde_json::json!({"case":"namespace-recreate-with-original-guard","succeeded":recreated.is_ok(),"error":recreated.as_ref().err().and_then(std::io::Error::raw_os_error)})
    );
    assert!(
        recreated.is_ok(),
        "the observed namespace recreation must remain reproducible: {recreated:?}"
    );
    let mut replacement_writer = writer(&f.transcript).unwrap();
    let result = f.sdk_delete(&f.configuration, &[]);
    replacement_writer
        .write_all(b"replacement writer survives native unlink\n")
        .unwrap();
    original.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = String::new();
    original.read_to_string(&mut bytes).unwrap();
    println!(
        "{}",
        serde_json::json!({"case":"replacement-counterexample","result":result,"originalRenamedRetained":saved.exists(),"replacementVisible":f.transcript.exists(),"replacementWriterStillUsable":true,"originalHandleStillReadsOriginal":bytes.contains("sessionId")})
    );
    assert!(bytes.contains("sessionId"));
    assert_eq!(result["success"], true);
    assert!(!f.transcript.exists());
}

#[test]
fn ClaudeDelete_DirectoryShareDoesNotExcludeOldOrLateChildWriter_007() {
    let f = Fixture::new();
    let old = f.children.join("old-child.jsonl");
    fs::write(&old, "old child\n").unwrap();
    let mut old_writer = writer(&old).unwrap();
    let _directory = directory_guard(&f.children, true).unwrap();
    old_writer
        .write_all(b"directory guard did not reject this writer\n")
        .unwrap();
    assert!(file_guard(&old, true).is_err());
    drop(old_writer);
    let _known_old = file_guard(&old, true).unwrap();
    let late = f.children.join("late-child.jsonl");
    fs::write(&late, "late child\n").unwrap();
    let mut late_writer = writer(&late).unwrap();
    let _root = file_guard(&f.transcript, true).unwrap();
    let result = f.sdk_delete(&f.configuration, &[]);
    late_writer.write_all(b"late writer after SDK\n").unwrap();
    println!(
        "{}",
        serde_json::json!({"case":"child-writer-counterexample","result":result,"rootVisible":f.transcript.exists(),"oldChildVisible":old.exists(),"lateChildVisible":late.exists(),"oldWriterNotExcludedByDirectory":true,"knownOldFileGuardHeld":true,"lateWriterUsableAfterSdk":true})
    );
    assert_eq!(result["success"], true);
    assert!(!f.transcript.exists());
    assert!(!old.exists());
    assert!(!late.exists());
}

#[test]
fn ClaudeDelete_LockedChildProducesPartialRemoval_005() {
    let f = Fixture::new();
    let child = f.children.join("locked-child.jsonl");
    fs::write(&child, "synthetic protected child\n").unwrap();
    let _child = file_guard(&child, false).unwrap();
    let result = f.sdk_delete(&f.configuration, &[]);
    println!(
        "{}",
        serde_json::json!({"case":"partial-counterexample","result":result,"rootVisible":f.transcript.exists(),"lockedChildRetained":child.exists()})
    );
    assert_eq!(result["success"], false);
    assert_eq!(result["code"], "EBUSY");
    assert!(!f.transcript.exists());
    assert!(child.exists());
}

#[test]
fn ClaudeDelete_ProjectJunctionMapsActualOriginal_006() {
    let f = Fixture::new();
    let private = f.root.path().join("private");
    fs::create_dir_all(private.join("projects")).unwrap();
    junction(&private.join("projects/authorized"), &f.project);
    let _container = directory_guard(&f.project, false).unwrap();
    let _alias = directory_guard(&private.join("projects/authorized"), false).unwrap();
    assert!(fs::rename(&f.project, f.configuration.join("moved-project")).is_err());
    assert!(fs::remove_dir(private.join("projects/authorized")).is_err());
    let guard = file_guard(&f.transcript, true).unwrap();
    let result = f.sdk_delete(&private, &[]);
    println!(
        "{}",
        serde_json::json!({"case":"project-junction-private-permission","result":result,"originalVisible":f.transcript.exists()})
    );
    drop(guard);
    assert_eq!(result["success"], true);
    assert!(!f.transcript.exists());
    assert!(!f.children.exists());
}

#[test]
fn ClaudeDelete_RecursiveSdkDoesNotFollowChildJunction_008() {
    let f = Fixture::new();
    let outside = f.root.path().join("external-data");
    fs::create_dir(&outside).unwrap();
    let keep = outside.join("external.keep");
    fs::write(&keep, "synthetic external data retained\n").unwrap();
    junction(&f.children.join("linked-outside"), &outside);
    let result = f.sdk_delete(&f.configuration, &[]);
    println!(
        "{}",
        serde_json::json!({"case":"recursive-child-junction","result":result,"rootVisible":f.transcript.exists(),"childrenVisible":f.children.exists(),"externalMarkerRetained":keep.exists()})
    );
    assert_eq!(result["success"], true);
    assert!(!f.transcript.exists());
    assert!(!f.children.exists());
    assert!(keep.exists());
    assert_eq!(
        fs::read_to_string(&keep).unwrap(),
        "synthetic external data retained\n"
    );
}
