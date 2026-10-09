//! Bounded evidence from fresh, fixture-owned locations only. No hive export,
//! unrelated profile collection, historical app startup or cleanup-by-guessing.
use super::{blocked, bounded_read, name, report, write_new};
use crate::version_history::{
    verified_package::sha256,
    windows::{
        files::{Directory, FileAccess},
        registry::{RegistrationKey, RegistrationSlot, RegistryView},
    },
};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::MetadataExt,
        io::{FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    sync::Arc,
};
use windows::Win32::{
    Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_HANDLE_EOF, ERROR_NO_MORE_FILES, ERROR_NO_MORE_ITEMS,
        ERROR_PATH_NOT_FOUND, HANDLE,
    },
    Security::{
        GetFileSecurityW, GetSecurityDescriptorControl, GetSecurityDescriptorLength,
        IsValidSecurityDescriptor, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, SE_SELF_RELATIVE,
    },
    Storage::FileSystem::{
        FindClose, FindFirstStreamW, FindNextStreamW, FindStreamInfoStandard,
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW, VS_FIXEDFILEINFO,
        WIN32_FIND_DATAW, WIN32_FIND_STREAM_DATA,
    },
    System::{
        Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
        },
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
            TH32CS_SNAPPROCESS,
        },
        Registry::*,
    },
    UI::Shell::{
        FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Profile,
        FOLDERID_ProgramFiles, FOLDERID_ProgramFilesX86, FOLDERID_Programs, FOLDERID_PublicDesktop,
        FOLDERID_RoamingAppData, IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DONT_VERIFY,
        SLGP_RAWPATH,
    },
};
use windows_core::{Interface, GUID, PCWSTR, PWSTR};
fn win(e: windows_core::Error) -> io::Error {
    io::Error::other(e)
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn path_wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn known(id: &GUID) -> io::Result<PathBuf> {
    let value = unsafe { SHGetKnownFolderPath(id, KF_FLAG_DONT_VERIFY, None).map_err(win)? };
    let result = unsafe { value.to_string() }
        .map(PathBuf::from)
        .map_err(io::Error::other);
    unsafe {
        CoTaskMemFree(Some(value.0.cast()));
    }
    result
}
const SLOTS: [(RegistrationSlot, &str); 6] = [
    (
        RegistrationSlot::Uninstall,
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CC Desk",
    ),
    (RegistrationSlot::Publisher, r"Software\shawnwu2022\CC Desk"),
    (
        RegistrationSlot::Directory,
        r"Software\Classes\Directory\shell\cc-desk",
    ),
    (
        RegistrationSlot::Background,
        r"Software\Classes\Directory\Background\shell\cc-desk",
    ),
    (
        RegistrationSlot::LegacyDirectory,
        r"Software\Classes\Directory\shell\cc-box",
    ),
    (
        RegistrationSlot::LegacyBackground,
        r"Software\Classes\Directory\Background\shell\cc-box",
    ),
];
const VIEWS: [(RegistryView, REG_SAM_FLAGS); 2] = [
    (RegistryView::View32, KEY_WOW64_32KEY),
    (RegistryView::View64, KEY_WOW64_64KEY),
];
struct Com;
impl Com {
    fn enter() -> io::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(win)?;
        }
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
pub(super) struct Scope {
    absent_paths: Vec<PathBuf>,
    shortcuts: Vec<PathBuf>,
    all_shortcuts: Vec<PathBuf>,
    _com: Com,
}
impl Scope {
    pub(super) fn capture() -> io::Result<Self> {
        let com = Com::enter()?;
        let profile = known(&FOLDERID_Profile)?;
        let local = known(&FOLDERID_LocalAppData)?;
        let roaming = known(&FOLDERID_RoamingAppData)?;
        for p in [&profile, &local, &roaming] {
            Directory::open_absolute(p)?;
        }
        let mut absent_paths = vec![profile.join(".cc-box"), profile.join(".cc-desk")];
        for base in [&local, &roaming] {
            for leaf in [
                "io.github.shawnwu2022.ccdesk",
                "com.cc-box.app",
                "CC Desk",
                "CC-Box",
            ] {
                absent_paths.push(base.join(leaf));
            }
        }
        for id in [&FOLDERID_ProgramFiles, &FOLDERID_ProgramFilesX86] {
            absent_paths.push(known(id)?.join("CC Desk"));
        }
        let mut shortcuts = vec![];
        let mut all_shortcuts = vec![];
        for id in [
            &FOLDERID_Desktop,
            &FOLDERID_Programs,
            &FOLDERID_PublicDesktop,
            &FOLDERID_CommonPrograms,
        ] {
            let path = known(id)?;
            Directory::open_absolute(&path)?;
            all_shortcuts.push(path.join("CC Desk.lnk"));
            for leaf in ["CC Desk.lnk", "CC-Box.lnk"] {
                absent_paths.push(path.join(leaf));
            }
            absent_paths.push(path.join("CC Desk"));
            if id == &FOLDERID_Desktop || id == &FOLDERID_Programs {
                shortcuts.push(path.join("CC Desk.lnk"));
            }
        }
        Ok(Self {
            absent_paths,
            shortcuts,
            all_shortcuts,
            _com: com,
        })
    }
    pub(super) fn require_absent(&self) -> io::Result<()> {
        for path in &self.absent_paths {
            require_absent(path)?;
        }
        for (slot, path) in SLOTS {
            for (view, flags) in VIEWS {
                if RegistrationKey::open(slot, view, "")?.is_some()
                    || open_key(HKEY_LOCAL_MACHINE, path, flags)?.is_some()
                {
                    return Err(blocked(
                        "pre-existing Desk registration is not fixture-owned",
                    ));
                }
            }
        }
        reject_wix()?;
        reject_running_desk()
    }
    pub(super) fn absence_report(&self) -> io::Result<Value> {
        self.require_absent()?;
        Ok(
            json!({"checkedAbsentPaths":self.absent_paths,"registrationSlots":SLOTS.iter().map(|(_,p)|p).collect::<Vec<_>>(),"views":[32,64],"hives":["HKCU","HKLM"],"matchingMachineWixRegistration":false,"matchingDeskProcessAtCheck":false,"scope":"initial absence only; process-name observation is not production quiescence authority"}),
        )
    }
    pub(super) fn seed(&self, install: &Path) -> io::Result<()> {
        self.require_absent()?;
        write_new(
            &install.join("source-only-leftover.txt"),
            b"fixture source-only leftover; /UPDATE must preserve this",
        )?;
        write_new(
            &install.join("Old Desk.exe"),
            b"fixture placeholder; never executable",
        )?;
        for path in [SLOTS[4].1, SLOTS[5].1] {
            set_fixture(path, "FixtureOwner", "CCDesk payload v0.17.7")?;
            set_fixture(
                &format!("{path}\\command"),
                "",
                "fixture legacy command, not executed",
            )?;
        }
        for path in [SLOTS[2].1, SLOTS[3].1] {
            set_fixture(path, "FixtureOwner", "CCDesk payload v0.17.7")?;
            set_fixture(path, "", "fixture old menu")?;
        }
        set_fixture(SLOTS[0].1, "MainBinaryName", "Old Desk.exe")?;
        set_fixture(SLOTS[0].1, "DisplayName", "CC Desk")?;
        set_fixture(SLOTS[0].1, "DisplayVersion", "0.18.0")?;
        set_fixture(SLOTS[0].1, "Publisher", "shawnwu2022")?;
        set_fixture(
            SLOTS[1].1,
            "",
            install
                .to_str()
                .ok_or_else(|| blocked("fixture path encoding"))?,
        )?;
        for path in &self.shortcuts {
            require_absent(path)?;
            let link: IShellLinkW =
                unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(win)? };
            let target = path_wide(&install.join("Old Desk.exe"));
            unsafe {
                link.SetPath(PCWSTR(target.as_ptr())).map_err(win)?;
            }
            let persistent: IPersistFile = link.cast().map_err(win)?;
            let p = path_wide(path);
            unsafe {
                persistent.Save(PCWSTR(p.as_ptr()), true).map_err(win)?;
            }
            if shortcut_target(path)? != install.join("Old Desk.exe") {
                return Err(blocked("fixture shortcut seed target differs"));
            }
        }
        Ok(())
    }
    pub(super) fn snapshot(&self, evidence: &Path, phase: &str) -> io::Result<Value> {
        let mut registry = BTreeMap::new();
        let mut registry_nodes = 0usize;
        for (slot, path) in SLOTS {
            for (view, flags) in VIEWS {
                // Production chain check rejects registry links in our known trees.
                let exists = RegistrationKey::open(slot, view, "")?.is_some();
                let value = if exists {
                    Some(snapshot_key(
                        open_key(HKEY_CURRENT_USER, path, flags)?
                            .ok_or_else(|| blocked("registration disappeared"))?,
                        0,
                        &mut registry_nodes,
                    )?)
                } else {
                    None
                };
                report(
                    evidence,
                    &format!("{phase}-registry-{}.json", registry.len()),
                    &value,
                )?;
                registry.insert(format!("HKCU/{view:?}/{path}"), value);
                if open_key(HKEY_LOCAL_MACHINE, path, flags)?.is_some() {
                    return Err(blocked(
                        "unexpected machine registration appeared; preserve evidence",
                    ));
                }
            }
        }
        let mut shortcuts = BTreeMap::new();
        for (index, path) in self.all_shortcuts.iter().enumerate() {
            let value = match fs_kind(path)? {
                None => Value::Null,
                Some(false) => {
                    let dir = Directory::open_absolute(path.parent().unwrap())?;
                    let held = dir.open_file(
                        crate::version_history::windows::files::ComponentName::new(
                            path.file_name().unwrap(),
                        )?,
                        FileAccess::Read,
                    )?;
                    let bytes = bounded_read(path, 64 * 1024)?;
                    let saved = format!("{phase}-shortcut-{index}.lnk");
                    write_new(&evidence.join(&saved), &bytes)?;
                    let target = shortcut_target(path)?;
                    held.verify()?;
                    json!({"sha256":sha256(&bytes),"size":bytes.len(),"target":target,"rawArtifact":saved,"securityDescriptor":file_security(path)?})
                }
                Some(true) => return Err(blocked("shortcut changed to directory")),
            };
            shortcuts.insert(path.clone(), value);
        }
        let mut paths = BTreeMap::new();
        for path in &self.absent_paths {
            paths.insert(path.clone(), fs_kind(path)?);
        }
        Ok(json!({"registry":registry,"shortcuts":shortcuts,"knownPaths":paths}))
    }
}
use std::fs;
fn fs_kind(path: &Path) -> io::Result<Option<bool>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_attributes() & 0x400 != 0 => {
            Err(blocked("reparse point in fixture effect location"))
        }
        Ok(m) if m.is_file() || m.is_dir() => Ok(Some(m.is_dir())),
        Ok(_) => Err(blocked("unsupported fixture object")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn require_absent(path: &Path) -> io::Result<()> {
    if fs_kind(path)?.is_some() {
        return Err(blocked("pre-existing Desk path is not fixture-owned"));
    }
    // Verify the nearest existing ancestor; no path redirection counts as isolation.
    let mut parent = path.parent().ok_or_else(|| blocked("missing parent"))?;
    while fs_kind(parent)?.is_none() {
        parent = parent
            .parent()
            .ok_or_else(|| blocked("missing existing parent"))?;
    }
    Directory::open_absolute(parent)?;
    Ok(())
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
fn open_key(hive: HKEY, path: &str, view: REG_SAM_FLAGS) -> io::Result<Option<Key>> {
    let p = wide(path);
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            hive,
            PCWSTR(p.as_ptr()),
            Some(REG_OPTION_OPEN_LINK.0),
            KEY_READ | view,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(win)?;
    Ok(Some(Key(key)))
}
fn key_values(key: &Key) -> io::Result<BTreeMap<String, Value>> {
    let mut values = BTreeMap::new();
    for index in 0..=32 {
        let mut name = [0u16; 1024];
        let mut n = name.len() as u32;
        let mut kind = 0;
        let mut bytes = vec![0u8; 4096];
        let mut size = bytes.len() as u32;
        let status = unsafe {
            RegEnumValueW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut n,
                None,
                Some(&mut kind),
                Some(bytes.as_mut_ptr()),
                Some(&mut size),
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(values);
        }
        status.ok().map_err(win)?;
        if index == 32 {
            return Err(blocked("registry value budget exceeded"));
        }
        let name = String::from_utf16(&name[..n as usize])
            .map_err(|_| blocked("unsupported registry name"))?;
        if name == "SymbolicLinkValue" {
            return Err(blocked("registry link observed"));
        }
        bytes.truncate(size as usize);
        values.insert(name, json!({"type":kind,"bytes":bytes}));
    }
    Err(blocked("incomplete registry values"))
}
fn subkeys(key: &Key, max: u32) -> io::Result<Vec<String>> {
    let mut names = vec![];
    for index in 0..=max {
        let mut text = [0u16; 256];
        let mut length = text.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(text.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            return Ok(names);
        }
        status.ok().map_err(win)?;
        if index == max {
            return Err(blocked("registry key budget exceeded"));
        }
        names.push(
            String::from_utf16(&text[..length as usize])
                .map_err(|_| blocked("unsupported registry key"))?,
        );
    }
    Err(blocked("incomplete registry subkeys"))
}
fn snapshot_key(key: Key, depth: usize, count: &mut usize) -> io::Result<Value> {
    *count += 1;
    if depth > 8 || *count > 64 {
        return Err(blocked("registry tree budget exceeded"));
    }
    let values = key_values(&key)?;
    let bytes = registry_security(&key)?;
    let mut children = BTreeMap::new();
    for child in subkeys(&key, 32)? {
        children.insert(
            child.clone(),
            snapshot_key(
                open_key(key.0, &child, REG_SAM_FLAGS(0))?
                    .ok_or_else(|| blocked("registry tree changed"))?,
                depth + 1,
                count,
            )?,
        );
    }
    Ok(json!({"values":values,"securityDescriptor":bytes,"children":children}))
}
fn value_text(key: &Key, name: &str) -> io::Result<Option<String>> {
    let p = wide(name);
    let mut kind = REG_VALUE_TYPE(0);
    let mut bytes = vec![0u8; 8192];
    let mut size = bytes.len() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(p.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(win)?;
    if kind != REG_SZ || size < 2 || !size.is_multiple_of(2) {
        return Err(blocked("unsupported registration text"));
    }
    let units: Vec<_> = bytes[..size as usize]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    if units.last() != Some(&0) {
        return Err(blocked("unterminated registration text"));
    }
    Ok(Some(
        String::from_utf16(&units[..units.len() - 1])
            .map_err(|_| blocked("invalid registration text"))?,
    ))
}
fn set_fixture(path: &str, name: &str, value: &str) -> io::Result<()> {
    // Fixed paths only, entered after all six slots were positively absent.
    if !SLOTS
        .iter()
        .any(|(_, p)| path == *p || path == format!("{p}\\command"))
    {
        return Err(blocked("seed outside fixed fixture registration"));
    }
    let p = wide(path);
    let n = wide(name);
    let text = wide(value);
    let bytes: Vec<_> = text.iter().flat_map(|u| u.to_le_bytes()).collect();
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(p.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE | KEY_WOW64_64KEY,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(win)?;
    }
    let key = Key(key);
    unsafe {
        RegSetValueExW(key.0, PCWSTR(n.as_ptr()), None, REG_SZ, Some(&bytes))
            .ok()
            .map_err(win)?;
        RegFlushKey(key.0).ok().map_err(win)?;
    }
    if value_text(&key, name)?.as_deref() != Some(value) {
        return Err(blocked("seed value differs"));
    }
    Ok(())
}
fn reject_wix() -> io::Result<()> {
    for (_, view) in VIEWS {
        if let Some(root) = open_key(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
            view,
        )? {
            for child in subkeys(&root, 4096)? {
                if let Some(key) = open_key(root.0, &child, view)? {
                    if machine_desk_identity(&key)? {
                        return Err(blocked("machine Desk/WiX registration exists"));
                    }
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IdentityMatch {
    Exact,
    Different,
    Unknown,
}

// RegQueryValueEx does not guarantee string termination. Decode bounded bytes
// without dereferencing a native string or expanding ambient environment data.
fn identity_text(kind: REG_VALUE_TYPE, bytes: &[u8], expected: &str) -> IdentityMatch {
    if (kind != REG_SZ && kind != REG_EXPAND_SZ)
        || bytes.len() > 8192
        || !bytes.len().is_multiple_of(2)
    {
        return IdentityMatch::Unknown;
    }
    let mut units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|value| u16::from_le_bytes(*value))
        .collect();
    if units.last() == Some(&0) {
        units.pop();
    }
    if units.contains(&0) {
        return IdentityMatch::Unknown;
    }
    let Ok(value) = String::from_utf16(&units) else {
        return IdentityMatch::Unknown;
    };
    if kind == REG_EXPAND_SZ && value.contains('%') {
        return IdentityMatch::Unknown;
    }
    if value.eq_ignore_ascii_case(expected) {
        IdentityMatch::Exact
    } else {
        IdentityMatch::Different
    }
}

fn identity_field(key: &Key, name: &str, expected: &str) -> io::Result<IdentityMatch> {
    let name = wide(name);
    let mut kind = REG_VALUE_TYPE(0);
    let mut bytes = [0u8; 8192];
    let mut length = bytes.len() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(IdentityMatch::Different);
    }
    if status == windows::Win32::Foundation::ERROR_MORE_DATA {
        return Ok(IdentityMatch::Unknown);
    }
    status.ok().map_err(win)?;
    if length as usize > bytes.len() {
        return Ok(IdentityMatch::Unknown);
    }
    Ok(identity_text(kind, &bytes[..length as usize], expected))
}

fn identity_pair(name: IdentityMatch, publisher: IdentityMatch) -> io::Result<bool> {
    // One positively different field disproves the same exact product/publisher
    // pair used by the previous gate. Unknown data alone never proves absence.
    if name == IdentityMatch::Different || publisher == IdentityMatch::Different {
        return Ok(false);
    }
    if name == IdentityMatch::Exact && publisher == IdentityMatch::Exact {
        return Ok(true);
    }
    Err(blocked("machine Desk identity fields are ambiguous"))
}

fn machine_desk_identity(key: &Key) -> io::Result<bool> {
    identity_pair(
        identity_field(key, "DisplayName", "CC Desk")?,
        identity_field(key, "Publisher", "shawnwu2022")?,
    )
}

// 非目标记录可由另一已验证字段排除；未知字段绝不能隐藏可能匹配的Desk安装。
#[test]
fn HistoryPayload_MachineIdentity_008() {
    use IdentityMatch::{Different, Exact, Unknown};
    let bytes = |value: &str| {
        value
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    };
    for kind in [REG_SZ, REG_EXPAND_SZ] {
        for text in ["CC Desk", "CC Desk\0", "cc desk"] {
            assert_eq!(identity_text(kind, &bytes(text), "CC Desk"), Exact);
        }
        for text in ["", "\0", "Other product"] {
            assert_eq!(identity_text(kind, &bytes(text), "CC Desk"), Different);
        }
    }
    assert_eq!(
        identity_text(REG_EXPAND_SZ, &bytes("%PRODUCT%"), "CC Desk"),
        Unknown
    );
    assert_eq!(
        identity_text(REG_SZ, &bytes("CC Desk\0other"), "CC Desk"),
        Unknown
    );
    assert_eq!(
        identity_text(REG_BINARY, &bytes("CC Desk"), "CC Desk"),
        Unknown
    );
    assert_eq!(identity_text(REG_SZ, &[0], "CC Desk"), Unknown);
    assert_eq!(identity_text(REG_SZ, &[0, 0xd8], "CC Desk"), Unknown);
    assert_eq!(identity_text(REG_SZ, &[0; 8194], "CC Desk"), Unknown);
    for first in [Exact, Different, Unknown] {
        for second in [Exact, Different, Unknown] {
            let result = identity_pair(first, second);
            if first == Different || second == Different {
                assert!(!result.unwrap());
            } else if first == Exact && second == Exact {
                assert!(result.unwrap());
            } else {
                assert!(result.is_err());
            }
        }
    }
}
const PROCESS_SNAPSHOT_BUDGET: usize = 16384;

fn process_snapshot_next(result: windows_core::Result<()>) -> io::Result<bool> {
    match result {
        Ok(()) => Ok(true),
        // ToolHelp process enumeration uses FILES, unlike registry enumeration.
        Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => Ok(false),
        Err(e) => Err(win(e)),
    }
}

fn visit_process_snapshot(
    budget: usize,
    mut visit: impl FnMut(&PROCESSENTRY32W) -> io::Result<()>,
) -> io::Result<usize> {
    if budget > PROCESS_SNAPSHOT_BUDGET {
        return Err(blocked("process inventory exceeded budget"));
    }
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(win)? };
    let _owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    // An empty/malformed initial snapshot does not establish fixture absence.
    unsafe {
        Process32FirstW(handle, &mut entry).map_err(win)?;
    }
    for count in 1..=budget {
        visit(&entry)?;
        if !process_snapshot_next(unsafe { Process32NextW(handle, &mut entry) })? {
            return Ok(count);
        }
    }
    Err(blocked("process inventory exceeded budget"))
}

fn reject_running_desk() -> io::Result<()> {
    visit_process_snapshot(PROCESS_SNAPSHOT_BUDGET, |entry| {
        let end = entry
            .szExeFile
            .iter()
            .position(|v| *v == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
        if ["cc-desk.exe", "cc desk.exe", "cc-box.exe", "cc box.exe"]
            .iter()
            .any(|p| name.eq_ignore_ascii_case(p))
        {
            return Err(blocked("Desk process exists before fixture installer"));
        }
        Ok(())
    })?;
    Ok(())
}

// 真实只读快照必须遍历至原生结束码，包含当前进程，且不保存其他进程信息。
#[test]
fn HistoryPayload_ProcessSnapshot_009() {
    let mut current_seen = false;
    let count = visit_process_snapshot(PROCESS_SNAPSHOT_BUDGET, |entry| {
        current_seen |= entry.th32ProcessID == std::process::id();
        Ok(())
    })
    .expect("bounded ToolHelp snapshot must reach its documented terminal status");
    assert!(current_seen);
    assert!((1..=PROCESS_SNAPSHOT_BUDGET).contains(&count));
}

// 非结束错误、条目预算耗尽和回调拒绝均不能伪装成成功或生产静默证明。
#[test]
fn HistoryPayload_ProcessSnapshotGuards_010() {
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_HANDLE};
    assert!(process_snapshot_next(Ok(())).unwrap());
    assert!(
        !process_snapshot_next(Err(windows_core::Error::from_hresult(
            ERROR_NO_MORE_FILES.to_hresult()
        )))
        .unwrap()
    );
    for code in [
        ERROR_NO_MORE_ITEMS,
        ERROR_ACCESS_DENIED,
        ERROR_INVALID_HANDLE,
    ] {
        let error =
            process_snapshot_next(Err(windows_core::Error::from_hresult(code.to_hresult())))
                .unwrap_err();
        assert_eq!(
            error
                .get_ref()
                .unwrap()
                .downcast_ref::<windows_core::Error>()
                .unwrap()
                .code(),
            code.to_hresult()
        );
    }
    assert!(visit_process_snapshot(0, |_| panic!("zero budget visited an entry")).is_err());
    assert!(visit_process_snapshot(PROCESS_SNAPSHOT_BUDGET + 1, |_| {
        panic!("over-budget snapshot visited an entry")
    })
    .is_err());
    assert_eq!(
        visit_process_snapshot(PROCESS_SNAPSHOT_BUDGET, |_| Err(blocked("probe refusal")))
            .unwrap_err()
            .to_string(),
        "probe refusal"
    );
}
pub(super) fn existing_webview() -> io::Result<Value> {
    crate::version_history::windows::webview::reject_manager_overrides()?;
    let mut browser_version = PWSTR::null();
    unsafe {
        webview2_com::Microsoft::Web::WebView2::Win32::GetAvailableCoreWebView2BrowserVersionString(PCWSTR::null(), &mut browser_version).map_err(win)?;
    }
    let actual = unsafe { browser_version.to_string() }.map_err(io::Error::other);
    unsafe {
        CoTaskMemFree(Some(browser_version.0.cast()));
    }
    let actual = actual?;
    if actual.is_empty() || actual.len() > 128 {
        return Err(blocked(
            "WebView2 loader could not resolve an existing runtime",
        ));
    }
    let key = r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    for (hive, label, view) in [
        (HKEY_LOCAL_MACHINE, "HKLM", KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, "HKCU", KEY_WOW64_64KEY),
    ] {
        if let Some(key) = open_key(hive, key, view)? {
            if let Some(version) = value_text(&key, "pv")? {
                if version.split('.').count() == 4
                    && version
                        .split('.')
                        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                    && version != "0.0.0.0"
                {
                    return Ok(
                        json!({"hive":label,"version":version,"loaderResolvedVersion":actual,"alreadyRegistered":true,"prerequisiteInstallAllowed":false,"observation":"existing runtime registration; no runtime launched"}),
                    );
                }
            }
        }
    }
    Err(blocked(
        "existing WebView2 runtime registration required; no bootstrap/elevation fallback",
    ))
}
fn shortcut_target(path: &Path) -> io::Result<PathBuf> {
    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(win)? };
    let persistent: IPersistFile = link.cast().map_err(win)?;
    let p = path_wide(path);
    unsafe {
        persistent
            .Load(PCWSTR(p.as_ptr()), STGM_READ)
            .map_err(win)?;
    }
    let mut text = [0u16; 32768];
    let mut data = WIN32_FIND_DATAW::default();
    unsafe {
        link.GetPath(&mut text, &mut data, SLGP_RAWPATH.0 as u32)
            .map_err(win)?;
    }
    let end = text
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| blocked("shortcut path exceeds budget"))?;
    Ok(PathBuf::from(OsString::from_wide(&text[..end])))
}
const MAX_DESCRIPTOR_BYTES: usize = 65536;
const RELATIVE_DESCRIPTOR_HEADER: usize = 20;

// Bound every referenced SID/ACL before calling APIs without a length argument.
// This is a memory-extent check; native descriptor validation remains authoritative.
fn descriptor_extent(bytes: &[u8]) -> io::Result<usize> {
    if !(RELATIVE_DESCRIPTOR_HEADER..=MAX_DESCRIPTOR_BYTES).contains(&bytes.len())
        || bytes[0] != 1
        || u16::from_le_bytes([bytes[2], bytes[3]]) & SE_SELF_RELATIVE.0 == 0
    {
        return Err(blocked("unsupported captured descriptor header"));
    }
    let mut extent = RELATIVE_DESCRIPTOR_HEADER;
    for (field, acl) in [(4, false), (8, false), (12, true), (16, true)] {
        let offset = u32::from_le_bytes(bytes[field..field + 4].try_into().unwrap()) as usize;
        if offset == 0 {
            continue;
        }
        if offset < RELATIVE_DESCRIPTOR_HEADER
            || !offset.is_multiple_of(4)
            || offset.checked_add(8).is_none_or(|end| end > bytes.len())
        {
            return Err(blocked("captured descriptor offset exceeds returned bytes"));
        }
        let length = if acl {
            u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize
        } else {
            if bytes[offset + 1] > 15 {
                return Err(blocked(
                    "captured descriptor SID exceeds subauthority bound",
                ));
            }
            8 + bytes[offset + 1] as usize * 4
        };
        let end = offset
            .checked_add(length)
            .filter(|end| length >= 8 && *end <= bytes.len())
            .ok_or_else(|| blocked("captured descriptor component exceeds returned bytes"))?;
        extent = extent.max(end);
    }
    Ok(extent)
}

struct DescriptorCapture {
    words: Vec<u32>,
}
impl DescriptorCapture {
    fn new() -> Self {
        Self {
            words: vec![0u32; MAX_DESCRIPTOR_BYTES / size_of::<u32>()],
        }
    }
    fn pointer(&mut self) -> PSECURITY_DESCRIPTOR {
        PSECURITY_DESCRIPTOR(self.words.as_mut_ptr().cast())
    }
    fn finish(mut self, returned: u32) -> io::Result<Vec<u8>> {
        let returned = returned as usize;
        if !(RELATIVE_DESCRIPTOR_HEADER..=MAX_DESCRIPTOR_BYTES).contains(&returned) {
            return Err(blocked("captured descriptor returned size exceeds budget"));
        }
        let extent = descriptor_extent(unsafe {
            std::slice::from_raw_parts(self.words.as_ptr().cast::<u8>(), returned)
        })?;
        let descriptor = self.pointer();
        if !unsafe { IsValidSecurityDescriptor(descriptor) }.as_bool() {
            return Err(blocked("captured security descriptor is invalid"));
        }
        let mut control = 0;
        let mut revision = 0;
        unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) }
            .map_err(win)?;
        if revision != 1 || control & SE_SELF_RELATIVE.0 == 0 {
            return Err(blocked(
                "captured descriptor is not self-relative revision one",
            ));
        }
        let length = unsafe { GetSecurityDescriptorLength(descriptor) } as usize;
        if length < extent || length > returned {
            return Err(blocked(
                "native descriptor length does not contain bounded components",
            ));
        }
        // Preserve every descriptor byte, including trailing zeroes within its
        // validated length. Allocation capacity is never evidence or a trim rule.
        Ok(unsafe { std::slice::from_raw_parts(descriptor.0.cast::<u8>(), length) }.to_vec())
    }
}

fn registry_security(key: &Key) -> io::Result<Vec<u8>> {
    let mut capture = DescriptorCapture::new();
    let mut returned = MAX_DESCRIPTOR_BYTES as u32;
    unsafe {
        RegGetKeySecurity(
            key.0,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(capture.pointer()),
            &mut returned,
        )
        .ok()
        .map_err(win)?;
    }
    capture.finish(returned)
}

fn file_security(path: &Path) -> io::Result<Vec<u8>> {
    let p = path_wide(path);
    let mut capture = DescriptorCapture::new();
    let mut needed = 0;
    let ok = unsafe {
        GetFileSecurityW(
            PCWSTR(p.as_ptr()),
            (OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0,
            Some(capture.pointer()),
            MAX_DESCRIPTOR_BYTES as u32,
            &mut needed,
        )
    };
    if !ok.as_bool() {
        return Err(io::Error::last_os_error());
    }
    capture.finish(needed)
}

// 本机转换器给出的实际描述符字节（含末尾零）必须完整保留，容量填充值不属于证据。
#[test]
fn HistoryPayload_DescriptorLength_011() {
    use windows::Win32::{
        Foundation::{LocalFree, HLOCAL},
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
    };
    let text = wide("O:SYG:SYD:(A;;GR;;;SY)");
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    let mut length = 0;
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(text.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            Some(&mut length),
        )
        .unwrap();
    }
    struct Allocation(PSECURITY_DESCRIPTOR);
    impl Drop for Allocation {
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }
    let allocation = Allocation(descriptor);
    assert!((20..=MAX_DESCRIPTOR_BYTES as u32).contains(&length));
    let expected =
        unsafe { std::slice::from_raw_parts(allocation.0 .0.cast::<u8>(), length as usize) };
    assert_eq!(expected.last(), Some(&0));
    for fill in [0u32, 0x7e7e7e7e] {
        for returned in [length, MAX_DESCRIPTOR_BYTES as u32] {
            let mut capture = DescriptorCapture::new();
            capture.words.fill(fill);
            unsafe {
                std::ptr::copy_nonoverlapping(
                    expected.as_ptr(),
                    capture.words.as_mut_ptr().cast(),
                    expected.len(),
                );
            }
            assert_eq!(capture.finish(returned).unwrap(), expected);
        }
    }
}

// 非相对格式、越界偏移、截断 SID/ACL 与错误长度都在原生指针读取前被拒绝。
#[test]
fn HistoryPayload_DescriptorBounds_012() {
    let mut header = vec![0u8; 20];
    header[0] = 1;
    header[2..4].copy_from_slice(&SE_SELF_RELATIVE.0.to_le_bytes());
    assert_eq!(descriptor_extent(&header).unwrap(), 20);
    assert!(descriptor_extent(&header[..19]).is_err());
    let mut bad = header.clone();
    bad[2..4].fill(0);
    assert!(descriptor_extent(&bad).is_err());
    for offset in [1u32, 19, 21, u32::MAX] {
        let mut bad = header.clone();
        bad[4..8].copy_from_slice(&offset.to_le_bytes());
        assert!(descriptor_extent(&bad).is_err());
    }
    let mut sid = header.clone();
    sid.resize(28, 0);
    sid[4..8].copy_from_slice(&20u32.to_le_bytes());
    sid[20] = 1;
    for count in [1, 16] {
        sid[21] = count;
        assert!(descriptor_extent(&sid).is_err());
    }
    let mut acl = header;
    acl.resize(28, 0);
    acl[16..20].copy_from_slice(&20u32.to_le_bytes());
    for length in [0u16, 7, 9, u16::MAX] {
        acl[22..24].copy_from_slice(&length.to_le_bytes());
        assert!(descriptor_extent(&acl).is_err());
    }
    for returned in [0, 19, MAX_DESCRIPTOR_BYTES as u32 + 1] {
        assert!(DescriptorCapture::new().finish(returned).is_err());
    }
}

// 真实注册表和文件只读 API 均经过同一有界提取器；不导出环境描述符或改变权限。
#[test]
fn HistoryPayload_DescriptorNative_013() {
    let key = open_key(HKEY_CURRENT_USER, "Software", KEY_WOW64_64KEY)
        .unwrap()
        .expect("current-user Software key");
    let registry = registry_security(&key).unwrap();
    assert!(descriptor_extent(&registry).unwrap() <= registry.len());
    let file = file_security(&std::env::current_exe().unwrap()).unwrap();
    assert!(descriptor_extent(&file).unwrap() <= file.len());
}

fn reject_streams(path: &Path) -> io::Result<()> {
    let p = path_wide(path);
    let mut data = WIN32_FIND_STREAM_DATA::default();
    let handle = match unsafe {
        FindFirstStreamW(
            PCWSTR(p.as_ptr()),
            FindStreamInfoStandard,
            (&mut data as *mut WIN32_FIND_STREAM_DATA).cast(),
            None,
        )
    } {
        Ok(h) => h,
        Err(e) if e.code() == ERROR_HANDLE_EOF.to_hresult() => return Ok(()),
        Err(e) => return Err(win(e)),
    };
    struct Find(HANDLE);
    impl Drop for Find {
        fn drop(&mut self) {
            unsafe {
                let _ = FindClose(self.0);
            }
        }
    }
    let _find = Find(handle);
    loop {
        let end = data
            .cStreamName
            .iter()
            .position(|v| *v == 0)
            .ok_or_else(|| blocked("stream name exceeds budget"))?;
        if String::from_utf16_lossy(&data.cStreamName[..end]) != "::$DATA" {
            return Err(blocked("alternate data stream in installed payload"));
        }
        match unsafe { FindNextStreamW(handle, (&mut data as *mut WIN32_FIND_STREAM_DATA).cast()) }
        {
            Ok(()) => (),
            Err(e) if e.code() == ERROR_HANDLE_EOF.to_hresult() => return Ok(()),
            Err(e) => return Err(win(e)),
        }
    }
}
#[derive(Serialize)]
pub(super) struct Entry {
    path: String,
    directory: bool,
    size: u64,
    sha256: Option<String>,
    attributes: u32,
    identity: crate::version_history::windows::files::FileIdentity,
    security_descriptor: Vec<u8>,
}
pub(super) fn capture_tree(root: Arc<Directory>) -> io::Result<Vec<Entry>> {
    fn walk(
        dir: Arc<Directory>,
        relative: &str,
        entries: &mut Vec<Entry>,
        held_dirs: &mut Vec<Arc<Directory>>,
        held_files: &mut Vec<crate::version_history::windows::files::PinnedFile>,
        total: &mut u64,
    ) -> io::Result<()> {
        if relative.split('/').count() > 16 || entries.len() >= 256 {
            return Err(blocked("installed inventory exceeds depth/entry budget"));
        }
        let path = PathBuf::from(dir.path()?);
        let m = fs::symlink_metadata(&path)?;
        reject_streams(&path)?;
        entries.push(Entry {
            path: relative.into(),
            directory: true,
            size: 0,
            sha256: None,
            attributes: m.file_attributes(),
            identity: dir.identity().clone(),
            security_descriptor: file_security(&path)?,
        });
        let children = dir.read_children(256)?;
        let mut names = Vec::<String>::new();
        for child in children {
            let text = child
                .os_string()
                .into_string()
                .map_err(|_| blocked("unrepresentable installed filename"))?;
            // Windows ordinal ignore-case equality, not an ASCII-only alias check.
            let units: Vec<_> = text.encode_utf16().collect();
            for prior in &names {
                let prior: Vec<_> = prior.encode_utf16().collect();
                if unsafe {
                    windows::Win32::Globalization::CompareStringOrdinal(&units, &prior, true)
                } == windows::Win32::Globalization::CSTR_EQUAL
                {
                    return Err(blocked("alternate-path collision"));
                }
            }
            names.push(text.clone());
            let next = if relative.is_empty() {
                text.clone()
            } else {
                format!("{relative}/{text}")
            };
            let actual = path.join(&text);
            let metadata = fs::symlink_metadata(&actual)?;
            if metadata.is_dir() {
                walk(
                    dir.open_directory(child)?,
                    &next,
                    entries,
                    held_dirs,
                    held_files,
                    total,
                )?;
            } else {
                if !metadata.is_file() || entries.len() >= 256 || metadata.len() > 256 * 1024 * 1024
                {
                    return Err(blocked("unsupported installed file or inventory budget"));
                }
                *total = total
                    .checked_add(metadata.len())
                    .ok_or_else(|| blocked("inventory size overflow"))?;
                if *total > 512 * 1024 * 1024 {
                    return Err(blocked("installed byte budget exceeded"));
                }
                let file = dir.open_file(child, FileAccess::Read)?;
                reject_streams(&actual)?;
                entries.push(Entry {
                    path: next,
                    directory: false,
                    size: metadata.len(),
                    sha256: Some(file.digest()?),
                    attributes: metadata.file_attributes(),
                    identity: file.identity().clone(),
                    security_descriptor: file_security(&actual)?,
                });
                held_files.push(file);
            }
        }
        held_dirs.push(dir);
        Ok(())
    }
    let mut entries = vec![];
    let mut dirs = vec![];
    let mut files = vec![];
    walk(root, "", &mut entries, &mut dirs, &mut files, &mut 0)?;
    for d in &dirs {
        d.recheck()?;
    }
    for f in &files {
        f.verify()?;
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
pub(super) fn pe_identity(path: &Path) -> io::Result<Value> {
    let parent = Directory::open_absolute(path.parent().unwrap())?;
    let file = parent.open_file(name("cc-desk.exe")?, FileAccess::Read)?;
    let bytes = bounded_read(path, 256 * 1024 * 1024)?;
    if bytes.get(..2) != Some(b"MZ") || bytes.len() < 64 {
        return Err(blocked("installed image is not PE"));
    }
    let at = u32::from_le_bytes(bytes[60..64].try_into().unwrap()) as usize;
    if at > bytes.len().saturating_sub(26) || bytes.get(at..at + 4) != Some(b"PE\0\0") {
        return Err(blocked("invalid installed PE header"));
    }
    let machine = u16::from_le_bytes(bytes[at + 4..at + 6].try_into().unwrap());
    let magic = u16::from_le_bytes(bytes[at + 24..at + 26].try_into().unwrap());
    let p = path_wide(path);
    let length = unsafe { GetFileVersionInfoSizeW(PCWSTR(p.as_ptr()), None) };
    if length == 0 || length > 1024 * 1024 {
        return Err(blocked("installed version resource missing or over budget"));
    }
    let mut resource = vec![0u32; (length as usize).div_ceil(4)];
    unsafe {
        GetFileVersionInfoW(
            PCWSTR(p.as_ptr()),
            None,
            length,
            resource.as_mut_ptr().cast(),
        )
        .map_err(win)?;
    }
    fn query(resource: &[u32], key: &str) -> io::Result<(*mut core::ffi::c_void, u32)> {
        let key = wide(key);
        let mut data = std::ptr::null_mut();
        let mut size = 0;
        let ok = unsafe {
            VerQueryValueW(
                resource.as_ptr().cast(),
                PCWSTR(key.as_ptr()),
                &mut data,
                &mut size,
            )
        };
        if !ok.as_bool() || data.is_null() {
            return Err(blocked("required version resource field missing"));
        }
        Ok((data, size))
    }
    let (fixed, size) = query(&resource, "\\")?;
    if size < size_of::<VS_FIXEDFILEINFO>() as u32 {
        return Err(blocked("invalid fixed version resource"));
    }
    let fixed = unsafe { &*fixed.cast::<VS_FIXEDFILEINFO>() };
    let (translations, size) = query(&resource, "\\VarFileInfo\\Translation")?;
    if size == 0 || size > 64 || !size.is_multiple_of(4) {
        return Err(blocked("unsupported version translation table"));
    }
    let translations =
        unsafe { std::slice::from_raw_parts(translations.cast::<u16>(), size as usize / 2) };
    let mut strings = BTreeMap::new();
    for pair in translations.as_chunks::<2>().0 {
        for field in ["ProductName", "ProductVersion", "FileVersion"] {
            let key = format!("\\StringFileInfo\\{:04x}{:04x}\\{field}", pair[0], pair[1]);
            let (text, size) = query(&resource, &key)?;
            if size == 0 || size > 1024 {
                return Err(blocked("version string exceeds budget"));
            }
            let text = unsafe { std::slice::from_raw_parts(text.cast::<u16>(), size as usize) };
            let end = text
                .iter()
                .position(|v| *v == 0)
                .ok_or_else(|| blocked("unterminated version string"))?;
            strings.insert(
                key,
                String::from_utf16(&text[..end]).map_err(|_| blocked("invalid version string"))?,
            );
        }
    }
    file.verify()?;
    Ok(
        json!({"machine":machine,"optionalHeaderMagic":magic,"fixedProductVersion":[fixed.dwProductVersionMS>>16,fixed.dwProductVersionMS&65535,fixed.dwProductVersionLS>>16,fixed.dwProductVersionLS&65535],"strings":strings,"fileSha256":file.digest()?}),
    )
}
const CONPTY_RESOURCES: [(&str, u64, &str); 3] = [
    (
        "conpty.dll",
        109920,
        "39fba2713e2495117b1591ae8c32a3b904bea7aa66069cf7815e2844c76d75d8",
    ),
    (
        "OpenConsole.exe",
        1066296,
        "b7fd936c2668b87b9ecf7b3366dc6568afc1c6f981874cba3e955a1c35cf8160",
    ),
    (
        "LICENSE-Microsoft-ConPTY.txt",
        1116,
        "5d177f23ecfeb0ea8e050b6a5a16355e1ae9a0b286436ca8f83ed08b3795be6b",
    ),
];

type ObservedEntry<'a> = (&'a str, bool, u64, Option<&'a str>);
fn check_payload_shape(
    fixture: &super::fixture::Fixture,
    case: &str,
    entries: &[ObservedEntry<'_>],
    pe: &Value,
) -> io::Result<()> {
    if !matches!(case, "clean" | "seeded-existing") {
        return Err(blocked("unknown fixture case"));
    }
    let expected_version: Vec<_> = fixture
        .version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap())
        .chain([0])
        .collect();
    if pe["machine"] != json!(0x8664)
        || pe["optionalHeaderMagic"] != json!(0x20b)
        || pe["fixedProductVersion"] != json!(expected_version)
    {
        return Err(blocked(
            "measured installed PE architecture or version differs",
        ));
    }
    let strings = pe["strings"]
        .as_object()
        .ok_or_else(|| blocked("missing PE resource strings"))?;
    if strings.iter().any(|(key, value)| {
        if key.ends_with("ProductName") {
            value != "CC Desk"
        } else {
            value != fixture.version.as_str() && value != &json!(format!("{}.0", fixture.version))
        }
    }) {
        return Err(blocked("measured PE product/version strings differ"));
    }
    let mut expected = vec!["", "cc-desk.exe", "uninstall.exe"];
    if fixture.has_conpty() {
        for (path, size, hash) in CONPTY_RESOURCES {
            if !entries
                .iter()
                .any(|e| e.0 == path && !e.1 && e.2 == size && e.3 == Some(hash))
            {
                return Err(blocked(
                    "installed source-declared resource differs; retain measured inventory",
                ));
            }
            expected.push(path);
        }
    }
    if case == "seeded-existing" {
        expected.push("source-only-leftover.txt");
    }
    expected.sort_unstable();
    let actual: Vec<_> = entries.iter().map(|entry| entry.0).collect();
    if actual != expected || entries.iter().any(|entry| entry.1 != entry.0.is_empty()) {
        return Err(blocked(
            "unexplained installed file/directory; preserve inventory for review",
        ));
    }
    Ok(())
}

pub(super) fn check_observations(
    fixture: &super::fixture::Fixture,
    case: &str,
    tree: &[Entry],
    pe: &Value,
    scope: &Scope,
    install: &Path,
) -> io::Result<()> {
    let entries: Vec<_> = tree
        .iter()
        .map(|entry| {
            (
                entry.path.as_str(),
                entry.directory,
                entry.size,
                entry.sha256.as_deref(),
            )
        })
        .collect();
    check_payload_shape(fixture, case, &entries, pe)?;
    for (slot, _) in [SLOTS[4], SLOTS[5]] {
        for (view, _) in VIEWS {
            if RegistrationKey::open(slot, view, "")?.is_some() {
                return Err(blocked(
                    "legacy Explorer key was not deleted as observed source declares",
                ));
            }
        }
    }
    for shortcut in &scope.shortcuts {
        if case == "seeded-existing" {
            if shortcut_target(shortcut)? != install.join("cc-desk.exe") {
                return Err(blocked("existing shortcut migration differs despite /NS"));
            }
        } else {
            require_absent(shortcut)?;
        }
    }
    for path in &scope.absent_paths {
        if !scope.shortcuts.contains(path) {
            require_absent(path)?;
        }
    }
    if case == "seeded-existing"
        && bounded_read(&install.join("source-only-leftover.txt"), 128)?
            != b"fixture source-only leftover; /UPDATE must preserve this"
    {
        return Err(blocked("/UPDATE source-only leftover changed"));
    }
    Ok(())
}

pub(super) fn export_tree(root: &Path, evidence: &Path, entries: &[Entry]) -> io::Result<()> {
    let output = evidence.join("captured-payload");
    fs::create_dir(&output)?;
    for entry in entries {
        if entry.path.is_empty() {
            continue;
        }
        let target = output.join(&entry.path);
        if entry.directory {
            fs::create_dir(&target)?;
            continue;
        }
        let source = root.join(&entry.path);
        let parent = Directory::open_absolute(
            source
                .parent()
                .ok_or_else(|| blocked("missing capture parent"))?,
        )?;
        let held = parent.open_file(
            crate::version_history::windows::files::ComponentName::new(
                source.file_name().unwrap(),
            )?,
            FileAccess::Read,
        )?;
        let bytes = bounded_read(&source, entry.size)?;
        if bytes.len() as u64 != entry.size
            || Some(sha256(&bytes)) != entry.sha256
            || held.identity() != &entry.identity
        {
            return Err(blocked("payload changed before evidence copy"));
        }
        write_new(&target, &bytes)?;
        held.verify()?;
    }
    Ok(())
}

// 固定版本和 case 只能接受完整精确目录，不能把旧版本套用 ConPTY 白名单。
#[test]
fn HistoryPayload_InventoryMatrix_024() {
    for version in [
        "0.14.0", "0.15.0", "0.16.0", "0.17.0", "0.17.1", "0.17.2", "0.17.5", "0.17.6", "0.17.7",
    ] {
        let fixture = super::fixture::load(version).unwrap();
        let parts: Vec<_> = version
            .split('.')
            .map(|p| p.parse::<u32>().unwrap())
            .chain([0])
            .collect();
        let pe = json!({"machine":0x8664,"optionalHeaderMagic":0x20b,"fixedProductVersion":parts,
            "strings":{"ProductName":"CC Desk","FileVersion":version,"ProductVersion":format!("{version}.0")}});
        for case in ["clean", "seeded-existing"] {
            let mut entries = vec![
                ("", true, 0, None),
                ("cc-desk.exe", false, 123, Some("measured")),
                ("uninstall.exe", false, 456, Some("measured")),
            ];
            if fixture.has_conpty() {
                entries.extend(
                    CONPTY_RESOURCES
                        .iter()
                        .map(|(p, s, h)| (*p, false, *s, Some(*h))),
                );
            }
            if case == "seeded-existing" {
                entries.push(("source-only-leftover.txt", false, 53, Some("measured")));
            }
            entries.sort_by_key(|entry| entry.0);
            check_payload_shape(&fixture, case, &entries, &pe).unwrap();
            let mut missing = entries.clone();
            missing.pop();
            assert!(check_payload_shape(&fixture, case, &missing, &pe).is_err());
            let mut extra = entries.clone();
            extra.push(("Old Desk.exe", false, 3, Some("measured")));
            assert!(check_payload_shape(&fixture, case, &extra, &pe).is_err());
            let mut wrong_kind = entries.clone();
            wrong_kind[1].1 = true;
            assert!(check_payload_shape(&fixture, case, &wrong_kind, &pe).is_err());
            let mut wrong_pe = pe.clone();
            wrong_pe["fixedProductVersion"] = json!([0, 18, 0, 0]);
            assert!(check_payload_shape(&fixture, case, &entries, &wrong_pe).is_err());
            wrong_pe = pe.clone();
            wrong_pe["machine"] = json!(0x14c);
            assert!(check_payload_shape(&fixture, case, &entries, &wrong_pe).is_err());
            wrong_pe = pe.clone();
            wrong_pe["strings"]["FileVersion"] = json!("0.18.0");
            assert!(check_payload_shape(&fixture, case, &entries, &wrong_pe).is_err());
            assert!(check_payload_shape(&fixture, "unknown", &entries, &pe).is_err());
            if !fixture.has_conpty() {
                let mut unexpected_resource = entries.clone();
                unexpected_resource.push(("conpty.dll", false, 109920, Some("measured")));
                unexpected_resource.sort_by_key(|entry| entry.0);
                assert!(check_payload_shape(&fixture, case, &unexpected_resource, &pe).is_err());
            }
            let other = super::fixture::load(if version == "0.17.7" {
                "0.17.6"
            } else {
                "0.17.7"
            })
            .unwrap();
            assert!(check_payload_shape(&other, case, &entries, &pe).is_err());
            if fixture.has_conpty() {
                let mut changed = entries.clone();
                changed.iter_mut().find(|e| e.0 == "conpty.dll").unwrap().3 = Some("wrong");
                assert!(check_payload_shape(&fixture, case, &changed, &pe).is_err());
            }
        }
    }
}
