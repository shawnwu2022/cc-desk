//! Typed operations are restricted to the six installer-owned HKCU slots.
//! Complete tree/permission capture and journal orchestration belong to the
//! coordinator. This module never imports HKCU or supplies installation approval.
use super::{blocked, win_error};
use serde::{Deserialize, Serialize};
use std::io;
use windows::Win32::{Foundation::ERROR_FILE_NOT_FOUND, System::Registry::*};
use windows_core::PCWSTR;

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
        Ok(Some(Self {
            chain,
            slot,
            view,
            relative: relative.into(),
        }))
    }
    fn raw(&self) -> HKEY {
        self.chain.raw()
    }
    pub(crate) fn read(&self, name: &str) -> io::Result<Option<RegistryValue>> {
        read_value(self.raw(), name)
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
    if length > 1024 * 1024 {
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

fn registry_text(bytes: &[u8]) -> Option<String> {
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
