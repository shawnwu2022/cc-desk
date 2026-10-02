//! Complete bounded product-owned registration. Shared Run contributes exactly
//! one named value; no whole shared key is restored. Paths remain fixed selectors
//! resolved through retained, no-link keys, including one verified OS Classes alias.
//! Registry writes/readbacks are not an atomic CAS against other applications;
//! the coordinator must separately establish writer quiescence. An observed
//! conflict is retained and blocks, and uncertain issued effects never replay.
use super::{
    blocked,
    files::{Directory, FileIdentity, PrivateDirectory},
    lease::ExclusiveLease,
    registry::{
        current_key_name, enum_subkeys, key_stamp, read_value_bounded, registry_text, KeyStamp,
        RegistryValue,
    },
    scope::RegisteredInstallation,
    security::CurrentUser,
    win_error,
};
use crate::version_history::{
    journal::{
        EffectKind, EffectSpec, JournalBinding, JournalEvent, JournalPhase, JournalStore,
        ManifestRole, Observation, ObservedResult, RegistrationOperation, RegistrationSlot,
    },
    verified_package::sha256,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, rc::Rc, sync::Arc};
use windows::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_ITEMS},
    Security::{
        DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR,
    },
    System::Registry::*,
};
use windows_core::{PCWSTR, PWSTR};

const MAX_KEYS: usize = 4096;
const MAX_VALUES: usize = 16384;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_VALUE: usize = 1024 * 1024;
const MAX_DEPTH: usize = 32;
const RUN_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_NAME: &str = "CC Desk";
const SLOTS: [RegistrationSlot; 6] = [
    RegistrationSlot::Uninstall,
    RegistrationSlot::Publisher,
    RegistrationSlot::DeskDirectory,
    RegistrationSlot::DeskDirectoryBackground,
    RegistrationSlot::LegacyDirectory,
    RegistrationSlot::LegacyDirectoryBackground,
];
fn slot_path(slot: RegistrationSlot) -> &'static str {
    match slot {
        RegistrationSlot::Uninstall => {
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\CC Desk"
        }
        RegistrationSlot::Publisher => "Software\\shawnwu2022\\CC Desk",
        RegistrationSlot::DeskDirectory => "Software\\Classes\\Directory\\shell\\cc-desk",
        RegistrationSlot::DeskDirectoryBackground => {
            "Software\\Classes\\Directory\\Background\\shell\\cc-desk"
        }
        RegistrationSlot::LegacyDirectory => "Software\\Classes\\Directory\\shell\\cc-box",
        RegistrationSlot::LegacyDirectoryBackground => {
            "Software\\Classes\\Directory\\Background\\shell\\cc-box"
        }
        RegistrationSlot::OwnedRun => RUN_PATH,
    }
}
fn wide(text: &str) -> io::Result<Vec<u16>> {
    if text.contains('\0') || text.encode_utf16().count() > 32760 {
        return Err(blocked("unsupported registry selector"));
    }
    Ok(text.encode_utf16().chain(Some(0)).collect())
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
struct Guard {
    key: Rc<Key>,
    name: Vec<u16>,
    stamp: KeyStamp,
    link: Option<RegistryValue>,
}
impl Guard {
    fn capture(key: Rc<Key>) -> io::Result<Self> {
        Ok(Self {
            name: current_key_name(key.0)?,
            stamp: key_stamp(key.0)?,
            link: read_value_bounded(key.0, "SymbolicLinkValue", 65536)?
                .filter(|value| value.kind == REG_LINK.0),
            key,
        })
    }
    fn verify(&self) -> io::Result<()> {
        if current_key_name(self.key.0)? != self.name
            || key_stamp(self.key.0)? != self.stamp
            || read_value_bounded(self.key.0, "SymbolicLinkValue", 65536)?
                .filter(|value| value.kind == REG_LINK.0)
                != self.link
        {
            return Err(blocked("retained registry observation changed"));
        }
        Ok(())
    }
}
fn open_one(parent: HKEY, name: &str, access: REG_SAM_FLAGS) -> io::Result<Option<Rc<Key>>> {
    let name = wide(name)?;
    let mut raw = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            parent,
            PCWSTR(name.as_ptr()),
            Some(REG_OPTION_OPEN_LINK.0),
            access | KEY_WOW64_64KEY,
            &mut raw,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    status.ok().map_err(win_error)?;
    Ok(Some(Rc::new(Key(raw))))
}
struct OpenPath {
    leaf: Option<Rc<Key>>,
    guards: Vec<Guard>,
}
fn open_path(
    root: HKEY,
    path: &str,
    sid: &str,
    leaf_access: REG_SAM_FLAGS,
) -> io::Result<OpenPath> {
    let parts: Vec<_> = path.split('\\').collect();
    let mut parent = root;
    let mut guards = Vec::new();
    let mut last = None;
    for (index, part) in parts.iter().enumerate() {
        let Some(key) = open_one(
            parent,
            part,
            if index + 1 == parts.len() {
                leaf_access
            } else {
                KEY_READ
            },
        )?
        else {
            return Ok(OpenPath { leaf: None, guards });
        };
        let guard = Guard::capture(key.clone())?;
        if let Some(link) = &guard.link {
            let target = format!(r"\REGISTRY\USER\{sid}_Classes");
            if root != HKEY_CURRENT_USER
                || index != 1
                || parts[..=index] != ["Software", "Classes"]
                || link.kind != REG_LINK.0
                || registry_text(&link.bytes)
                    .is_none_or(|value| !value.eq_ignore_ascii_case(&target))
            {
                return Err(blocked("unsupported registry alias"));
            }
            guards.push(guard);
            let key = open_one(HKEY_USERS, &format!("{sid}_Classes"), KEY_READ)?
                .ok_or_else(|| blocked("current-user Classes target missing"))?;
            let guard = Guard::capture(key.clone())?;
            if guard.link.is_some() {
                return Err(blocked("linked Classes target"));
            }
            guards.push(guard);
            parent = key.0;
            last = Some(key);
            continue;
        }
        parent = key.0;
        guards.push(guard);
        last = Some(key);
    }
    for guard in &guards {
        guard.verify()?;
    }
    Ok(OpenPath { leaf: last, guards })
}
fn descriptor(key: HKEY) -> io::Result<Vec<u8>> {
    let flags = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut length = 0;
    let status = unsafe { RegGetKeySecurity(key, flags, None, &mut length) };
    if status != ERROR_INSUFFICIENT_BUFFER || !(20..=65536).contains(&length) {
        return Err(blocked("registry security unavailable"));
    }
    let mut words = vec![0u32; (length as usize).div_ceil(4)];
    unsafe {
        RegGetKeySecurity(
            key,
            flags,
            Some(PSECURITY_DESCRIPTOR(words.as_mut_ptr().cast())),
            &mut length,
        )
        .ok()
        .map_err(win_error)?;
    }
    if length as usize > words.len() * 4 {
        return Err(blocked("registry descriptor changed size"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(words.as_ptr().cast::<u8>(), length as usize) }
        .to_vec();
    super::security::ValidatedDescriptor::from_bytes(&bytes)?;
    Ok(bytes)
}
fn key_class(key: HKEY) -> io::Result<String> {
    let mut bytes = vec![0u16; 1024];
    let mut length = bytes.len() as u32;
    unsafe {
        RegQueryInfoKeyW(
            key,
            Some(PWSTR(bytes.as_mut_ptr())),
            Some(&mut length),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .ok()
        .map_err(win_error)?;
    }
    if length as usize >= bytes.len() {
        return Err(blocked("registry class exceeds limit"));
    }
    String::from_utf16(&bytes[..length as usize]).map_err(|_| blocked("unsupported registry class"))
}
#[derive(Default)]
struct Budget {
    keys: usize,
    values: usize,
    bytes: usize,
}
impl Budget {
    fn bytes(&mut self, count: usize) -> io::Result<()> {
        self.bytes = self
            .bytes
            .checked_add(count)
            .ok_or_else(|| blocked("registry inventory limit"))?;
        if self.bytes > MAX_BYTES {
            return Err(blocked("registry inventory limit"));
        }
        Ok(())
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    relative: Vec<String>,
    namespace: Vec<u16>,
    class: String,
    security: Vec<u8>,
    values: BTreeMap<String, RegistryValue>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Tree {
    slot: RegistrationSlot,
    parent: SharedParent,
    nodes: Vec<Node>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedParent {
    namespace: Option<Vec<u16>>,
    security: Option<Vec<u8>>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedRun {
    namespace: Option<Vec<u16>>,
    parent_security: Option<Vec<u8>>,
    value: Option<RegistryValue>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    schema: u32,
    user_sid: String,
    installation: FileIdentity,
    trees: Vec<Tree>,
    run: OwnedRun,
}
fn values(key: HKEY, budget: &mut Budget) -> io::Result<BTreeMap<String, RegistryValue>> {
    let before = key_stamp(key)?;
    let mut result = BTreeMap::new();
    for index in 0..=MAX_VALUES {
        let mut name = vec![0u16; 32761];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumValueW(
                key,
                index as u32,
                Some(PWSTR(name.as_mut_ptr())),
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
        budget.values += 1;
        if budget.values > MAX_VALUES || length as usize >= name.len() {
            return Err(blocked("registry value count limit"));
        }
        let name = String::from_utf16(&name[..length as usize])
            .map_err(|_| blocked("unsupported registry value name"))?;
        let value = read_value_bounded(key, &name, MAX_VALUE)?
            .ok_or_else(|| blocked("registry value disappeared"))?;
        budget.bytes(name.len() + value.bytes.len())?;
        if result.insert(name, value).is_some() {
            return Err(blocked("duplicate registry enumeration"));
        }
    }
    if key_stamp(key)? != before {
        return Err(blocked("registry values changed during observation"));
    }
    Ok(result)
}
fn walk(
    key: Rc<Key>,
    relative: Vec<String>,
    budget: &mut Budget,
    nodes: &mut Vec<Node>,
    guards: &mut Vec<Guard>,
) -> io::Result<()> {
    budget.keys += 1;
    if budget.keys > MAX_KEYS || relative.len() > MAX_DEPTH {
        return Err(blocked("registry tree exceeds limit"));
    }
    let guard = Guard::capture(key.clone())?;
    if guard.link.is_some() {
        return Err(blocked("linked product registration"));
    }
    let names = enum_subkeys(key.0)?;
    let security = descriptor(key.0)?;
    budget.bytes(security.len())?;
    nodes.push(Node {
        relative: relative.clone(),
        namespace: guard.name.clone(),
        class: key_class(key.0)?,
        security,
        values: values(key.0, budget)?,
    });
    for name in &names {
        let child = open_one(key.0, name, KEY_READ)?
            .ok_or_else(|| blocked("registry child disappeared"))?;
        let mut path = relative.clone();
        path.push(name.clone());
        walk(child, path, budget, nodes, guards)?;
    }
    if enum_subkeys(key.0)? != names {
        return Err(blocked("registry children changed during observation"));
    }
    guard.verify()?;
    guards.push(guard);
    Ok(())
}
fn capture(
    root: HKEY,
    sid: &str,
    installation: FileIdentity,
) -> io::Result<(Snapshot, Vec<Guard>)> {
    let mut budget = Budget::default();
    let mut trees = Vec::new();
    let mut guards = Vec::new();
    for slot in SLOTS {
        let parent_path = slot_path(slot)
            .rsplit_once('\\')
            .ok_or_else(|| blocked("shared registry parent missing"))?
            .0;
        let parent_path = open_path(root, parent_path, sid, KEY_READ)?;
        let parent = if let Some(key) = parent_path.leaf {
            let namespace = current_key_name(key.0)?;
            let security = descriptor(key.0)?;
            budget.bytes(namespace.len() * 2 + security.len())?;
            SharedParent {
                namespace: Some(namespace),
                security: Some(security),
            }
        } else {
            SharedParent {
                namespace: None,
                security: None,
            }
        };
        guards.extend(parent_path.guards);
        let path = open_path(root, slot_path(slot), sid, KEY_READ)?;
        let mut nodes = Vec::new();
        if let Some(key) = path.leaf {
            walk(key, Vec::new(), &mut budget, &mut nodes, &mut guards)?;
        }
        guards.extend(path.guards);
        trees.push(Tree {
            slot,
            parent,
            nodes,
        });
    }
    let path = open_path(root, RUN_PATH, sid, KEY_READ)?;
    let run = if let Some(key) = path.leaf {
        let value = read_value_bounded(key.0, RUN_NAME, MAX_VALUE)?;
        budget.bytes(value.as_ref().map_or(0, |v| v.bytes.len()))?;
        let parent_security = descriptor(key.0)?;
        budget.bytes(parent_security.len())?;
        OwnedRun {
            namespace: Some(current_key_name(key.0)?),
            parent_security: Some(parent_security),
            value,
        }
    } else {
        OwnedRun {
            namespace: None,
            parent_security: None,
            value: None,
        }
    };
    guards.extend(path.guards);
    for guard in &guards {
        guard.verify()?;
    }
    Ok((
        Snapshot {
            schema: 2,
            user_sid: sid.into(),
            installation,
            trees,
            run,
        },
        guards,
    ))
}
/// The reserved value name is not ownership evidence. Only a terminated REG_SZ
/// containing one quoted fixed Desk executable, with no arguments, is currently
/// supported. Its parent must resolve to the actual registered directory object.
fn verify_owned_run(value: Option<&RegistryValue>, installation: &Directory) -> io::Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.kind != REG_SZ.0 || value.bytes.len() < 2 || !value.bytes.len().is_multiple_of(2) {
        return Err(blocked("Run value has unsupported ownership grammar"));
    }
    let units: Vec<_> = value
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let content = units
        .strip_suffix(&[0])
        .ok_or_else(|| blocked("Run string is not terminated"))?;
    if content.contains(&0) {
        return Err(blocked("Run string contains embedded terminator"));
    }
    let command =
        String::from_utf16(content).map_err(|_| blocked("Run command is not representable"))?;
    let image = command
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .filter(|value| {
            !value.is_empty() && !value.contains('"') && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| blocked("Run command has unsupported arguments or quoting"))?;
    let image = image.strip_prefix(r"\\?\").unwrap_or(image);
    let path = std::path::Path::new(image);
    if !path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("cc-desk.exe"))
    {
        return Err(blocked("reserved Run value targets another executable"));
    }
    let parent = Directory::open_absolute(
        path.parent()
            .ok_or_else(|| blocked("Run command parent missing"))?,
    )?;
    installation.recheck()?;
    if parent.identity() != installation.identity() {
        return Err(blocked("Run command targets another installation"));
    }
    parent.recheck()?;
    Ok(())
}
fn encode(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let bytes =
        serde_json::to_vec(value).map_err(|_| blocked("registry manifest encode failed"))?;
    if bytes.is_empty() || bytes.len() > 24 * 1024 * 1024 {
        return Err(blocked("registry manifest limit"));
    }
    Ok(bytes)
}
/// Complete current observation with real namespace/permission guards. Retain
/// consumes all key handles before NSIS may replace the product-owned trees.
pub(crate) struct HeldRegistrationState {
    root: HKEY,
    installation: Arc<Directory>,
    snapshot: Snapshot,
    guards: Vec<Guard>,
}
impl HeldRegistrationState {
    pub(crate) fn capture(installation: &RegisteredInstallation) -> io::Result<Self> {
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed"))?;
        let state = Self::observe(HKEY_CURRENT_USER, installation.directory().clone())?;
        installation
            .recheck()
            .map_err(|_| blocked("registered source changed"))?;
        state.recheck()?;
        Ok(state)
    }
    fn observe(root: HKEY, installation: Arc<Directory>) -> io::Result<Self> {
        let user = CurrentUser::capture()?;
        installation.recheck()?;
        let (snapshot, guards) = capture(root, user.sid_text(), installation.identity().clone())?;
        snapshot.validate(&user)?;
        require_source_access(root, &snapshot)?;
        Ok(Self {
            root,
            installation,
            snapshot,
            guards,
        })
    }
    pub(crate) fn recheck(&self) -> io::Result<()> {
        self.installation.recheck()?;
        for guard in &self.guards {
            guard.verify()?;
        }
        let (observed, _guards) = capture(
            self.root,
            &self.snapshot.user_sid,
            self.installation.identity().clone(),
        )?;
        if observed != self.snapshot {
            return Err(blocked("registration observation changed"));
        }
        for guard in &self.guards {
            guard.verify()?;
        }
        Ok(())
    }
    pub(crate) fn encode(&self) -> io::Result<Vec<u8>> {
        self.recheck()?;
        encode(&self.snapshot)
    }
    #[cfg(test)]
    pub(crate) fn fixture_capture(root: HKEY, installation: Arc<Directory>) -> io::Result<Self> {
        Self::observe(root, installation)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RetainedManifest {
    format: u32,
    control_root: FileIdentity,
    binding: JournalBinding,
    snapshot: Snapshot,
}
impl Snapshot {
    fn validate(&self, user: &CurrentUser) -> io::Result<()> {
        if self.schema != 2 || self.user_sid != user.sid_text() || self.trees.len() != SLOTS.len() {
            return Err(blocked("registration manifest binding differs"));
        }
        let mut budget = Budget::default();
        for (tree, slot) in self.trees.iter().zip(SLOTS) {
            if tree.slot != slot {
                return Err(blocked("registration manifest slot differs"));
            }
            if tree.parent.namespace.is_some() != tree.parent.security.is_some()
                || (!tree.nodes.is_empty() && tree.parent.namespace.is_none())
            {
                return Err(blocked("retained shared registry parent differs"));
            }
            if let Some(security) = &tree.parent.security {
                super::security::ValidatedDescriptor::from_bytes(security)?;
                budget.bytes(
                    security.len()
                        + tree
                            .parent
                            .namespace
                            .as_ref()
                            .map_or(0, |name| name.len() * 2),
                )?;
            }
            let mut names = std::collections::BTreeSet::new();
            for node in &tree.nodes {
                budget.keys += 1;
                if budget.keys > MAX_KEYS
                    || node.relative.len() > MAX_DEPTH
                    || !names.insert(node.relative.clone())
                {
                    return Err(blocked("registration manifest tree invalid"));
                }
                if node.relative.iter().any(|name| {
                    name.is_empty()
                        || name.encode_utf16().count() > 255
                        || name.contains(['\\', '/', '\0'])
                        || name.chars().any(char::is_control)
                }) {
                    return Err(blocked("unsupported retained registry component"));
                }
                if !node.relative.is_empty()
                    && !names.contains(&node.relative[..node.relative.len() - 1])
                {
                    return Err(blocked("retained registry parent missing"));
                }
                wide(&node.class)?;
                super::security::ValidatedDescriptor::from_bytes(&node.security)?
                    .require_assignable(user)?;
                if node
                    .values
                    .get("SymbolicLinkValue")
                    .is_some_and(|value| value.kind == REG_LINK.0)
                {
                    return Err(blocked("retained linked registration is unsupported"));
                }
                budget.bytes(node.security.len() + node.class.len() + node.namespace.len() * 2)?;
                for (name, value) in &node.values {
                    wide(name)?;
                    budget.values += 1;
                    if budget.values > MAX_VALUES || value.bytes.len() > MAX_VALUE {
                        return Err(blocked("registration manifest value limit"));
                    }
                    budget.bytes(name.len() + value.bytes.len())?;
                }
            }
            if tree
                .nodes
                .first()
                .is_some_and(|node| !node.relative.is_empty())
            {
                return Err(blocked("registration manifest root missing"));
            }
        }
        if self.run.namespace.is_some() != self.run.parent_security.is_some()
            || (self.run.value.is_some() && self.run.namespace.is_none())
        {
            return Err(blocked("retained Run parent differs"));
        }
        if let Some(security) = &self.run.parent_security {
            super::security::ValidatedDescriptor::from_bytes(security)?;
            budget.bytes(security.len())?;
        }
        budget.bytes(self.run.value.as_ref().map_or(0, |value| value.bytes.len()))?;
        if self
            .run
            .value
            .as_ref()
            .is_some_and(|value| value.bytes.len() > MAX_VALUE)
        {
            return Err(blocked("retained Run value limit"));
        }
        Ok(())
    }
    fn equivalent(&self, other: &Self) -> bool {
        self.user_sid == other.user_sid
            && self.installation == other.installation
            && self.run.value == other.run.value
            && ((self.run.parent_security.is_none() && self.run.value.is_none())
                || (other.run.parent_security.is_none() && other.run.value.is_none())
                || self.run.parent_security == other.run.parent_security)
            && self.trees.len() == other.trees.len()
            && self.trees.iter().zip(&other.trees).all(|(a, b)| {
                a.slot == b.slot
                    && (a.parent == b.parent
                        || (a.parent.namespace.is_none() && a.nodes.is_empty())
                        || (b.parent.namespace.is_none() && b.nodes.is_empty()))
                    && a.nodes.len() == b.nodes.len()
                    && a.nodes.iter().zip(&b.nodes).all(|(a, b)| {
                        a.relative == b.relative
                            && a.class == b.class
                            && a.security == b.security
                            && a.values == b.values
                    })
            })
    }
    fn tree_mut(&mut self, slot: RegistrationSlot) -> io::Result<&mut Tree> {
        self.trees
            .iter_mut()
            .find(|t| t.slot == slot)
            .ok_or_else(|| blocked("unknown registry slot"))
    }
}
impl HeldRegistrationState {
    pub(crate) fn retain(
        self,
        journal: &mut RegistrationJournal<'_>,
    ) -> io::Result<RetainedRegistrationState> {
        self.recheck()?;
        journal.require_phase(JournalPhase::Reviewed)?;
        self.snapshot.validate(&CurrentUser::capture()?)?;
        let manifest = RetainedManifest {
            format: 2,
            control_root: journal.root.directory().identity().clone(),
            binding: journal.binding.clone(),
            snapshot: self.snapshot.clone(),
        };
        let digest = journal.retain(&manifest)?;
        self.recheck()?;
        // Unknown source Run material remains privately retained but never
        // becomes a writable app-owned Registration role.
        verify_owned_run(manifest.snapshot.run.value.as_ref(), &self.installation)?;
        journal.append(JournalEvent::Manifest {
            role: ManifestRole::Registration,
            digest: digest.clone(),
        })?;
        self.recheck()?;
        let Self {
            root,
            installation,
            guards,
            ..
        } = self;
        drop(guards);
        Ok(RetainedRegistrationState {
            root,
            installation,
            manifest,
            digest,
        })
    }
}
/// This token only comes from complete live capture+retention or the exact
/// healthy journal's protected Registration role. There is no digest constructor.
pub(crate) struct RetainedRegistrationState {
    root: HKEY,
    installation: Arc<Directory>,
    manifest: RetainedManifest,
    digest: String,
}
impl RetainedRegistrationState {
    pub(crate) fn digest(&self) -> &str {
        &self.digest
    }
    pub(crate) fn reopen(
        journal: &mut RegistrationJournal<'_>,
        installation: Arc<Directory>,
    ) -> io::Result<Self> {
        CurrentUser::capture()?.require_unelevated()?;
        Self::reopen_at(journal, installation, HKEY_CURRENT_USER)
    }
    fn reopen_at(
        journal: &mut RegistrationJournal<'_>,
        installation: Arc<Directory>,
        root: HKEY,
    ) -> io::Result<Self> {
        journal.verify()?;
        installation.recheck()?;
        let digest = journal.role()?;
        let bytes = safe(journal.store.read_manifest(&digest))?;
        let manifest: RetainedManifest =
            serde_json::from_slice(&bytes).map_err(|_| blocked("invalid retained registration"))?;
        if manifest.format != 2
            || manifest.control_root != *journal.root.directory().identity()
            || manifest.binding != journal.binding
            || manifest.snapshot.installation != *installation.identity()
        {
            return Err(blocked("retained registration binding differs"));
        }
        manifest.snapshot.validate(&CurrentUser::capture()?)?;
        verify_owned_run(manifest.snapshot.run.value.as_ref(), &installation)?;
        if encode(&manifest)? != bytes {
            return Err(blocked("noncanonical retained registration"));
        }
        journal.verify()?;
        Ok(Self {
            root,
            installation,
            manifest,
            digest,
        })
    }
    #[cfg(test)]
    pub(crate) fn fixture_reopen(
        journal: &mut RegistrationJournal<'_>,
        installation: Arc<Directory>,
        root: HKEY,
    ) -> io::Result<Self> {
        Self::reopen_at(journal, installation, root)
    }
    fn verify(&self, journal: &mut RegistrationJournal<'_>) -> io::Result<()> {
        journal.verify()?;
        self.installation.recheck()?;
        if self.manifest.binding != journal.binding
            || self.manifest.control_root != *journal.root.directory().identity()
            || journal.role()? != self.digest
            || safe(journal.store.read_manifest(&self.digest))? != encode(&self.manifest)?
        {
            return Err(blocked("retained registration authority changed"));
        }
        self.manifest.snapshot.validate(&CurrentUser::capture()?)?;
        verify_owned_run(
            self.manifest.snapshot.run.value.as_ref(),
            &self.installation,
        )
    }
    /// Final source sealing check. All temporary registry keys are dropped
    /// before returning, while the journal retains the immutable original.
    pub(crate) fn verify_original(&self, journal: &mut RegistrationJournal<'_>) -> io::Result<()> {
        self.verify(journal)?;
        let observed = HeldRegistrationState::observe(self.root, self.installation.clone())?;
        if observed.snapshot != self.manifest.snapshot {
            return Err(blocked("original registration changed before sealing"));
        }
        observed.recheck()?;
        self.verify(journal)?;
        observed.recheck()
    }
}
fn safe<T>(result: Result<T, crate::cli::types::SafeError>) -> io::Result<T> {
    result.map_err(|_| blocked("registration journal unavailable"))
}
pub(crate) struct RegistrationJournal<'a> {
    store: &'a mut JournalStore,
    root: Arc<PrivateDirectory>,
    lease: &'a ExclusiveLease,
    binding: JournalBinding,
    generation: u64,
}
struct Pending {
    id: String,
    generation: u64,
}
impl<'a> RegistrationJournal<'a> {
    pub(crate) fn new(
        store: &'a mut JournalStore,
        root: Arc<PrivateDirectory>,
        lease: &'a ExclusiveLease,
        binding: JournalBinding,
        generation: u64,
    ) -> io::Result<Self> {
        let mut result = Self {
            store,
            root,
            lease,
            binding,
            generation,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }
    fn verify(&mut self) -> io::Result<()> {
        self.lease.verify_root(&self.root)?;
        self.root.verify(&CurrentUser::capture()?)?;
        safe(
            self.store
                .verify_windows_binding(&self.root, &self.binding, self.generation),
        )
    }
    fn state(&mut self) -> io::Result<crate::version_history::journal::SwitchJournal> {
        self.verify()?;
        let observed = safe(self.store.inspect(&self.binding))?;
        if observed.blocked {
            return Err(blocked("registration journal is blocked"));
        }
        observed
            .last_valid
            .ok_or_else(|| blocked("registration journal missing"))
    }
    fn require_phase(&mut self, phase: JournalPhase) -> io::Result<()> {
        let state = self.state()?;
        if state.phase() != phase || state.requires_reconciliation() {
            return Err(blocked("registration phase is unavailable"));
        }
        Ok(())
    }
    fn role(&mut self) -> io::Result<String> {
        self.state()?
            .manifest(ManifestRole::Registration)
            .map(str::to_owned)
            .ok_or_else(|| blocked("registration role missing"))
    }
    fn retain(&mut self, value: &impl Serialize) -> io::Result<String> {
        self.verify()?;
        let bytes = encode(value)?;
        let digest = safe(self.store.retain_manifest(&bytes))?;
        if safe(self.store.read_manifest(&digest))? != bytes {
            return Err(blocked("registration artifact changed"));
        }
        self.verify()?;
        Ok(digest)
    }
    fn append(&mut self, event: JournalEvent) -> io::Result<()> {
        self.verify()?;
        self.generation = safe(self.store.append(self.generation, event))?;
        self.verify()
    }
    fn begin(
        &mut self,
        slot: RegistrationSlot,
        manifest: &str,
        ordinal: &mut u32,
        operation: Option<RegistrationOperation>,
        before: &impl Serialize,
        expected: &impl Serialize,
    ) -> io::Result<Pending> {
        self.require_phase(JournalPhase::Restoring)?;
        let before = self.retain(before)?;
        let expected_postconditions = self.retain(expected)?;
        let id = restore_effect_id(&self.binding, *ordinal);
        let kind = match operation {
            Some(operation) => EffectKind::RecoveryRegistrationEntry {
                slot,
                operation,
                manifest: manifest.into(),
                entry_index: *ordinal,
            },
            None => EffectKind::VerifyRegistrationRestore { slot },
        };
        self.append(JournalEvent::Intent {
            effect: EffectSpec {
                effect_id: id.clone(),
                kind,
                before,
                expected_postconditions,
            },
        })?;
        *ordinal += 1;
        Ok(Pending {
            id,
            generation: self.generation,
        })
    }
    fn finish(
        &mut self,
        pending: Pending,
        result: io::Result<Snapshot>,
        root: HKEY,
        installation: &Directory,
    ) -> io::Result<Snapshot> {
        let observed = match result {
            Ok(value) => value,
            Err(error) => {
                let _ = self.append(JournalEvent::Observed {
                    effect_id: pending.id,
                    intent_generation: pending.generation,
                    result: ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    },
                });
                return Err(error);
            }
        };
        let digest = self.retain(&observed)?;
        let receipt = safe(self.store.retain_effect_receipt(
            &pending.id,
            Observation::Applied,
            &digest,
        ))?;
        // Artifact I/O may take time. Refresh actual namespace/data/security
        // after receipt retention and immediately before certifying Applied.
        let refreshed = (|| {
            installation.recheck()?;
            let (fresh, guards) =
                capture(root, &observed.user_sid, installation.identity().clone())?;
            if fresh != observed {
                self.retain(&fresh)?;
                return Err(blocked("registration changed before receipt publication"));
            }
            for guard in &guards {
                guard.verify()?;
            }
            Ok(guards)
        })();
        let guards = match refreshed {
            Ok(guards) => guards,
            Err(error) => {
                let _ = self.append(JournalEvent::Observed {
                    effect_id: pending.id,
                    intent_generation: pending.generation,
                    result: ObservedResult {
                        observation: Observation::Unknown,
                        receipt: None,
                    },
                });
                return Err(error);
            }
        };
        self.append(JournalEvent::Observed {
            effect_id: pending.id,
            intent_generation: pending.generation,
            result: ObservedResult {
                observation: Observation::Applied,
                receipt: Some(receipt),
            },
        })?;
        for guard in guards {
            guard.verify()?;
        }
        Ok(observed)
    }
}
fn restore_effect_id(binding: &JournalBinding, ordinal: u32) -> String {
    let bytes = encode(&(
        "cc-desk-registration-restore-v1",
        &binding.transaction_id,
        ordinal,
    ))
    .expect("bounded primitive binding");
    let hex = sha256(&bytes);
    let mut raw = [0u8; 16];
    for (index, byte) in raw.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).expect("sha256 hex");
    }
    raw[6] = (raw[6] & 15) | 64;
    raw[8] = (raw[8] & 63) | 128;
    uuid::Uuid::from_bytes(raw).to_string()
}

#[derive(Clone, Serialize)]
enum Mutation {
    RemoveValue {
        slot: RegistrationSlot,
        relative: Vec<String>,
        name: String,
    },
    RemoveKey {
        slot: RegistrationSlot,
        relative: Vec<String>,
    },
    CreateKey {
        slot: RegistrationSlot,
        relative: Vec<String>,
        class: String,
    },
    SetValue {
        slot: RegistrationSlot,
        relative: Vec<String>,
        name: String,
        value: RegistryValue,
    },
    SetSecurity {
        slot: RegistrationSlot,
        relative: Vec<String>,
        security: Vec<u8>,
    },
    SetRun {
        value: Option<RegistryValue>,
    },
}
impl Mutation {
    fn slot(&self) -> RegistrationSlot {
        match self {
            Self::RemoveValue { slot, .. }
            | Self::RemoveKey { slot, .. }
            | Self::CreateKey { slot, .. }
            | Self::SetValue { slot, .. }
            | Self::SetSecurity { slot, .. } => *slot,
            Self::SetRun { .. } => RegistrationSlot::OwnedRun,
        }
    }
    fn operation(&self) -> RegistrationOperation {
        match self {
            Self::RemoveValue { .. } | Self::SetRun { value: None } => {
                RegistrationOperation::RemoveOwnedValue
            }
            Self::RemoveKey { .. } => RegistrationOperation::RemoveOwnedKey,
            Self::CreateKey { .. } => RegistrationOperation::CreateKey,
            Self::SetValue { .. } | Self::SetRun { value: Some(_) } => {
                RegistrationOperation::SetValue
            }
            Self::SetSecurity { .. } => RegistrationOperation::SetPermissions,
        }
    }
    fn relative(&self) -> &[String] {
        match self {
            Self::RemoveValue { relative, .. }
            | Self::RemoveKey { relative, .. }
            | Self::CreateKey { relative, .. }
            | Self::SetValue { relative, .. }
            | Self::SetSecurity { relative, .. } => relative,
            Self::SetRun { .. } => &[],
        }
    }
}
fn material_equal(a: &Tree, b: &Tree) -> bool {
    a.slot == b.slot
        && a.nodes.len() == b.nodes.len()
        && a.nodes.iter().zip(&b.nodes).all(|(a, b)| {
            a.relative == b.relative
                && a.class == b.class
                && a.security == b.security
                && a.values == b.values
        })
}
fn plan(source: &Snapshot, current: &Snapshot) -> io::Result<Vec<Mutation>> {
    let mut result = Vec::new();
    for (source, current) in source.trees.iter().zip(&current.trees) {
        // Shared parent descriptors are restoration context, never writable
        // product state. Preserve changed current material, then stop before
        // a source unprotected DACL could inherit from a different parent.
        if source.parent.namespace.is_some() && source.parent != current.parent {
            return Err(blocked(
                "shared registry parent changed; retained conflict requires review",
            ));
        }
        if material_equal(source, current) {
            continue;
        }
        // These six roots are wholly product-owned. Unknown later contents were
        // retained first; each value/key removal still has its own intent/readback.
        for node in current.nodes.iter().rev() {
            for name in node.values.keys() {
                result.push(Mutation::RemoveValue {
                    slot: current.slot,
                    relative: node.relative.clone(),
                    name: name.clone(),
                });
            }
            result.push(Mutation::RemoveKey {
                slot: current.slot,
                relative: node.relative.clone(),
            });
        }
        for node in &source.nodes {
            result.push(Mutation::CreateKey {
                slot: source.slot,
                relative: node.relative.clone(),
                class: node.class.clone(),
            });
            for (name, value) in &node.values {
                result.push(Mutation::SetValue {
                    slot: source.slot,
                    relative: node.relative.clone(),
                    name: name.clone(),
                    value: value.clone(),
                });
            }
            // Restore the parent's descriptor before any children exist, so
            // inheritance cannot change an already restored child afterwards.
            result.push(Mutation::SetSecurity {
                slot: source.slot,
                relative: node.relative.clone(),
                security: node.security.clone(),
            });
        }
    }
    if source.run.parent_security.is_some()
        && source.run.parent_security != current.run.parent_security
    {
        return Err(blocked(
            "shared Run security changed; retained conflict requires review",
        ));
    }
    if source.run.value != current.run.value {
        if source.run.value.is_some() && current.run.namespace.is_none() {
            return Err(blocked("shared Run parent disappeared"));
        }
        result.push(Mutation::SetRun {
            value: source.run.value.clone(),
        });
    }
    if result.len() > 20_000 {
        return Err(blocked("registration restoration plan exceeds bound"));
    }
    Ok(result)
}
fn full_path(slot: RegistrationSlot, relative: &[String]) -> String {
    if relative.is_empty() {
        slot_path(slot).into()
    } else {
        format!("{}\\{}", slot_path(slot), relative.join("\\"))
    }
}
fn find_node<'a>(
    snapshot: &'a Snapshot,
    slot: RegistrationSlot,
    relative: &[String],
) -> io::Result<&'a Node> {
    snapshot
        .trees
        .iter()
        .find(|tree| tree.slot == slot)
        .and_then(|tree| tree.nodes.iter().find(|node| node.relative == relative))
        .ok_or_else(|| blocked("expected registry node missing"))
}
fn require_source_access(root: HKEY, source: &Snapshot) -> io::Result<()> {
    for tree in &source.trees {
        for node in &tree.nodes {
            let selected = open_path(
                root,
                &full_path(tree.slot, &node.relative),
                &source.user_sid,
                KEY_ALL_ACCESS,
            )?;
            let key = selected
                .leaf
                .as_ref()
                .ok_or_else(|| blocked("source registry node disappeared"))?;
            verify_node(key.0, node)?;
            for guard in &selected.guards {
                guard.verify()?;
            }
        }
    }
    Ok(())
}
fn preflight_restore(root: HKEY, current: &Snapshot, operations: &[Mutation]) -> io::Result<()> {
    for mutation in operations {
        if matches!(mutation,Mutation::CreateKey{relative,..} if relative.is_empty()) {
            let path = slot_path(mutation.slot());
            let (parent, _) = path
                .rsplit_once('\\')
                .ok_or_else(|| blocked("shared registry parent missing"))?;
            if open_path(
                root,
                parent,
                &current.user_sid,
                KEY_READ | KEY_CREATE_SUB_KEY,
            )?
            .leaf
            .is_none()
            {
                return Err(blocked("shared registry parent cannot be recreated"));
            }
        }
    }
    if operations
        .iter()
        .any(|operation| matches!(operation, Mutation::SetRun { .. }))
    {
        let run = open_path(root, RUN_PATH, &current.user_sid, KEY_READ | KEY_SET_VALUE)?;
        if run.leaf.is_none() {
            return Err(blocked("owned Run target unavailable"));
        }
        for guard in &run.guards {
            guard.verify()?;
        }
    }
    // Refuse known access conflicts before removing any product data. The
    // complete current bytes/permissions have already been retained privately.
    require_source_access(root, current)
}
fn verify_node(key: HKEY, expected: &Node) -> io::Result<()> {
    let mut budget = Budget::default();
    if current_key_name(key)? != expected.namespace
        || descriptor(key)? != expected.security
        || key_class(key)? != expected.class
        || values(key, &mut budget)? != expected.values
    {
        return Err(blocked("registry before state changed"));
    }
    Ok(())
}
fn flush(key: HKEY) -> io::Result<()> {
    probe(RegistrationFault::AfterMutation)?;
    unsafe { RegFlushKey(key) }.ok().map_err(win_error)?;
    probe(RegistrationFault::AfterFlush)
}
fn mutate(root: HKEY, before: &Snapshot, mutation: &Mutation) -> io::Result<()> {
    let path = full_path(mutation.slot(), mutation.relative());
    if let Mutation::CreateKey { class, .. } = mutation {
        let (parent_path, name) = path
            .rsplit_once('\\')
            .ok_or_else(|| blocked("missing registry parent"))?;
        let parent = open_path(
            root,
            parent_path,
            &before.user_sid,
            KEY_READ | KEY_CREATE_SUB_KEY,
        )?;
        let parent_key = parent
            .leaf
            .as_ref()
            .ok_or_else(|| blocked("shared registry parent disappeared"))?;
        if open_one(parent_key.0, name, KEY_READ)?.is_some() {
            return Err(blocked("registry create destination appeared"));
        }
        for guard in &parent.guards {
            guard.verify()?;
        }
        let name = wide(name)?;
        let class = wide(class)?;
        let mut raw = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        unsafe {
            RegCreateKeyExW(
                parent_key.0,
                PCWSTR(name.as_ptr()),
                None,
                PCWSTR(class.as_ptr()),
                REG_OPTION_NON_VOLATILE,
                KEY_ALL_ACCESS | KEY_WOW64_64KEY,
                None,
                &mut raw,
                Some(&mut disposition),
            )
            .ok()
            .map_err(win_error)?;
        }
        let key = Key(raw);
        if disposition != REG_CREATED_NEW_KEY {
            return Err(blocked("registry create collided with existing key"));
        }
        flush(key.0)?;
        return Ok(());
    }
    let access = if mutation.slot() == RegistrationSlot::OwnedRun {
        KEY_READ | KEY_SET_VALUE
    } else {
        KEY_ALL_ACCESS
    };
    let selected = open_path(root, &path, &before.user_sid, access)?;
    let key = selected
        .leaf
        .as_ref()
        .ok_or_else(|| blocked("registry mutation target disappeared"))?;
    for guard in &selected.guards {
        guard.verify()?;
    }
    if mutation.slot() == RegistrationSlot::OwnedRun {
        if current_key_name(key.0)?
            != *before
                .run
                .namespace
                .as_ref()
                .ok_or_else(|| blocked("Run parent missing"))?
            || Some(descriptor(key.0)?) != before.run.parent_security
            || read_value_bounded(key.0, RUN_NAME, MAX_VALUE)? != before.run.value
        {
            return Err(blocked("owned Run before state changed"));
        }
    } else {
        verify_node(
            key.0,
            find_node(before, mutation.slot(), mutation.relative())?,
        )?;
    }
    match mutation {
        Mutation::RemoveValue { name, .. } => {
            let name = wide(name)?;
            unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) }
                .ok()
                .map_err(win_error)?;
            flush(key.0)?;
        }
        Mutation::SetValue { name, value, .. } => {
            let name = wide(name)?;
            unsafe {
                RegSetValueExW(
                    key.0,
                    PCWSTR(name.as_ptr()),
                    None,
                    REG_VALUE_TYPE(value.kind),
                    Some(&value.bytes),
                )
            }
            .ok()
            .map_err(win_error)?;
            flush(key.0)?;
        }
        Mutation::SetRun { value } => {
            let name = wide(RUN_NAME)?;
            unsafe {
                match value {
                    Some(value) => RegSetValueExW(
                        key.0,
                        PCWSTR(name.as_ptr()),
                        None,
                        REG_VALUE_TYPE(value.kind),
                        Some(&value.bytes),
                    ),
                    None => RegDeleteValueW(key.0, PCWSTR(name.as_ptr())),
                }
            }
            .ok()
            .map_err(win_error)?;
            flush(key.0)?;
        }
        Mutation::SetSecurity { security, .. } => {
            let descriptor = super::security::ValidatedDescriptor::from_bytes(security)?;
            unsafe { RegSetKeySecurity(key.0, descriptor.information(), descriptor.raw()) }
                .ok()
                .map_err(win_error)?;
            flush(key.0)?;
        }
        Mutation::RemoveKey { .. } => {
            if !enum_subkeys(key.0)?.is_empty()
                || !values(key.0, &mut Budget::default())?.is_empty()
            {
                return Err(blocked("registry key became nonempty"));
            }
            let (parent_path, _) = path
                .rsplit_once('\\')
                .ok_or_else(|| blocked("registry parent missing"))?;
            let parent = open_path(root, parent_path, &before.user_sid, KEY_READ)?;
            let parent_key = parent
                .leaf
                .ok_or_else(|| blocked("registry parent disappeared"))?;
            let status = unsafe {
                windows::Wdk::System::Registry::NtDeleteKey(windows::Win32::Foundation::HANDLE(
                    key.0 .0,
                ))
            };
            if status.0 != 0 {
                return Err(blocked("registry key removal outcome unavailable"));
            }
            // Delete the exact opened key object, never a later object found by its name.
            drop(selected);
            flush(parent_key.0)?;
        }
        Mutation::CreateKey { .. } => unreachable!(),
    }
    Ok(())
}
fn verify_change(before: &Snapshot, after: &Snapshot, mutation: &Mutation) -> io::Result<()> {
    let mut expected = before.clone();
    match mutation {
        Mutation::SetRun { value } => expected.run.value = value.clone(),
        Mutation::CreateKey {
            slot,
            relative,
            class,
        } => {
            if expected
                .trees
                .iter()
                .find(|tree| tree.slot == *slot)
                .is_none_or(|tree| tree.nodes.iter().any(|node| node.relative == *relative))
            {
                return Err(blocked("registry creation precondition invalid"));
            }
            let observed = find_node(after, *slot, relative)?;
            if observed.class != *class || !observed.values.is_empty() {
                return Err(blocked("created registry key differs"));
            }
            let tree = expected.tree_mut(*slot)?;
            tree.nodes.push(observed.clone());
            tree.nodes.sort_by(|a, b| a.relative.cmp(&b.relative));
        }
        Mutation::RemoveKey { slot, relative } => {
            let tree = expected.tree_mut(*slot)?;
            let length = tree.nodes.len();
            tree.nodes.retain(|node| node.relative != *relative);
            if tree.nodes.len() + 1 != length {
                return Err(blocked("removed registry key not observed"));
            }
        }
        Mutation::RemoveValue {
            slot,
            relative,
            name,
        } => {
            let node = expected
                .tree_mut(*slot)?
                .nodes
                .iter_mut()
                .find(|node| node.relative == *relative)
                .ok_or_else(|| blocked("registry value key missing"))?;
            if node.values.remove(name).is_none() {
                return Err(blocked("removed registry value not observed"));
            }
        }
        Mutation::SetValue {
            slot,
            relative,
            name,
            value,
        } => {
            expected
                .tree_mut(*slot)?
                .nodes
                .iter_mut()
                .find(|node| node.relative == *relative)
                .ok_or_else(|| blocked("registry value key missing"))?
                .values
                .insert(name.clone(), value.clone());
        }
        Mutation::SetSecurity {
            slot,
            relative,
            security,
        } => {
            expected
                .tree_mut(*slot)?
                .nodes
                .iter_mut()
                .find(|node| node.relative == *relative)
                .ok_or_else(|| blocked("registry security key missing"))?
                .security = security.clone();
        }
    }
    if expected != *after {
        return Err(blocked("registry postcondition is unknown"));
    }
    Ok(())
}
impl RetainedRegistrationState {
    pub(crate) fn restore(
        &self,
        journal: &mut RegistrationJournal<'_>,
    ) -> io::Result<RestoredRegistrationReceipt> {
        self.verify(journal)?;
        journal.require_phase(JournalPhase::Restoring)?;
        let (mut current, guards) = capture(
            self.root,
            &self.manifest.snapshot.user_sid,
            self.installation.identity().clone(),
        )?;
        let binding = journal.binding.clone();
        let preserved = journal.retain(&(&binding, &self.digest, &current))?;
        verify_owned_run(current.run.value.as_ref(), &self.installation)?;
        let operations = plan(&self.manifest.snapshot, &current)?;
        preflight_restore(self.root, &current, &operations)?;
        let effects = operations
            .len()
            .checked_add(SLOTS.len() + 1)
            .ok_or_else(|| blocked("registry restore budget overflow"))?;
        let artifacts = effects
            .checked_mul(4)
            .ok_or_else(|| blocked("registry artifact budget overflow"))?;
        journal.verify()?;
        safe(
            journal
                .store
                .admit_restore_capacity(journal.generation, effects as u64, artifacts),
        )?;
        journal.verify()?;
        for guard in &guards {
            guard.verify()?;
        }
        drop(guards);
        // One ordinal sequence for the entire registration restore. A reopened
        // or replanned invocation always starts at zero, even if a different
        // slot would be its first remaining operation; it cannot replay work.
        let mut ordinal = 0;
        for mutation in operations {
            self.verify(journal)?;
            let (fresh, guards) = capture(
                self.root,
                &current.user_sid,
                self.installation.identity().clone(),
            )?;
            if fresh != current {
                journal.retain(&(&binding, &self.digest, &fresh))?;
                return Err(blocked("registry conflict retained before mutation"));
            }
            for guard in &guards {
                guard.verify()?;
            }
            drop(guards);
            let pending = journal.begin(
                mutation.slot(),
                &self.digest,
                &mut ordinal,
                Some(mutation.operation()),
                &(&preserved, &current),
                &mutation,
            )?;
            let result = (|| {
                mutate(self.root, &current, &mutation)?;
                let (after, guards) = capture(
                    self.root,
                    &current.user_sid,
                    self.installation.identity().clone(),
                )?;
                // Preserve any independently changed actual data even when it invalidates
                // our postcondition. An uncertain effect is never replayed or rolled back.
                let _retained = journal.retain(&after)?;
                verify_change(&current, &after, &mutation)?;
                for guard in guards {
                    guard.verify()?;
                }
                Ok(after)
            })();
            current = journal.finish(pending, result, self.root, &self.installation)?;
        }
        if !current.equivalent(&self.manifest.snapshot) {
            return Err(blocked("registration source restoration differs"));
        }
        for slot in SLOTS.into_iter().chain([RegistrationSlot::OwnedRun]) {
            self.verify(journal)?;
            let pending = journal.begin(
                slot,
                &self.digest,
                &mut ordinal,
                None,
                &(&preserved, &current),
                &self.manifest.snapshot,
            )?;
            let result = (|| {
                let (after, guards) = capture(
                    self.root,
                    &current.user_sid,
                    self.installation.identity().clone(),
                )?;
                if after != current || !after.equivalent(&self.manifest.snapshot) {
                    return Err(blocked("final registration observation changed"));
                }
                for guard in guards {
                    guard.verify()?;
                }
                Ok(after)
            })();
            current = journal.finish(pending, result, self.root, &self.installation)?;
        }
        let (observed, guards) = capture(
            self.root,
            &current.user_sid,
            self.installation.identity().clone(),
        )?;
        if observed != current {
            return Err(blocked("restored registration changed after receipt"));
        }
        let held = HeldRegistrationState {
            root: self.root,
            installation: self.installation.clone(),
            snapshot: observed,
            guards,
        };
        let receipt = RestoredRegistrationReceipt {
            held,
            source: self.manifest.snapshot.clone(),
            source_manifest: self.digest.clone(),
            preserved,
        };
        receipt.verify()?;
        Ok(receipt)
    }
}
pub(crate) struct RestoredRegistrationReceipt {
    held: HeldRegistrationState,
    source: Snapshot,
    source_manifest: String,
    preserved: String,
}
impl RestoredRegistrationReceipt {
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.held.recheck()?;
        if !self.held.snapshot.equivalent(&self.source) {
            return Err(blocked("restored registration evidence differs"));
        }
        Ok(())
    }
    pub(crate) fn preserved_current(&self) -> &str {
        &self.preserved
    }
    pub(crate) fn source_manifest(&self) -> &str {
        &self.source_manifest
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistrationFault {
    AfterMutation,
    AfterFlush,
}
#[cfg(test)]
thread_local! {static FAULT:std::cell::Cell<Option<RegistrationFault>>=const{std::cell::Cell::new(None)};}
fn probe(point: RegistrationFault) -> io::Result<()> {
    #[cfg(test)]
    if FAULT.with(|fault| {
        if fault.get() == Some(point) {
            fault.set(None);
            true
        } else {
            false
        }
    }) {
        return Err(io::Error::other("injected registry boundary"));
    }
    #[cfg(not(test))]
    let _ = point;
    Ok(())
}
#[cfg(test)]
pub(crate) struct RegistrationProbe(std::marker::PhantomData<std::rc::Rc<()>>);
#[cfg(test)]
impl Drop for RegistrationProbe {
    fn drop(&mut self) {
        FAULT.with(|fault| fault.set(None));
    }
}
#[cfg(test)]
pub(crate) fn probe_registration_fault(fault: RegistrationFault) -> RegistrationProbe {
    FAULT.with(|current| {
        assert!(current.replace(Some(fault)).is_none());
    });
    RegistrationProbe(std::marker::PhantomData)
}
