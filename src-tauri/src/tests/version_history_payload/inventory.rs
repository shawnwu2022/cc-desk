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
        ERROR_FILE_NOT_FOUND, ERROR_HANDLE_EOF, ERROR_NO_MORE_ITEMS, ERROR_PATH_NOT_FOUND, HANDLE,
    },
    Security::{
        GetFileSecurityW, DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
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
    let mut bytes = vec![0u8; 65536];
    let mut size = bytes.len() as u32;
    unsafe {
        RegGetKeySecurity(
            key.0,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            Some(PSECURITY_DESCRIPTOR(bytes.as_mut_ptr().cast())),
            &mut size,
        )
        .ok()
        .map_err(win)?;
    }
    bytes.truncate(size as usize);
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
    if kind != REG_SZ || size < 2 || size % 2 != 0 {
        return Err(blocked("unsupported registration text"));
    }
    let units: Vec<_> = bytes[..size as usize]
        .chunks_exact(2)
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
                    if value_text(&key, "DisplayName")?.as_deref() == Some("CC Desk")
                        && value_text(&key, "Publisher")?.as_deref() == Some("shawnwu2022")
                    {
                        return Err(blocked("machine Desk/WiX registration exists"));
                    }
                }
            }
        }
    }
    Ok(())
}
fn reject_running_desk() -> io::Result<()> {
    let handle = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(win)? };
    let _owned = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    unsafe {
        Process32FirstW(handle, &mut entry).map_err(win)?;
    }
    for _ in 0..16384 {
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
        match unsafe { Process32NextW(handle, &mut entry) } {
            Ok(()) => (),
            Err(e) if e.code() == ERROR_NO_MORE_ITEMS.to_hresult() => return Ok(()),
            Err(e) => return Err(win(e)),
        }
    }
    Err(blocked("process inventory exceeded budget"))
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
fn file_security(path: &Path) -> io::Result<Vec<u8>> {
    let p = path_wide(path);
    let mut bytes = vec![0u8; 65536];
    let mut needed = 0;
    let ok = unsafe {
        GetFileSecurityW(
            PCWSTR(p.as_ptr()),
            (OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION).0,
            Some(PSECURITY_DESCRIPTOR(bytes.as_mut_ptr().cast())),
            bytes.len() as u32,
            &mut needed,
        )
    };
    if !ok.as_bool() {
        return Err(io::Error::last_os_error());
    }
    bytes.truncate(needed as usize);
    Ok(bytes)
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
    if size == 0 || size > 64 || size % 4 != 0 {
        return Err(blocked("unsupported version translation table"));
    }
    let translations =
        unsafe { std::slice::from_raw_parts(translations.cast::<u16>(), size as usize / 2) };
    let mut strings = BTreeMap::new();
    for pair in translations.chunks_exact(2) {
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
pub(super) fn check_observations(
    case: &str,
    tree: &[Entry],
    pe: &Value,
    scope: &Scope,
    install: &Path,
) -> io::Result<()> {
    if pe["machine"] != json!(0x8664)
        || pe["optionalHeaderMagic"] != json!(0x20b)
        || pe["fixedProductVersion"] != json!([0, 17, 7, 0])
    {
        return Err(blocked(
            "measured installed PE architecture or version differs",
        ));
    }
    let strings = pe["strings"]
        .as_object()
        .ok_or_else(|| blocked("missing PE resource strings"))?;
    if strings.iter().any(|(k, v)| {
        if k.ends_with("ProductName") {
            v != "CC Desk"
        } else {
            v != "0.17.7" && v != "0.17.7.0"
        }
    }) {
        return Err(blocked("measured PE product/version strings differ"));
    }
    for (path, size, hash) in [
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
    ] {
        if !tree
            .iter()
            .any(|e| e.path == path && e.size == size && e.sha256.as_deref() == Some(hash))
        {
            return Err(blocked(
                "installed source-declared resource differs; retain measured inventory",
            ));
        }
    }
    let mut expected = vec![
        "",
        "cc-desk.exe",
        "uninstall.exe",
        "conpty.dll",
        "OpenConsole.exe",
        "LICENSE-Microsoft-ConPTY.txt",
    ];
    if case == "seeded-existing" {
        expected.push("source-only-leftover.txt");
    }
    expected.sort_unstable();
    let actual: Vec<_> = tree.iter().map(|e| e.path.as_str()).collect();
    if actual != expected {
        return Err(blocked(
            "unexplained installed file/directory; preserve inventory for review",
        ));
    }
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
