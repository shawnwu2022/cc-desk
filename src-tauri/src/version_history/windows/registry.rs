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
            && (self.bytes.len() < 2 || self.bytes.len() % 2 != 0 || !self.bytes.ends_with(&[0, 0]))
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
    chain: Vec<Key>,
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
        self.chain.last().expect("nonempty registry chain").0
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
fn open_chain(
    root: HKEY,
    path: &str,
    view: RegistryView,
    write_leaf: bool,
) -> io::Result<Option<Vec<Key>>> {
    let parts: Vec<_> = path.split('\\').collect();
    let mut keys = Vec::new();
    let mut parent = root;
    for (index, part) in parts.iter().enumerate() {
        let text: Vec<_> = part.encode_utf16().chain(Some(0)).collect();
        let mut raw = HKEY::default();
        let access = KEY_QUERY_VALUE
            | view.flags()
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
            return Ok(None);
        }
        status.ok().map_err(win_error)?;
        let key = Key(raw);
        if read_value(key.0, "SymbolicLinkValue")?.is_some_and(|value| value.kind == REG_LINK.0) {
            return Err(blocked("registry links are unsupported"));
        }
        parent = key.0;
        keys.push(key);
    }
    Ok(Some(keys))
}

/// Conservative pre-controller check: any configured WebView policy under one
/// of these override branches blocks, even if current app-name precedence would
/// not select it. Inaccessible or linked policy roots also block.
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
                            keys.last().expect("policy key").0,
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
                    if values != 0 || subkeys != 0 {
                        return Err(blocked("WebView policy override is configured"));
                    }
                }
            }
        }
    }
    Ok(())
}
