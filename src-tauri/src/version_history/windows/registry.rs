//! Typed operations are restricted to the six installer-owned HKCU slots.
//! Complete tree/permission capture and journal orchestration belong to the
//! coordinator. This module never imports HKCU or supplies installation approval.
use super::{blocked, win_error};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io};
use windows::Wdk::System::Registry::{KeyNameInformation, NtQueryKey};
use windows::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, FILETIME, HANDLE},
    System::Registry::*,
};
use windows_core::{PCWSTR, PWSTR};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RegistrationSlot {
    Uninstall,
    Publisher,
    Directory,
    Background,
    LegacyDirectory,
    LegacyBackground,
}
impl RegistrationSlot {
    fn path(self) -> &'static str {
        match self {
            Self::Uninstall => "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk",
            Self::Publisher => "Software\\shawnwu2022\\CC Desk",
            Self::Directory => "Software\\Classes\\Directory\\shell\\cc-desk",
            Self::Background => "Software\\Classes\\Directory\\Background\\shell\\cc-desk",
            Self::LegacyDirectory => "Software\\Classes\\Directory\\shell\\cc-box",
            Self::LegacyBackground => "Software\\Classes\\Directory\\Background\\shell\\cc-box",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RegistryView {
    View32,
    View64,
}
impl RegistryView {
    fn flags(self) -> REG_SAM_FLAGS {
        match self {
            Self::View32 => KEY_WOW64_32KEY,
            Self::View64 => KEY_WOW64_64KEY,
        }
    }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryValue {
    pub(crate) kind: u32,
    pub(crate) bytes: Vec<u8>,
}
impl RegistryValue {
    fn validate(&self) -> io::Result<()> {
        if self.bytes.len() > 1024 * 1024 || !matches!(self.kind, 0 | 1 | 2 | 3 | 4 | 7 | 11) {
            return Err(blocked("unsupported typed registry value"));
        }
        if matches!(self.kind, 1 | 2 | 7)
            && (self.bytes.len() < 2
                || !self.bytes.len().is_multiple_of(2)
                || !self.bytes.ends_with(&[0, 0]))
        {
            return Err(blocked("malformed registry string"));
        }
        if (self.kind == 4 && self.bytes.len() != 4) || (self.kind == 11 && self.bytes.len() != 8) {
            return Err(blocked("malformed registry integer"));
        }
        Ok(())
    }
}
pub(crate) struct RegistrationKey {
    chain: KeyChain,
    slot: RegistrationSlot,
    view: RegistryView,
    relative: String,
    namespace: Vec<Vec<u16>>,
}
impl RegistrationKey {
    pub(crate) fn open(
        slot: RegistrationSlot,
        view: RegistryView,
        relative: &str,
    ) -> io::Result<Option<Self>> {
        if relative.len() > 1024
            || relative.split('\\').any(|part| {
                !relative.is_empty()
                    && (part.is_empty()
                        || part == "."
                        || part == ".."
                        || part.chars().any(char::is_control)
                        || part.contains(['/', '\0']))
            })
        {
            return Err(blocked("unsupported registration subkey"));
        }
        let path = if relative.is_empty() {
            slot.path().to_owned()
        } else {
            format!("{}\\{relative}", slot.path())
        };
        let Some(chain) = open_chain(HKEY_CURRENT_USER, &path, view, true)? else {
            return Ok(None);
        };
        let namespace = chain.current_names()?;
        let result = Self {
            chain,
            slot,
            view,
            relative: relative.into(),
            namespace,
        };
        result.verify_binding()?;
        Ok(Some(result))
    }
    fn raw(&self) -> HKEY {
        self.chain.raw()
    }
    fn verify_binding(&self) -> io::Result<()> {
        let path = if self.relative.is_empty() {
            self.slot.path().to_owned()
        } else {
            format!("{}\\{}", self.slot.path(), self.relative)
        };
        let fresh =
            self.chain
                .reopen_bound(HKEY_CURRENT_USER, &path, self.view, false, &self.namespace)?;
        self.chain.verify_same_namespace(&fresh, &self.namespace)
    }
    pub(crate) fn read(&self, name: &str) -> io::Result<Option<RegistryValue>> {
        self.verify_binding()?;
        let observed = read_value(self.raw(), name)?;
        self.verify_binding()?;
        Ok(observed)
    }
    /// Compare actual typed before state immediately before one mutation, then
    /// flush the containing hive and compare actual typed after state. This is
    /// not a cross-key transaction or atomic CAS against other applications.
    pub(crate) fn set(
        &self,
        name: &str,
        expected: Option<&RegistryValue>,
        value: Option<&RegistryValue>,
    ) -> io::Result<RegistryReceipt> {
        let name_wide = value_name(name)?;
        if let Some(value) = value {
            value.validate()?;
        }
        if self.read(name)?.as_ref() != expected {
            return Err(blocked("registration before state changed"));
        }
        unsafe {
            match value {
                Some(value) => RegSetValueExW(
                    self.raw(),
                    PCWSTR(name_wide.as_ptr()),
                    None,
                    REG_VALUE_TYPE(value.kind),
                    Some(&value.bytes),
                )
                .ok()
                .map_err(win_error)?,
                None => {
                    if expected.is_some() {
                        RegDeleteValueW(self.raw(), PCWSTR(name_wide.as_ptr()))
                            .ok()
                            .map_err(win_error)?;
                    }
                }
            }
            RegFlushKey(self.raw()).ok().map_err(win_error)?;
        }
        let observed = self.read(name)?;
        if observed.as_ref() != value {
            return Err(blocked("registration postcondition is unknown"));
        }
        Ok(RegistryReceipt {
            slot: self.slot,
            view: self.view,
            relative: self.relative.clone(),
            name: name.into(),
            observed,
        })
    }
}
#[derive(Serialize)]
pub(crate) struct RegistryReceipt {
    slot: RegistrationSlot,
    view: RegistryView,
    relative: String,
    name: String,
    observed: Option<RegistryValue>,
}
fn value_name(name: &str) -> io::Result<Vec<u16>> {
    if name.len() > 32760 || name.contains('\0') || name.chars().any(char::is_control) {
        return Err(blocked("unsupported registry value name"));
    }
    Ok(name.encode_utf16().chain(Some(0)).collect())
}
fn read_value(key: HKEY, name: &str) -> io::Result<Option<RegistryValue>> {
    read_value_bounded(key, name, 1024 * 1024)
}
pub(super) fn read_value_bounded(
    key: HKEY,
    name: &str,
    maximum: usize,
) -> io::Result<Option<RegistryValue>> {
    let name = value_name(name)?;
    let mut kind = REG_VALUE_TYPE::default();
    let mut length = 0;
    let status = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(win_error)?;
    if length as usize > maximum {
        return Err(blocked("registry value exceeds limit"));
    }
    let mut bytes = vec![0u8; length as usize];
    unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut length),
        )
        .ok()
        .map_err(win_error)?;
    }
    bytes.truncate(length as usize);
    Ok(Some(RegistryValue {
        kind: kind.0,
        bytes,
    }))
}
/// Query the current name from the retained kernel object, never from a cached
/// path. This observation does not lock the registry namespace against rename.
pub(super) fn current_key_name(key: HKEY) -> io::Result<Vec<u16>> {
    const MAX_BYTES: usize = 64 * 1024;
    let mut buffer = vec![0u32; (MAX_BYTES + 4) / 4];
    let mut returned = 0;
    let status = unsafe {
        NtQueryKey(
            HANDLE(key.0),
            KeyNameInformation,
            Some(buffer.as_mut_ptr().cast()),
            (buffer.len() * 4) as u32,
            &mut returned,
        )
    };
    if status.0 != 0 {
        return Err(blocked("registry key name is unavailable"));
    }
    let length = buffer[0] as usize;
    if length == 0
        || !length.is_multiple_of(2)
        || length > MAX_BYTES
        || returned as usize != length + 4
    {
        return Err(blocked("unsupported registry key name"));
    }
    // KEY_NAME_INFORMATION contains a byte length followed by nonterminated
    // UTF-16. The u32 allocation aligns both fields; only returned bytes are read.
    let name =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<u16>(), length / 2) };
    Ok(name.to_vec())
}
fn same_current_key_name(held: HKEY, reopened: HKEY, expected: &[u16]) -> io::Result<()> {
    let before = current_key_name(held)?;
    let observed = current_key_name(reopened)?;
    if before != expected
        || observed != expected
        || current_key_name(held)? != before
        || current_key_name(reopened)? != observed
    {
        return Err(blocked("registry key namespace changed"));
    }
    Ok(())
}
struct AliasBinding {
    key_index: usize,
    value: RegistryValue,
}
struct KeyChain {
    keys: Vec<Key>,
    aliases: Vec<AliasBinding>,
}
impl KeyChain {
    fn raw(&self) -> HKEY {
        self.keys.last().expect("nonempty registry chain").0
    }
    fn reopen_bound(
        &self,
        root: HKEY,
        path: &str,
        view: RegistryView,
        enumerate_leaf: bool,
        expected: &[Vec<u16>],
    ) -> io::Result<Self> {
        let fresh = open_chain_access(root, path, view, false, enumerate_leaf)?
            .ok_or_else(|| blocked("registry namespace disappeared"))?;
        self.verify_same_namespace(&fresh, expected)?;
        Ok(fresh)
    }
    fn current_names(&self) -> io::Result<Vec<Vec<u16>>> {
        self.keys
            .iter()
            .map(|key| current_key_name(key.0))
            .collect()
    }
    // Expected names were returned by the OS at admission, including its actual
    // WOW64 mapping. They only constrain freshly queried held AND fixed-path
    // reopened objects; cached spelling alone never authorizes observation.
    fn verify_same_namespace(&self, fresh: &Self, expected: &[Vec<u16>]) -> io::Result<()> {
        self.verify_aliases()?;
        fresh.verify_aliases()?;
        if self.keys.len() != fresh.keys.len()
            || self.keys.len() != expected.len()
            || self.aliases.len() != fresh.aliases.len()
        {
            return Err(blocked("registry namespace changed"));
        }
        for ((held, reopened), name) in self.keys.iter().zip(&fresh.keys).zip(expected) {
            same_current_key_name(held.0, reopened.0, name)?;
        }
        Ok(())
    }
    fn verify_aliases(&self) -> io::Result<()> {
        for (index, key) in self.keys.iter().enumerate() {
            let observed = read_value(key.0, "SymbolicLinkValue")?;
            if let Some(binding) = self
                .aliases
                .iter()
                .find(|binding| binding.key_index == index)
            {
                if observed.as_ref() != Some(&binding.value) {
                    return Err(blocked("standard policy alias changed"));
                }
            } else if observed.is_some_and(|value| value.kind == REG_LINK.0) {
                return Err(blocked("canonical registry chain became linked"));
            }
        }
        Ok(())
    }
}

/// Only the observed OS-shared WebView policy alias has authority here. This
/// predicate is independent of diagnostic strings and never applies to writes.
pub(crate) fn shared_policy_alias(
    root: HKEY,
    view: RegistryView,
    path: &str,
    index: usize,
    write_leaf: bool,
    value: &RegistryValue,
) -> bool {
    const TARGET: &str = r"\REGISTRY\MACHINE\SOFTWARE\Policies";
    root == HKEY_LOCAL_MACHINE
        && view == RegistryView::View32
        && index == 1
        && !write_leaf
        && matches!(
            path,
            "Software\\Policies\\Microsoft\\Edge\\WebView2\\UserDataFolder"
                | "Software\\Policies\\Microsoft\\Edge\\WebView2\\BrowserExecutableFolder"
                | "Software\\Policies\\Microsoft\\Edge\\WebView2\\AdditionalBrowserArguments"
        )
        && value.kind == REG_LINK.0
        && value.bytes.len() <= TARGET.len() * 2 + 2
        && registry_text(&value.bytes).is_some_and(|target| target.eq_ignore_ascii_case(TARGET))
}

/// The only current-user Classes redirection admitted by product operations.
/// Return a fixed canonical HKEY_USERS component derived from the actual token;
/// the observed REG_LINK is never followed by RegOpenKeyEx.
pub(crate) fn current_user_classes_alias(
    root: HKEY,
    path: &str,
    index: usize,
    value: &RegistryValue,
) -> io::Result<Option<String>> {
    const PREFIXES: [&str; 4] = [
        "Software\\Classes\\Directory\\shell\\cc-desk",
        "Software\\Classes\\Directory\\Background\\shell\\cc-desk",
        "Software\\Classes\\Directory\\shell\\cc-box",
        "Software\\Classes\\Directory\\Background\\shell\\cc-box",
    ];
    if root != HKEY_CURRENT_USER
        || index != 1
        || value.kind != REG_LINK.0
        || !PREFIXES.iter().any(|prefix| {
            path == *prefix
                || path
                    .strip_prefix(*prefix)
                    .is_some_and(|tail| tail.starts_with('\\'))
        })
        || path.split('\\').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.contains(['/', '\0'])
                || part.chars().any(char::is_control)
        })
    {
        return Ok(None);
    }
    let user = super::security::CurrentUser::capture()?;
    user.require_unelevated()?;
    let component = format!("{}_Classes", user.sid_text());
    let target = format!(r"\REGISTRY\USER\{component}");
    if registry_text(&value.bytes).is_some_and(|value| value.eq_ignore_ascii_case(&target)) {
        Ok(Some(component))
    } else {
        Ok(None)
    }
}

pub(super) fn registry_text(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<_> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let units = units.strip_suffix(&[0]).unwrap_or(&units);
    String::from_utf16(units).ok()
}

fn open_chain(
    root: HKEY,
    path: &str,
    view: RegistryView,
    write_leaf: bool,
) -> io::Result<Option<KeyChain>> {
    open_chain_access(root, path, view, write_leaf, false)
}
fn open_chain_access(
    root: HKEY,
    path: &str,
    view: RegistryView,
    write_leaf: bool,
    enumerate_leaf: bool,
) -> io::Result<Option<KeyChain>> {
    let parts: Vec<_> = path.split('\\').collect();
    let mut chain = KeyChain {
        keys: Vec::new(),
        aliases: Vec::new(),
    };
    let mut parent = root;
    let mut effective_view = view;
    for (index, part) in parts.iter().enumerate() {
        let text: Vec<_> = part.encode_utf16().chain(Some(0)).collect();
        let mut raw = HKEY::default();
        let access = KEY_QUERY_VALUE
            | effective_view.flags()
            | if enumerate_leaf && index + 1 == parts.len() {
                KEY_ENUMERATE_SUB_KEYS
            } else {
                REG_SAM_FLAGS(0)
            }
            | if write_leaf && index + 1 == parts.len() {
                KEY_SET_VALUE
            } else {
                REG_SAM_FLAGS(0)
            };
        let status = unsafe {
            RegOpenKeyExW(
                parent,
                PCWSTR(text.as_ptr()),
                Some(REG_OPTION_OPEN_LINK.0),
                access,
                &mut raw,
            )
        };
        if status == ERROR_FILE_NOT_FOUND {
            chain.verify_aliases()?;
            return Ok(None);
        }
        status.ok().map_err(win_error)?;
        let key = Key(raw);
        if let Some(value) =
            read_value(key.0, "SymbolicLinkValue")?.filter(|value| value.kind == REG_LINK.0)
        {
            if shared_policy_alias(root, view, path, index, write_leaf, &value) {
                // Never ask the OS to follow the observed link. Open only the
                // fixed native shared target, retaining the original alias.
                let canonical = open_chain(
                    HKEY_LOCAL_MACHINE,
                    "Software\\Policies",
                    RegistryView::View64,
                    false,
                )?
                .ok_or_else(|| blocked("standard policy alias target is missing"))?;
                let key_index = chain.keys.len();
                chain.keys.push(key);
                chain.aliases.push(AliasBinding { key_index, value });
                chain.keys.extend(canonical.keys);
                parent = chain.raw();
                effective_view = RegistryView::View64;
                chain.verify_aliases()?;
                continue;
            }
            if let Some(component) = current_user_classes_alias(root, path, index, &value)? {
                // Explicitly open the verified current SID's canonical hive;
                // write access remains confined to the originally fixed leaf.
                let canonical = open_chain(HKEY_USERS, &component, view, false)?
                    .ok_or_else(|| blocked("current-user Classes target is missing"))?;
                let key_index = chain.keys.len();
                chain.keys.push(key);
                chain.aliases.push(AliasBinding { key_index, value });
                chain.keys.extend(canonical.keys);
                parent = chain.raw();
                chain.verify_aliases()?;
                continue;
            }
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                link_diagnostic(root, view, part, &value.bytes),
            ));
        }
        parent = key.0;
        chain.keys.push(key);
    }
    chain.verify_aliases()?;
    Ok(Some(chain))
}

const UNINSTALL_ROOT: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall";
const PRODUCT_ROOT: &str = "Software\\shawnwu2022\\CC Desk";
const INSTALL_VALUE_NAMES: &[&str] = &[
    "DisplayName",
    "Publisher",
    "DisplayVersion",
    "MainBinaryName",
    "InstallLocation",
    "UninstallString",
    "DisplayIcon",
    "WindowsInstaller",
    "CurrentUser",
    "AllUsers",
];
const MAX_UNINSTALL_KEYS: usize = 2048;
const MAX_DISCOVERY_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallHive {
    CurrentUser,
    LocalMachine,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct InstallRecord {
    pub(crate) hive: InstallHive,
    pub(crate) view: RegistryView,
    pub(crate) name: String,
    pub(crate) values: BTreeMap<String, Option<RegistryValue>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct KeyStamp {
    children: u32,
    values: u32,
    modified: u64,
}
pub(super) fn key_stamp(key: HKEY) -> io::Result<KeyStamp> {
    let mut children = 0;
    let mut values = 0;
    let mut modified = FILETIME::default();
    unsafe {
        RegQueryInfoKeyW(
            key,
            None,
            None,
            None,
            Some(&mut children),
            None,
            None,
            Some(&mut values),
            None,
            None,
            None,
            Some(&mut modified),
        )
        .ok()
        .map_err(win_error)?;
    }
    Ok(KeyStamp {
        children,
        values,
        modified: ((modified.dwHighDateTime as u64) << 32) | modified.dwLowDateTime as u64,
    })
}
pub(super) fn enum_subkeys(key: HKEY) -> io::Result<Vec<String>> {
    let before = key_stamp(key)?;
    if before.children as usize > MAX_UNINSTALL_KEYS {
        return Err(blocked("uninstall key count exceeds limit"));
    }
    let mut names = Vec::new();
    for index in 0..=before.children {
        let mut buffer = [0u16; 256];
        let mut length = buffer.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        status.ok().map_err(win_error)?;
        if length == 0 || length > 255 {
            return Err(blocked("unsupported uninstall key name"));
        }
        let name = String::from_utf16(&buffer[..length as usize])
            .map_err(|_| blocked("unrepresentable uninstall key"))?;
        if name.contains(['\\', '/', '\0']) || name.chars().any(char::is_control) {
            return Err(blocked("unsupported uninstall key name"));
        }
        names.push(name);
    }
    if names.len() != before.children as usize || key_stamp(key)? != before {
        return Err(blocked("uninstall namespace changed"));
    }
    names.sort();
    Ok(names)
}
fn open_install_child(parent: HKEY, view: RegistryView, name: &str) -> io::Result<Key> {
    let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
    let mut raw = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            parent,
            PCWSTR(wide.as_ptr()),
            Some(REG_OPTION_OPEN_LINK.0),
            KEY_QUERY_VALUE | view.flags(),
            &mut raw,
        )
        .ok()
        .map_err(win_error)?;
    }
    let key = Key(raw);
    reject_install_link(key.0)?;
    Ok(key)
}
struct ReadInstallKey {
    key: Key,
    namespace: Vec<u16>,
    stamp: KeyStamp,
    record: InstallRecord,
}
impl ReadInstallKey {
    fn observe(
        parent: HKEY,
        hive: InstallHive,
        view: RegistryView,
        name: String,
        total: &mut usize,
    ) -> io::Result<Self> {
        let key = open_install_child(parent, view, &name)?;
        let raw = key.0;
        let stamp = key_stamp(raw)?;
        reject_install_link(raw)?;
        let mut values = BTreeMap::new();
        for name in INSTALL_VALUE_NAMES {
            let value = read_value_bounded(raw, name, 65536)?;
            *total = total
                .checked_add(value.as_ref().map_or(0, |value| value.bytes.len()))
                .ok_or_else(|| blocked("registration inventory exceeds limit"))?;
            if *total > MAX_DISCOVERY_BYTES {
                return Err(blocked("registration inventory exceeds limit"));
            }
            values.insert((*name).into(), value);
        }
        let result = Self {
            namespace: current_key_name(key.0)?,
            key,
            stamp,
            record: InstallRecord {
                hive,
                view,
                name,
                values,
            },
        };
        result.recheck()?;
        Ok(result)
    }
    fn recheck(&self) -> io::Result<()> {
        self.recheck_key(self.key.0)
    }
    fn recheck_under(&self, parent: HKEY) -> io::Result<()> {
        let fresh = open_install_child(parent, self.record.view, &self.record.name)?;
        same_current_key_name(self.key.0, fresh.0, &self.namespace)?;
        self.recheck()?;
        self.recheck_key(fresh.0)?;
        same_current_key_name(self.key.0, fresh.0, &self.namespace)?;
        Ok(())
    }
    fn recheck_key(&self, key: HKEY) -> io::Result<()> {
        reject_install_link(key)?;
        if key_stamp(key)? != self.stamp {
            return Err(blocked("installation registration changed"));
        }
        for (name, expected) in &self.record.values {
            if &read_value_bounded(key, name, 65536)? != expected {
                return Err(blocked("installation registration changed"));
            }
        }
        if key_stamp(key)? != self.stamp {
            return Err(blocked("installation registration changed during read"));
        }
        Ok(())
    }
}
fn reject_install_link(key: HKEY) -> io::Result<()> {
    if read_value_bounded(key, "SymbolicLinkValue", 65536)?
        .is_some_and(|value| value.kind == REG_LINK.0)
    {
        return Err(blocked("linked installation registration is unsupported"));
    }
    Ok(())
}
struct UninstallView {
    root: HKEY,
    view: RegistryView,
    chain: Option<KeyChain>,
    namespace: Vec<Vec<u16>>,
    names: Vec<String>,
    records: Vec<ReadInstallKey>,
}
impl UninstallView {
    fn observe(
        root: HKEY,
        hive: InstallHive,
        view: RegistryView,
        total: &mut usize,
    ) -> io::Result<Self> {
        let chain = open_chain_access(root, UNINSTALL_ROOT, view, false, true)?;
        let namespace = chain
            .as_ref()
            .map(KeyChain::current_names)
            .transpose()?
            .unwrap_or_default();
        let mut names = Vec::new();
        let mut records = Vec::new();
        if let Some(chain) = &chain {
            names = enum_subkeys(chain.raw())?;
            for name in &names {
                records.push(ReadInstallKey::observe(
                    chain.raw(),
                    hive,
                    view,
                    name.clone(),
                    total,
                )?);
            }
        }
        let result = Self {
            root,
            view,
            chain,
            namespace,
            names,
            records,
        };
        result.recheck()?;
        Ok(result)
    }
    fn recheck(&self) -> io::Result<()> {
        if let Some(chain) = &self.chain {
            let fresh =
                chain.reopen_bound(self.root, UNINSTALL_ROOT, self.view, true, &self.namespace)?;
            if enum_subkeys(chain.raw())? != self.names || enum_subkeys(fresh.raw())? != self.names
            {
                return Err(blocked("uninstall namespace changed"));
            }
            for record in &self.records {
                record.recheck_under(fresh.raw())?;
            }
            if enum_subkeys(chain.raw())? != self.names || enum_subkeys(fresh.raw())? != self.names
            {
                return Err(blocked("uninstall namespace changed during observation"));
            }
            chain.verify_same_namespace(&fresh, &self.namespace)?;
        } else if open_chain_access(self.root, UNINSTALL_ROOT, self.view, false, true)?.is_some() {
            return Err(blocked("uninstall namespace appeared"));
        }
        Ok(())
    }
}
struct PublisherView {
    root: HKEY,
    view: RegistryView,
    chain: Option<KeyChain>,
    namespace: Vec<Vec<u16>>,
    value: Option<RegistryValue>,
    stamp: Option<KeyStamp>,
}
impl PublisherView {
    fn observe(root: HKEY, view: RegistryView) -> io::Result<Self> {
        let chain = open_chain(root, PRODUCT_ROOT, view, false)?;
        let namespace = chain
            .as_ref()
            .map(KeyChain::current_names)
            .transpose()?
            .unwrap_or_default();
        let (value, stamp) = if let Some(chain) = &chain {
            (
                read_value_bounded(chain.raw(), "", 65536)?,
                Some(key_stamp(chain.raw())?),
            )
        } else {
            (None, None)
        };
        let result = Self {
            root,
            view,
            chain,
            namespace,
            value,
            stamp,
        };
        result.recheck()?;
        Ok(result)
    }
    fn recheck(&self) -> io::Result<()> {
        if let Some(chain) = &self.chain {
            let fresh =
                chain.reopen_bound(self.root, PRODUCT_ROOT, self.view, false, &self.namespace)?;
            for key in [chain.raw(), fresh.raw()] {
                if Some(key_stamp(key)?) != self.stamp
                    || read_value_bounded(key, "", 65536)? != self.value
                    || Some(key_stamp(key)?) != self.stamp
                {
                    return Err(blocked("publisher registration changed"));
                }
            }
            chain.verify_same_namespace(&fresh, &self.namespace)?;
        } else if open_chain(self.root, PRODUCT_ROOT, self.view, false)?.is_some() {
            return Err(blocked("publisher registration appeared"));
        }
        Ok(())
    }
}
/// Complete bounded name/selection-field observations across both hives/views.
/// Every existing key remains held and its current fixed-path binding is reopened
/// and rechecked. This does not guarantee future atomic namespace stability or a
/// complete registration/security restore manifest. No write rights are opened.
pub(crate) struct InstallRegistryObservation {
    views: Vec<UninstallView>,
    publisher: Vec<PublisherView>,
}
impl InstallRegistryObservation {
    pub(crate) fn capture() -> io::Result<Self> {
        Self::capture_hives(HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE)
    }
    fn capture_hives(user: HKEY, machine: HKEY) -> io::Result<Self> {
        let mut total = 0;
        let mut views = Vec::new();
        let mut publisher = Vec::new();
        for (root, hive) in [
            (user, InstallHive::CurrentUser),
            (machine, InstallHive::LocalMachine),
        ] {
            for view in [RegistryView::View32, RegistryView::View64] {
                views.push(UninstallView::observe(root, hive, view, &mut total)?);
            }
        }
        for view in [RegistryView::View32, RegistryView::View64] {
            publisher.push(PublisherView::observe(user, view)?);
        }
        let result = Self { views, publisher };
        result.recheck()?;
        Ok(result)
    }
    pub(crate) fn records(&self) -> impl Iterator<Item = &InstallRecord> {
        self.views
            .iter()
            .flat_map(|view| view.records.iter().map(|record| &record.record))
    }
    pub(crate) fn publisher_values(&self) -> impl Iterator<Item = Option<&RegistryValue>> {
        self.publisher.iter().map(|view| view.value.as_ref())
    }
    pub(crate) fn recheck(&self) -> io::Result<()> {
        for view in &self.views {
            view.recheck()?;
        }
        for view in &self.publisher {
            view.recheck()?;
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn fixture_hives(user: HKEY, machine: HKEY) -> io::Result<Self> {
        Self::capture_hives(user, machine)
    }
}

/// Bounded diagnosis only. Recognizing a standard target does not authorize
/// following it. Never put arbitrary registry data, paths or SIDs into errors.
pub(crate) fn link_diagnostic(
    root: HKEY,
    view: RegistryView,
    component: &str,
    target: &[u8],
) -> String {
    let hive = if root == HKEY_CURRENT_USER {
        "currentUser"
    } else if root == HKEY_LOCAL_MACHINE {
        "localMachine"
    } else {
        "unknown"
    };
    let component = match component {
        "Software" => "software",
        "Classes" => "classes",
        "Policies" => "policies",
        "Microsoft" => "microsoft",
        "Edge" => "edge",
        "WebView2" => "webview2",
        "UserDataFolder" => "userDataFolder",
        "BrowserExecutableFolder" => "browserExecutableFolder",
        "AdditionalBrowserArguments" => "additionalBrowserArguments",
        _ => "unknown",
    };
    let recognized = [
        ("machinePolicies", r"\REGISTRY\MACHINE\SOFTWARE\Policies"),
        ("machineClasses", r"\REGISTRY\MACHINE\SOFTWARE\Classes"),
        (
            "machineClasses32",
            r"\REGISTRY\MACHINE\SOFTWARE\Classes\Wow6432Node",
        ),
    ];
    let category = registry_text(target)
        .and_then(|value| {
            recognized
                .iter()
                .find(|(_, expected)| value.eq_ignore_ascii_case(expected))
                .map(|(category, _)| *category)
                .or_else(|| {
                    super::security::CurrentUser::capture()
                        .ok()
                        .and_then(|user| {
                            let expected = format!(r"\REGISTRY\USER\{}_Classes", user.sid_text());
                            value
                                .eq_ignore_ascii_case(&expected)
                                .then_some("currentUserClasses")
                        })
                })
        })
        .unwrap_or("unknown");
    format!("registry links are unsupported: hive={hive}, view={view:?}, component={component}, target={category}")
}

/// Conservative pre-controller check: any configured WebView policy under one
/// of these override branches blocks, even if current app-name precedence would
/// not select it. Inaccessible or unknown linked policy roots also block; only
/// the exact shared machine Policies alias is traversed via its fixed target.
pub(super) fn reject_webview_overrides() -> io::Result<()> {
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for view in [RegistryView::View32, RegistryView::View64] {
            for branch in [
                "UserDataFolder",
                "BrowserExecutableFolder",
                "AdditionalBrowserArguments",
            ] {
                let path = format!("Software\\Policies\\Microsoft\\Edge\\WebView2\\{branch}");
                if let Some(keys) = open_chain(root, &path, view, false)? {
                    let mut values = 0;
                    let mut subkeys = 0;
                    let status = unsafe {
                        RegQueryInfoKeyW(
                            keys.raw(),
                            None,
                            None,
                            None,
                            Some(&mut subkeys),
                            None,
                            None,
                            Some(&mut values),
                            None,
                            None,
                            None,
                            None,
                        )
                    };
                    status.ok().map_err(win_error)?;
                    keys.verify_aliases()?;
                    reject_policy_contents(values, subkeys)?;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn reject_policy_contents(values: u32, subkeys: u32) -> io::Result<()> {
    if values != 0 || subkeys != 0 {
        return Err(blocked("WebView policy override is configured"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) struct AliasGuardProbe(KeyChain);
#[cfg(test)]
impl AliasGuardProbe {
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.0.verify_aliases()
    }
}
#[cfg(test)]
pub(crate) fn fixture_alias_guard(root: HKEY, name: &str) -> io::Result<AliasGuardProbe> {
    let text = value_name(name)?;
    let mut raw = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            root,
            PCWSTR(text.as_ptr()),
            Some(REG_OPTION_OPEN_LINK.0),
            KEY_QUERY_VALUE,
            &mut raw,
        )
        .ok()
        .map_err(win_error)?;
    }
    let key = Key(raw);
    let value = read_value(key.0, "SymbolicLinkValue")?
        .filter(|value| value.kind == REG_LINK.0)
        .ok_or_else(|| blocked("fixture alias value missing"))?;
    Ok(AliasGuardProbe(KeyChain {
        keys: vec![key],
        aliases: vec![AliasBinding {
            key_index: 0,
            value,
        }],
    }))
}
