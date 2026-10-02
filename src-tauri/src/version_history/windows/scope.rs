//! Read-only registered-install and configured-root observations. These retain
//! real objects, but do not mint snapshot, installer or effective-CLI authority.
use super::{
    files::{ChildEntry, ComponentName, Directory, FileAccess, PinnedFile},
    security::CurrentUser,
};
use super::{
    process::ExactProcess,
    registry::{InstallHive, InstallRecord, InstallRegistryObservation, RegistryValue},
};
use crate::cli::{
    environment::{build_environment, lookup, EnvMap},
    native_projection::selection::locations,
    profiles::{Launcher, Override, Profile},
    storage::{decode_workspace, WorkspaceDocument},
    types::CliKind,
};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value};
use std::{
    cell::Cell,
    collections::BTreeMap,
    ffi::OsStr,
    fmt,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    os::windows::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};

#[path = "scope_context.rs"]
mod context_inventory;
pub(crate) use context_inventory::{ConfiguredExclusions, ContextConfiguredInventory};

const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SELECTORS: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeBlock {
    Elevated,
    UnsupportedArchitecture,
    Unregistered,
    CompetingInstallation,
    UnsupportedRegistration,
    RegistrationChanged,
    Relocated,
    ImageUnsupported,
    InputUnavailable,
    InputMalformed,
    InputLimit,
    InputChanged,
    ConfiguredScopeUnknown,
    PathUnsupported,
    Overlap,
    UnsupportedSchema,
}
type ScopeResult<T> = Result<T, ScopeBlock>;

/// Reject duplicate object keys before Value can silently discard them. Unknown
/// fields are retained for decoding, not denied merely because they are new.
#[derive(Clone, Copy)]
struct StrictValue<'a> {
    limit: &'a Cell<bool>,
}
impl<'de> DeserializeSeed<'de> for StrictValue<'_> {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        struct StrictVisitor<'a>(StrictValue<'a>);
        impl<'de> Visitor<'de> for StrictVisitor<'_> {
            type Value = Value;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded JSON")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(value.into())
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(value.into())
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(value.into())
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(Value::Number)
                    .ok_or_else(|| E::custom("unsupported number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(value.into())
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(value.into())
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(Value::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                self.visit_unit()
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut source: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = source.next_element_seed(self.0)? {
                    if values.len() >= MAX_SELECTORS {
                        self.0.limit.set(true);
                        return Err(de::Error::custom("collection limit"));
                    }
                    values.push(value);
                }
                Ok(Value::Array(values))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut source: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = source.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                    if values.len() >= MAX_SELECTORS {
                        self.0.limit.set(true);
                        return Err(de::Error::custom("collection limit"));
                    }
                    values.insert(key, source.next_value_seed(self.0)?);
                }
                Ok(Value::Object(values))
            }
        }
        deserializer.deserialize_any(StrictVisitor(self))
    }
}
fn strict_json(bytes: &[u8], maximum: usize) -> ScopeResult<Value> {
    if bytes.len() > maximum {
        return Err(ScopeBlock::InputLimit);
    }
    let limit = Cell::new(false);
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue { limit: &limit }
        .deserialize(&mut decoder)
        .map_err(|_| {
            if limit.get() {
                ScopeBlock::InputLimit
            } else {
                ScopeBlock::InputMalformed
            }
        })?;
    decoder.end().map_err(|_| ScopeBlock::InputMalformed)?;
    if !value.is_object() {
        return Err(ScopeBlock::InputMalformed);
    }
    Ok(value)
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigScopeInputs {
    claude_path: Option<String>,
    git_bash_path: Option<String>,
    claude_env_vars: Option<BTreeMap<String, String>>,
    default_custom_args: Option<String>,
    hidden_projects: Option<Vec<String>>,
    last_opened_project: Option<String>,
    #[serde(skip)]
    raw: Value,
}
impl ConfigScopeInputs {
    pub(crate) fn decode(bytes: &[u8]) -> ScopeResult<Self> {
        let raw = strict_json(bytes, MAX_CONFIG_BYTES)?;
        let mut result: Self =
            serde_json::from_value(raw.clone()).map_err(|_| ScopeBlock::InputMalformed)?;
        result.raw = raw;
        Ok(result)
    }
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectsScopeInputs {
    #[serde(default)]
    pinned_projects: Vec<String>,
    #[serde(default)]
    archived_sessions: BTreeMap<String, Vec<String>>,
    display_names: Option<BTreeMap<String, String>>,
    session_records: Option<BTreeMap<String, crate::store::SessionUiRecord>>,
    launch_preferences: Option<BTreeMap<String, crate::store::ProjectLaunchPreference>>,
}
impl ProjectsScopeInputs {
    pub(crate) fn decode(bytes: &[u8]) -> ScopeResult<Self> {
        let result: Self = serde_json::from_value(strict_json(bytes, MAX_INPUT_BYTES)?)
            .map_err(|_| ScopeBlock::InputMalformed)?;
        for (key, value) in result.session_records.iter().flatten() {
            crate::store::validate_session_record_key(key)
                .map_err(|_| ScopeBlock::InputMalformed)?;
            crate::store::validate_session_ui_record(value)
                .map_err(|_| ScopeBlock::InputMalformed)?;
        }
        for value in result
            .launch_preferences
            .iter()
            .flat_map(|values| values.values())
        {
            crate::store::validate_project_launch_preference(value)
                .map_err(|_| ScopeBlock::InputMalformed)?;
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathKind {
    Directory,
    File,
}
pub(crate) struct PathCandidate {
    path: PathBuf,
    kind: PathKind,
}
impl PathCandidate {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn kind(&self) -> PathKind {
        self.kind
    }
}
fn drive_path(path: &Path) -> ScopeResult<PathBuf> {
    let value = path.to_str().ok_or(ScopeBlock::PathUnsupported)?;
    // fs::canonicalize stores this exact drive-prefix spelling in native
    // registration records. Other device/UNC namespaces remain unsupported.
    let value = value.strip_prefix(r"\\?\").unwrap_or(value);
    let path = PathBuf::from(value);
    super::files::validate_absolute(&path.as_os_str().encode_wide().collect::<Vec<_>>())
        .map_err(|_| ScopeBlock::PathUnsupported)?;
    Ok(path)
}
fn add_candidate(result: &mut Vec<PathCandidate>, path: &Path, kind: PathKind) -> ScopeResult<()> {
    let path = drive_path(path)?;
    if result
        .iter()
        .any(|candidate| candidate.kind == kind && candidate.path == path)
    {
        return Ok(());
    }
    if result.len() >= MAX_SELECTORS {
        return Err(ScopeBlock::InputLimit);
    }
    result.push(PathCandidate { path, kind });
    Ok(())
}
fn add_locations(result: &mut Vec<PathCandidate>, cli: CliKind, env: &EnvMap) -> ScopeResult<()> {
    if lookup(env, OsStr::new("BASH_ENV")).is_some_and(|value| !value.is_empty()) {
        return Err(ScopeBlock::ConfiguredScopeUnknown);
    }
    if let Some(path) =
        lookup(env, OsStr::new("CLAUDE_CODE_GIT_BASH_PATH")).filter(|path| !path.is_empty())
    {
        add_candidate(result, Path::new(path), PathKind::File)?;
    }
    let locations = locations(cli, env).map_err(|_| ScopeBlock::ConfiguredScopeUnknown)?;
    add_candidate(result, &locations.root, PathKind::Directory)?;
    if let Some(parent) = locations.user_config {
        add_candidate(result, &parent.join(".claude.json"), PathKind::File)?;
    }
    Ok(())
}
/// A static configured-input inventory. It never claims the effective behavior
/// of arbitrary future CLI/shell code, and never executes a profile or script.
pub(crate) fn configured_candidates(
    home: &Path,
    inherited: &EnvMap,
    workspace: &WorkspaceDocument,
    config: &ConfigScopeInputs,
    projects: &ProjectsScopeInputs,
) -> ScopeResult<Vec<PathCandidate>> {
    let mut result = Vec::new();
    for cli in [CliKind::Claude, CliKind::Codex] {
        let profile = Profile::new("scope-observation", cli);
        let env = build_environment(inherited, &EnvMap::new(), &profile, None, None)
            .map_err(|_| ScopeBlock::ConfiguredScopeUnknown)?;
        add_locations(&mut result, cli, &env)?;
    }
    if config
        .default_custom_args
        .as_ref()
        .is_some_and(|value| !value.is_empty())
    {
        return Err(ScopeBlock::ConfiguredScopeUnknown);
    }
    let legacy = Profile::new("legacyClaude", CliKind::Claude);
    let env = build_environment(inherited, &EnvMap::new(), &legacy, Some(&config.raw), None)
        .map_err(|_| ScopeBlock::ConfiguredScopeUnknown)?;
    add_locations(&mut result, CliKind::Claude, &env)?;
    add_candidate(&mut result, &home.join(".claude"), PathKind::Directory)?;
    add_candidate(&mut result, &home.join(".claude.json"), PathKind::File)?;
    for profile in workspace.profiles.values() {
        if profile.cli == CliKind::Shell
            || !matches!(profile.launcher, Launcher::Native)
            || matches!(&profile.default_args, Override::Set(args) if !args.is_empty())
        {
            return Err(ScopeBlock::ConfiguredScopeUnknown);
        }
        let env = build_environment(inherited, &EnvMap::new(), profile, Some(&config.raw), None)
            .map_err(|_| ScopeBlock::ConfiguredScopeUnknown)?;
        add_locations(&mut result, profile.cli, &env)?;
        if let Override::Set(path) = &profile.program_path {
            add_candidate(&mut result, Path::new(path), PathKind::File)?;
        }
    }
    for path in [
        config.claude_path.as_deref(),
        config.git_bash_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|path| !path.is_empty())
    {
        add_candidate(&mut result, Path::new(path), PathKind::File)?;
    }
    for project in workspace.registered_projects.values() {
        add_candidate(&mut result, &project.selected_path, PathKind::Directory)?;
        if let Some(path) = &project.canonical_path {
            add_candidate(&mut result, path, PathKind::Directory)?;
        }
    }
    for path in config
        .hidden_projects
        .iter()
        .flatten()
        .chain(config.last_opened_project.iter())
        .chain(projects.pinned_projects.iter())
        .chain(projects.archived_sessions.keys())
        .chain(
            projects
                .display_names
                .iter()
                .flat_map(|values| values.keys()),
        )
        .chain(
            projects
                .launch_preferences
                .iter()
                .flat_map(|values| values.keys()),
        )
    {
        add_candidate(&mut result, Path::new(path), PathKind::Directory)?;
    }
    for record in projects
        .session_records
        .iter()
        .flat_map(|values| values.values())
    {
        add_candidate(
            &mut result,
            Path::new(&record.project_path),
            PathKind::Directory,
        )?;
    }
    Ok(result)
}

enum PathObject {
    Directory(Arc<Directory>),
    File(PinnedFile),
    Absent {
        parent: Arc<Directory>,
        suffix: Vec<ComponentName>,
    },
}
pub(crate) struct ObservedPath {
    object: PathObject,
    kind: PathKind,
}
impl ObservedPath {
    pub(crate) fn from_directory(directory: Arc<Directory>) -> ScopeResult<Self> {
        directory.recheck().map_err(|_| ScopeBlock::InputChanged)?;
        Ok(Self {
            object: PathObject::Directory(directory),
            kind: PathKind::Directory,
        })
    }
    pub(crate) fn directory(&self) -> Option<&Arc<Directory>> {
        match &self.object {
            PathObject::Directory(directory) => Some(directory),
            _ => None,
        }
    }
    pub(crate) fn absent_location(&self) -> Option<(&Arc<Directory>, &[ComponentName])> {
        match &self.object {
            PathObject::Absent { parent, suffix } => Some((parent, suffix)),
            _ => None,
        }
    }
    pub(crate) fn observe(path: &Path, kind: PathKind) -> ScopeResult<Self> {
        let path = drive_path(path)?;
        let text = path.to_str().ok_or(ScopeBlock::PathUnsupported)?;
        let mut parent = Directory::open_absolute(Path::new(&text[..3]))
            .map_err(|_| ScopeBlock::InputUnavailable)?;
        let components = path
            .components()
            .filter_map(|part| match part {
                Component::Normal(name) => Some(ComponentName::new(name)),
                _ => None,
            })
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| ScopeBlock::PathUnsupported)?;
        if components.is_empty() {
            return if kind == PathKind::Directory {
                Ok(Self {
                    object: PathObject::Directory(parent),
                    kind,
                })
            } else {
                Err(ScopeBlock::PathUnsupported)
            };
        }
        for (index, component) in components.iter().enumerate() {
            if kind == PathKind::File && index + 1 == components.len() {
                return match parent.open_file(component.clone(), FileAccess::Read) {
                    Ok(file) => Ok(Self {
                        object: PathObject::File(file),
                        kind,
                    }),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                        object: PathObject::Absent {
                            parent,
                            suffix: vec![component.clone()],
                        },
                        kind,
                    }),
                    Err(_) => Err(ScopeBlock::InputUnavailable),
                };
            }
            match parent.open_directory(component.clone()) {
                Ok(next) => parent = next,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Self {
                        object: PathObject::Absent {
                            parent,
                            suffix: components[index..].to_vec(),
                        },
                        kind,
                    })
                }
                Err(_) => return Err(ScopeBlock::InputUnavailable),
            }
        }
        Ok(Self {
            object: PathObject::Directory(parent),
            kind,
        })
    }
    pub(crate) fn recheck(&self) -> ScopeResult<()> {
        match &self.object {
            PathObject::Directory(directory) => {
                directory.recheck().map_err(|_| ScopeBlock::InputChanged)
            }
            PathObject::File(file) => file.verify().map_err(|_| ScopeBlock::InputChanged),
            PathObject::Absent { parent, suffix } => {
                parent.recheck().map_err(|_| ScopeBlock::InputChanged)?;
                let result = if suffix.len() == 1 && self.kind == PathKind::File {
                    parent
                        .open_file(suffix[0].clone(), FileAccess::Read)
                        .map(|_| ())
                } else {
                    parent.open_directory(suffix[0].clone()).map(|_| ())
                };
                match result {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    _ => Err(ScopeBlock::InputChanged),
                }
            }
        }
    }
    fn components(&self) -> ScopeResult<Vec<Vec<u16>>> {
        self.recheck()?;
        let (base, tail) = match &self.object {
            PathObject::Directory(directory) => (
                directory.path().map_err(|_| ScopeBlock::InputChanged)?,
                vec![],
            ),
            PathObject::File(file) => (file.path().map_err(|_| ScopeBlock::InputChanged)?, vec![]),
            PathObject::Absent { parent, suffix } => (
                parent.path().map_err(|_| ScopeBlock::InputChanged)?,
                suffix.clone(),
            ),
        };
        let mut parts: Vec<Vec<u16>> = base
            .encode_wide()
            .collect::<Vec<_>>()
            .split(|unit| *unit == b'\\' as u16)
            .filter(|part| !part.is_empty())
            .map(Vec::from)
            .collect();
        parts.extend(
            tail.iter()
                .map(|name| name.os_string().encode_wide().collect()),
        );
        Ok(parts)
    }
    pub(crate) fn overlaps(&self, other: &Self) -> ScopeResult<bool> {
        let a = self.components()?;
        let b = other.components()?;
        for (left, right) in a.iter().zip(&b) {
            let comparison = unsafe { CompareStringOrdinal(left, right, true) };
            if comparison.0 == 0 {
                return Err(ScopeBlock::PathUnsupported);
            }
            if comparison != CSTR_EQUAL {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

struct InputFile {
    observation: ObservedPath,
    bytes: Option<Vec<u8>>,
}
impl InputFile {
    fn capture(path: &Path, maximum: usize) -> ScopeResult<Self> {
        let observation = ObservedPath::observe(path, PathKind::File)?;
        let bytes = match &observation.object {
            PathObject::File(file) => Some(read_file(file, maximum)?),
            PathObject::Absent { .. } => None,
            PathObject::Directory(_) => return Err(ScopeBlock::InputMalformed),
        };
        Ok(Self { observation, bytes })
    }
    fn recheck(&self) -> ScopeResult<()> {
        self.observation.recheck()?;
        if let (PathObject::File(file), Some(bytes)) = (&self.observation.object, &self.bytes) {
            if &read_file(file, bytes.len())? != bytes {
                return Err(ScopeBlock::InputChanged);
            }
        }
        Ok(())
    }
}
fn read_file(file: &PinnedFile, maximum: usize) -> ScopeResult<Vec<u8>> {
    file.verify().map_err(|_| ScopeBlock::InputChanged)?;
    if file
        .file
        .metadata()
        .map_err(|_| ScopeBlock::InputUnavailable)?
        .len()
        > maximum as u64
    {
        return Err(ScopeBlock::InputLimit);
    }
    let mut source = &file.file;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| ScopeBlock::InputUnavailable)?;
    let mut bytes = Vec::new();
    source
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ScopeBlock::InputUnavailable)?;
    if bytes.len() > maximum {
        return Err(ScopeBlock::InputLimit);
    }
    file.verify().map_err(|_| ScopeBlock::InputChanged)?;
    Ok(bytes)
}
pub(crate) struct ScopeInputs {
    desk: ObservedPath,
    files: Vec<InputFile>,
    workspace: WorkspaceDocument,
    config: ConfigScopeInputs,
    projects: ProjectsScopeInputs,
}
impl ScopeInputs {
    pub(crate) fn capture(desk: &Path) -> ScopeResult<Self> {
        let observation = ObservedPath::observe(desk, PathKind::Directory)?;
        let files = [
            InputFile::capture(&desk.join("cli-workspace.v1.json"), MAX_INPUT_BYTES)?,
            InputFile::capture(&desk.join("config.json"), MAX_CONFIG_BYTES)?,
            InputFile::capture(&desk.join("projects.json"), MAX_INPUT_BYTES)?,
        ];
        let workspace = if let Some(bytes) = &files[0].bytes {
            let raw = strict_json(bytes, MAX_INPUT_BYTES)?;
            decode_workspace(&serde_json::to_vec(&raw).map_err(|_| ScopeBlock::InputMalformed)?)
                .map_err(|error| {
                    if error.code == "UNSUPPORTED_SCHEMA" {
                        ScopeBlock::UnsupportedSchema
                    } else {
                        ScopeBlock::InputMalformed
                    }
                })?
        } else {
            WorkspaceDocument::default()
        };
        let config = ConfigScopeInputs::decode(files[1].bytes.as_deref().unwrap_or(b"{}"))?;
        let projects = ProjectsScopeInputs::decode(files[2].bytes.as_deref().unwrap_or(b"{}"))?;
        let result = Self {
            desk: observation,
            files: files.into_iter().collect(),
            workspace,
            config,
            projects,
        };
        result.recheck()?;
        Ok(result)
    }
    pub(crate) fn recheck(&self) -> ScopeResult<()> {
        self.desk.recheck()?;
        for file in &self.files {
            file.recheck()?;
        }
        validate_project_locations(&self.workspace)?;
        Ok(())
    }
}

fn validate_project_locations(workspace: &WorkspaceDocument) -> ScopeResult<()> {
    use windows::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    for project in workspace.registered_projects.values() {
        let selected = ObservedPath::observe(&project.selected_path, PathKind::Directory)?;
        if let Some(directory) = selected.directory() {
            if crate::cli::source_scope::is_verified_key(&project.source_path_key) {
                let mut info = BY_HANDLE_FILE_INFORMATION::default();
                unsafe {
                    GetFileInformationByHandle(directory.raw(), &mut info)
                        .map_err(|_| ScopeBlock::InputChanged)?;
                }
                let actual = format!(
                    "local:windows:{}:{}:{}",
                    info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow
                );
                if actual != project.source_path_key {
                    return Err(ScopeBlock::InputChanged);
                }
            }
            if let Some(path) = &project.canonical_path {
                let canonical = ObservedPath::observe(path, PathKind::Directory)?;
                if canonical.directory().map(|value| value.identity()) != Some(directory.identity())
                {
                    return Err(ScopeBlock::InputChanged);
                }
            }
        } else if let Some(path) = &project.canonical_path {
            if ObservedPath::observe(path, PathKind::Directory)?
                .directory()
                .is_some()
            {
                return Err(ScopeBlock::InputChanged);
            }
        }
    }
    Ok(())
}

fn ordinal_equal(left: &OsStr, right: &OsStr) -> ScopeResult<bool> {
    let left: Vec<_> = left.encode_wide().collect();
    let right: Vec<_> = right.encode_wide().collect();
    let result = unsafe { CompareStringOrdinal(&left, &right, true) };
    if result.0 == 0 {
        return Err(ScopeBlock::PathUnsupported);
    }
    Ok(result == CSTR_EQUAL)
}
fn registration_text(value: Option<&RegistryValue>) -> ScopeResult<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    if value.kind != 1
        || value.bytes.len() < 2
        || value.bytes.len() > 65536
        || !value.bytes.len().is_multiple_of(2)
    {
        return Err(ScopeBlock::UnsupportedRegistration);
    }
    let units: Vec<_> = value
        .bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u16::from_le_bytes(*b))
        .collect();
    let Some(units) = units.strip_suffix(&[0]) else {
        return Err(ScopeBlock::UnsupportedRegistration);
    };
    if units.contains(&0) {
        return Err(ScopeBlock::UnsupportedRegistration);
    }
    String::from_utf16(units)
        .map(Some)
        .map_err(|_| ScopeBlock::UnsupportedRegistration)
}
fn field<'a>(record: &'a InstallRecord, name: &str) -> Option<&'a RegistryValue> {
    record.values.get(name).and_then(Option::as_ref)
}
fn text_field(record: &InstallRecord, name: &str) -> ScopeResult<Option<String>> {
    registration_text(field(record, name))
}
fn dword(value: Option<&RegistryValue>) -> ScopeResult<Option<u32>> {
    let Some(value) = value else { return Ok(None) };
    if value.kind != 4 || value.bytes.len() != 4 {
        return Err(ScopeBlock::UnsupportedRegistration);
    }
    Ok(Some(u32::from_le_bytes(
        value
            .bytes
            .as_slice()
            .try_into()
            .map_err(|_| ScopeBlock::UnsupportedRegistration)?,
    )))
}
fn quoted_path(value: &str) -> ScopeResult<PathBuf> {
    let text = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .ok_or(ScopeBlock::UnsupportedRegistration)?;
    if text.contains('"') {
        return Err(ScopeBlock::UnsupportedRegistration);
    }
    drive_path(Path::new(text))
}
fn registered_directory(observation: &InstallRegistryObservation) -> ScopeResult<PathBuf> {
    observation
        .recheck()
        .map_err(|_| ScopeBlock::RegistrationChanged)?;
    let mut selected: Option<&InstallRecord> = None;
    let mut selected_views = 0;
    for record in observation.records() {
        let name = text_field(record, "DisplayName")?;
        let publisher = text_field(record, "Publisher")?;
        let binary = text_field(record, "MainBinaryName")?;
        let related = ordinal_equal(OsStr::new(&record.name), OsStr::new("CC Desk"))?
            || name
                .as_ref()
                .is_some_and(|value| value.eq_ignore_ascii_case("CC Desk"))
            || (publisher.as_deref() == Some("shawnwu2022")
                && binary.as_deref() == Some("cc-desk.exe"));
        if !related {
            continue;
        }
        if record.hive == InstallHive::LocalMachine {
            return Err(ScopeBlock::CompetingInstallation);
        }
        selected_views += 1;
        if let Some(previous) = selected {
            // These exact HKCU branches are OS-shared across the two views.
            // Only matching key spelling and complete selected values dedupe.
            if previous.view == record.view
                || !ordinal_equal(OsStr::new(&previous.name), OsStr::new(&record.name))?
                || previous.values != record.values
            {
                return Err(ScopeBlock::CompetingInstallation);
            }
        } else {
            selected = Some(record);
        }
    }
    let record = selected.ok_or(ScopeBlock::Unregistered)?;
    if selected_views != 2 || !ordinal_equal(OsStr::new(&record.name), OsStr::new("CC Desk"))? {
        return Err(ScopeBlock::UnsupportedRegistration);
    }
    if text_field(record, "DisplayName")?.as_deref() != Some("CC Desk")
        || text_field(record, "Publisher")?.as_deref() != Some("shawnwu2022")
        || text_field(record, "MainBinaryName")?.as_deref() != Some("cc-desk.exe")
        || text_field(record, "DisplayVersion")?.as_deref() != Some(env!("CARGO_PKG_VERSION"))
        || dword(field(record, "WindowsInstaller"))?.is_some_and(|value| value != 0)
        || dword(field(record, "AllUsers"))?.is_some_and(|value| value != 0)
        || dword(field(record, "CurrentUser"))?.is_some_and(|value| value != 1)
    {
        return Err(ScopeBlock::UnsupportedRegistration);
    }
    let directory = quoted_path(
        &text_field(record, "InstallLocation")?.ok_or(ScopeBlock::UnsupportedRegistration)?,
    )?;
    for (field_name, leaf) in [
        ("DisplayIcon", "cc-desk.exe"),
        ("UninstallString", "uninstall.exe"),
    ] {
        let observed = quoted_path(
            &text_field(record, field_name)?.ok_or(ScopeBlock::UnsupportedRegistration)?,
        )?;
        if !ordinal_equal(observed.as_os_str(), directory.join(leaf).as_os_str())? {
            return Err(ScopeBlock::UnsupportedRegistration);
        }
    }
    for value in observation.publisher_values() {
        let text = registration_text(value)?.ok_or(ScopeBlock::UnsupportedRegistration)?;
        let publisher_path = drive_path(Path::new(&text))?;
        if !ordinal_equal(publisher_path.as_os_str(), directory.as_os_str())? {
            return Err(ScopeBlock::UnsupportedRegistration);
        }
    }
    Ok(directory)
}
fn verify_x64_header(file: &PinnedFile) -> ScopeResult<()> {
    file.verify().map_err(|_| ScopeBlock::InputChanged)?;
    let size = file
        .file
        .metadata()
        .map_err(|_| ScopeBlock::InputUnavailable)?
        .len();
    let mut source = &file.file;
    source
        .seek(SeekFrom::Start(0))
        .map_err(|_| ScopeBlock::ImageUnsupported)?;
    let mut dos = [0u8; 64];
    source
        .read_exact(&mut dos)
        .map_err(|_| ScopeBlock::ImageUnsupported)?;
    if &dos[..2] != b"MZ" {
        return Err(ScopeBlock::ImageUnsupported);
    }
    let offset = u32::from_le_bytes(
        dos[0x3c..0x40]
            .try_into()
            .map_err(|_| ScopeBlock::ImageUnsupported)?,
    ) as u64;
    if !(64..=1024 * 1024).contains(&offset) || offset.checked_add(26).is_none_or(|end| end > size)
    {
        return Err(ScopeBlock::ImageUnsupported);
    }
    source
        .seek(SeekFrom::Start(offset))
        .map_err(|_| ScopeBlock::ImageUnsupported)?;
    let mut pe = [0u8; 26];
    source
        .read_exact(&mut pe)
        .map_err(|_| ScopeBlock::ImageUnsupported)?;
    if &pe[..4] != b"PE\0\0"
        || u16::from_le_bytes([pe[4], pe[5]]) != 0x8664
        || u16::from_le_bytes([pe[24], pe[25]]) != 0x20b
        || u16::from_le_bytes([pe[22], pe[23]]) & 0x2000 != 0
    {
        return Err(ScopeBlock::ImageUnsupported);
    }
    file.verify().map_err(|_| ScopeBlock::InputChanged)
}
struct RegisteredLocation {
    spelling: PathBuf,
    directory: Arc<Directory>,
    image: PinnedFile,
}
fn observe_registered_location(
    registry: &InstallRegistryObservation,
    current_image: &Path,
) -> ScopeResult<RegisteredLocation> {
    let spelling = registered_directory(registry)?;
    let directory =
        Directory::open_absolute(&spelling).map_err(|_| ScopeBlock::UnsupportedRegistration)?;
    let current = drive_path(current_image)?;
    if !ordinal_equal(
        current.file_name().ok_or(ScopeBlock::Relocated)?,
        OsStr::new("cc-desk.exe"),
    )? {
        return Err(ScopeBlock::Relocated);
    }
    let current_parent = Directory::open_absolute(current.parent().ok_or(ScopeBlock::Relocated)?)
        .map_err(|_| ScopeBlock::Relocated)?;
    if directory.identity() != current_parent.identity() {
        return Err(ScopeBlock::Relocated);
    }
    let image = directory
        .open_file(
            ComponentName::new(OsStr::new("cc-desk.exe"))
                .map_err(|_| ScopeBlock::ImageUnsupported)?,
            FileAccess::Read,
        )
        .map_err(|_| ScopeBlock::ImageUnsupported)?;
    let actual = current_parent
        .open_file(
            ComponentName::new(current.file_name().ok_or(ScopeBlock::Relocated)?)
                .map_err(|_| ScopeBlock::Relocated)?,
            FileAccess::Read,
        )
        .map_err(|_| ScopeBlock::Relocated)?;
    if actual.identity() != image.identity() {
        return Err(ScopeBlock::Relocated);
    }
    verify_x64_header(&image)?;
    registry
        .recheck()
        .map_err(|_| ScopeBlock::RegistrationChanged)?;
    Ok(RegisteredLocation {
        spelling,
        directory,
        image,
    })
}
/// Ordinary source-host or exact-process manager re-admission. This never
/// approves arbitrary registered bytes for launch.
pub(crate) struct RegisteredInstallation {
    user: CurrentUser,
    process: ExactProcess,
    registry: InstallRegistryObservation,
    location: RegisteredLocation,
}
impl RegisteredInstallation {
    pub(crate) fn capture() -> ScopeResult<Self> {
        let process = ExactProcess::capture_observed(std::process::id())
            .map_err(|_| ScopeBlock::ImageUnsupported)?;
        let current = std::env::current_exe().map_err(|_| ScopeBlock::ImageUnsupported)?;
        Self::capture_for_source(process, &current)
    }
    /// The caller has already reopened the exact protected handoff process.
    /// original_image is only a selector: registry, token and held file identity
    /// are all independently observed below, including from a copied manager.
    pub(crate) fn capture_for_source(
        process: ExactProcess,
        original_image: &Path,
    ) -> ScopeResult<Self> {
        let registry = InstallRegistryObservation::capture()
            .map_err(|_| ScopeBlock::UnsupportedRegistration)?;
        Self::from_source_registry(process, original_image, registry)
    }
    fn from_source_registry(
        process: ExactProcess,
        original_image: &Path,
        registry: InstallRegistryObservation,
    ) -> ScopeResult<Self> {
        if !cfg!(target_arch = "x86_64") {
            return Err(ScopeBlock::UnsupportedArchitecture);
        }
        let user = CurrentUser::capture().map_err(|_| ScopeBlock::InputUnavailable)?;
        user.require_unelevated()
            .map_err(|_| ScopeBlock::Elevated)?;
        process
            .verify_current_user(&user)
            .map_err(|_| ScopeBlock::ImageUnsupported)?;
        let location = observe_registered_location(&registry, original_image)?;
        process
            .verify_held_image(&location.image)
            .map_err(|_| ScopeBlock::Relocated)?;
        let result = Self {
            user,
            process,
            registry,
            location,
        };
        result.recheck()?;
        Ok(result)
    }
    #[cfg(test)]
    pub(crate) fn fixture_for_source(
        process: ExactProcess,
        original_image: &Path,
        user: windows::Win32::System::Registry::HKEY,
        machine: windows::Win32::System::Registry::HKEY,
    ) -> ScopeResult<Self> {
        Self::from_source_registry(
            process,
            original_image,
            InstallRegistryObservation::fixture_hives(user, machine)
                .map_err(|_| ScopeBlock::UnsupportedRegistration)?,
        )
    }
    pub(crate) fn release_after_exit(self) -> ScopeResult<ExitedInstallation> {
        self.user
            .require_unelevated()
            .map_err(|_| ScopeBlock::Elevated)?;
        self.registry
            .recheck()
            .map_err(|_| ScopeBlock::RegistrationChanged)?;
        self.location
            .directory
            .recheck()
            .map_err(|_| ScopeBlock::InputChanged)?;
        self.location
            .image
            .verify()
            .map_err(|_| ScopeBlock::InputChanged)?;
        let identity = self.location.image.identity().clone();
        let digest = self
            .location
            .image
            .digest()
            .map_err(|_| ScopeBlock::InputChanged)?;
        let name = self.location.image.name.clone();
        let terminal = self
            .process
            .release_terminated_image()
            .map_err(|_| ScopeBlock::InputUnavailable)?;
        let RegisteredLocation {
            spelling,
            directory,
            image,
        } = self.location;
        drop(image);
        let exited = ExitedInstallation {
            user: self.user,
            terminal,
            registry: self.registry,
            directory,
            spelling,
            name,
            identity,
            digest,
        };
        exited.verify()?;
        Ok(exited)
    }
    pub(crate) fn recheck(&self) -> ScopeResult<()> {
        self.user
            .require_unelevated()
            .map_err(|_| ScopeBlock::Elevated)?;
        self.registry
            .recheck()
            .map_err(|_| ScopeBlock::RegistrationChanged)?;
        self.location
            .directory
            .recheck()
            .map_err(|_| ScopeBlock::InputChanged)?;
        self.process
            .verify_current_user(&self.user)
            .map_err(|_| ScopeBlock::ImageUnsupported)?;
        self.process
            .verify_held_image(&self.location.image)
            .map_err(|_| ScopeBlock::Relocated)?;
        let named =
            Directory::open_absolute(&self.location.spelling).map_err(|_| ScopeBlock::Relocated)?;
        if named.identity() != self.location.directory.identity() {
            return Err(ScopeBlock::Relocated);
        }
        Ok(())
    }
    pub(crate) fn directory(&self) -> &Arc<Directory> {
        &self.location.directory
    }
    pub(crate) fn original_path(&self) -> &Path {
        &self.location.spelling
    }
    pub(crate) fn image(&self) -> &PinnedFile {
        &self.location.image
    }
}
/// Exact waited source ownership after image read guards are consumed. The
/// original directory and registry observations remain held; no caller supplies
/// a digest or file identity to obtain this fence input.
pub(crate) struct ExitedInstallation {
    user: CurrentUser,
    terminal: super::process::TerminatedProcess,
    registry: InstallRegistryObservation,
    directory: Arc<Directory>,
    spelling: PathBuf,
    name: ComponentName,
    identity: super::files::FileIdentity,
    digest: String,
}
impl ExitedInstallation {
    pub(crate) fn verify(&self) -> ScopeResult<()> {
        self.user
            .require_unelevated()
            .map_err(|_| ScopeBlock::Elevated)?;
        self.terminal
            .verify()
            .map_err(|_| ScopeBlock::ImageUnsupported)?;
        self.registry
            .recheck()
            .map_err(|_| ScopeBlock::RegistrationChanged)?;
        self.directory
            .recheck()
            .map_err(|_| ScopeBlock::InputChanged)?;
        let named = Directory::open_absolute(&self.spelling).map_err(|_| ScopeBlock::Relocated)?;
        if named.identity() != self.directory.identity() {
            return Err(ScopeBlock::Relocated);
        }
        Ok(())
    }
    pub(crate) fn directory(&self) -> &Arc<Directory> {
        &self.directory
    }
    pub(crate) fn image_name(&self) -> &ComponentName {
        &self.name
    }
    pub(crate) fn acquire_source_fence(&self) -> std::io::Result<super::fence::ImageFence> {
        self.verify()
            .map_err(|_| super::blocked("exited source observation changed"))?;
        // Keep the original OS sharing error so the coordinator may distinguish
        // a definitely unacquired fence from an uncertain later effect.
        let fence = super::fence::ImageFence::acquire(
            self.directory.clone(),
            self.name.clone(),
            &self.identity,
            &self.digest,
        )?;
        self.verify()
            .map_err(|_| super::blocked("exited source observation changed"))?;
        fence.verify()?;
        Ok(fence)
    }
    /// Consumes pre-install registry readers only after their original source
    /// image has been matched to the actual retained exclusive fence.
    pub(crate) fn into_fenced(
        self,
        fence: Arc<parking_lot::Mutex<super::fence::ImageFence>>,
    ) -> ScopeResult<FencedInstallation> {
        self.verify()?;
        verify_original_fence(
            &fence.lock(),
            &self.directory,
            &self.name,
            &self.identity,
            &self.digest,
        )?;
        self.verify()?;
        let Self {
            user,
            terminal,
            registry,
            directory,
            spelling,
            name,
            identity,
            digest,
        } = self;
        drop(registry);
        let result = FencedInstallation {
            user,
            terminal,
            directory,
            spelling,
            name,
            identity,
            digest,
            _fence: fence,
        };
        result.verify()?;
        result.verify_fence(&result._fence.lock())?;
        Ok(result)
    }
}

/// Historical registered-source ownership after its obsolete registry readers
/// have been consumed. It retains the actual terminal process and fence guard,
/// not current registry state after the installer intentionally changes it.
pub(crate) struct FencedInstallation {
    user: CurrentUser,
    terminal: super::process::TerminatedProcess,
    directory: Arc<Directory>,
    spelling: PathBuf,
    name: ComponentName,
    identity: super::files::FileIdentity,
    digest: String,
    _fence: Arc<parking_lot::Mutex<super::fence::ImageFence>>,
}
impl FencedInstallation {
    /// No mutex reacquisition: boundary verification may run while context
    /// holds the fence. Its caller also verifies_fence with that existing lock.
    pub(crate) fn verify(&self) -> ScopeResult<()> {
        self.user
            .require_unelevated()
            .map_err(|_| ScopeBlock::Elevated)?;
        self.terminal
            .verify()
            .map_err(|_| ScopeBlock::ImageUnsupported)?;
        self.directory
            .recheck()
            .map_err(|_| ScopeBlock::InputChanged)?;
        let named = Directory::open_absolute(&self.spelling).map_err(|_| ScopeBlock::Relocated)?;
        if named.identity() != self.directory.identity() {
            return Err(ScopeBlock::Relocated);
        }
        Ok(())
    }
    pub(crate) fn verify_fence(&self, fence: &super::fence::ImageFence) -> ScopeResult<()> {
        self.verify()?;
        verify_original_fence(
            fence,
            &self.directory,
            &self.name,
            &self.identity,
            &self.digest,
        )
    }
    pub(crate) fn directory(&self) -> &Arc<Directory> {
        &self.directory
    }
    pub(crate) fn image_name(&self) -> &ComponentName {
        &self.name
    }
}
fn verify_original_fence(
    fence: &super::fence::ImageFence,
    directory: &Directory,
    name: &ComponentName,
    identity: &super::files::FileIdentity,
    digest: &str,
) -> ScopeResult<()> {
    use sha2::{Digest, Sha256};
    fence.verify().map_err(|_| ScopeBlock::ImageUnsupported)?;
    if fence.identity() != identity
        || !fence
            .context_original_child(directory, name)
            .map_err(|_| ScopeBlock::ImageUnsupported)?
    {
        return Err(ScopeBlock::Relocated);
    }
    let length = fence
        .context_metadata()
        .map_err(|_| ScopeBlock::ImageUnsupported)?
        .size;
    let mut actual = Sha256::new();
    let mut offset = 0;
    let mut bytes = [0; 65536];
    loop {
        let count = fence
            .context_read(offset, &mut bytes)
            .map_err(|_| ScopeBlock::ImageUnsupported)?;
        if count == 0 {
            break;
        }
        offset += count as u64;
        if offset > length {
            return Err(ScopeBlock::InputChanged);
        }
        actual.update(&bytes[..count]);
    }
    if offset != length || format!("{:x}", actual.finalize()) != digest {
        return Err(ScopeBlock::InputChanged);
    }
    fence.verify().map_err(|_| ScopeBlock::ImageUnsupported)
}
#[cfg(test)]
pub(crate) fn fixture_registration_location(
    registry: &InstallRegistryObservation,
    current: &Path,
) -> ScopeResult<()> {
    // Tests exercise observed registry/file policy, not a fabricated user token
    // or production RegisteredInstallation admission.
    observe_registered_location(registry, current).map(|_| ())
}

struct HistoryPrefix {
    file: PinnedFile,
    length: u64,
    digest: String,
    file_length: u64,
}
struct HistoryDirectory {
    directory: Arc<Directory>,
    entries: Vec<ChildEntry>,
}
fn history_entries(directory: &Directory) -> ScopeResult<Vec<ChildEntry>> {
    bounded_history_entries(directory, MAX_SELECTORS)
}
fn bounded_history_entries(directory: &Directory, maximum: usize) -> ScopeResult<Vec<ChildEntry>> {
    let mut entries = directory.read_child_entries(maximum).map_err(|error| {
        if super::files::is_inventory_limit(&error) {
            ScopeBlock::InputLimit
        } else {
            ScopeBlock::InputUnavailable
        }
    })?;
    entries.sort_by_key(|entry| entry.name.os_string());
    Ok(entries)
}
/// The fixed Legacy reader associates a session with its first top-level cwd.
/// Bound that exact input read and retain only its hash/handles, never message
/// bodies. Native histories are not promoted into registration authority here.
pub(crate) struct LegacyProjectInputs {
    root: ObservedPath,
    directories: Vec<HistoryDirectory>,
    prefixes: Vec<HistoryPrefix>,
    paths: Vec<PathBuf>,
}
impl LegacyProjectInputs {
    pub(crate) fn capture(home: &Path) -> ScopeResult<Self> {
        const MAX_PREFIX: usize = 4 * 1024 * 1024;
        const MAX_PREFIX_TOTAL: usize = 64 * 1024 * 1024;
        let root =
            ObservedPath::observe(&home.join(".claude").join("projects"), PathKind::Directory)?;
        let mut result = Self {
            root,
            directories: Vec::new(),
            prefixes: Vec::new(),
            paths: Vec::new(),
        };
        let PathObject::Directory(root) = &result.root.object else {
            result.recheck()?;
            return Ok(result);
        };
        let root = root.clone();
        let entries = history_entries(&root)?;
        let mut total = 0usize;
        let mut entry_count = entries.len();
        for entry in &entries {
            if !entry.directory {
                continue;
            }
            let directory = root
                .open_directory(entry.name.clone())
                .map_err(|_| ScopeBlock::InputChanged)?;
            let children = history_entries(&directory)?;
            entry_count = entry_count
                .checked_add(children.len())
                .ok_or(ScopeBlock::InputLimit)?;
            if entry_count > 100_000 {
                return Err(ScopeBlock::InputLimit);
            }
            for child in &children {
                if child.directory {
                    continue;
                }
                let name = child.name.os_string();
                let path = Path::new(&name);
                let Some(name_text) = name.to_str() else {
                    return Err(ScopeBlock::PathUnsupported);
                };
                if name_text.starts_with("agent-")
                    || !matches!(
                        path.extension().and_then(OsStr::to_str),
                        Some("jsonl" | "txt")
                    )
                {
                    continue;
                }
                if result.prefixes.len() >= MAX_SELECTORS {
                    return Err(ScopeBlock::InputLimit);
                }
                let file = directory
                    .open_file(child.name.clone(), FileAccess::Read)
                    .map_err(|_| ScopeBlock::InputUnavailable)?;
                let file_length = file
                    .file
                    .metadata()
                    .map_err(|_| ScopeBlock::InputUnavailable)?
                    .len();
                let mut source = &file.file;
                source
                    .seek(SeekFrom::Start(0))
                    .map_err(|_| ScopeBlock::InputUnavailable)?;
                let mut reader = BufReader::new(source.take(MAX_PREFIX as u64 + 1));
                let mut prefix = Vec::new();
                let mut line = Vec::new();
                loop {
                    line.clear();
                    let count = reader
                        .read_until(b'\n', &mut line)
                        .map_err(|_| ScopeBlock::InputUnavailable)?;
                    if count == 0 {
                        break;
                    }
                    total = total.checked_add(count).ok_or(ScopeBlock::InputLimit)?;
                    if prefix.len() + count > MAX_PREFIX || total > MAX_PREFIX_TOTAL {
                        return Err(ScopeBlock::InputLimit);
                    }
                    prefix.extend_from_slice(&line);
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let value = strict_json(&line, MAX_PREFIX)?;
                    if let Some(cwd) = value.get("cwd").filter(|cwd| !cwd.is_null()) {
                        let cwd = cwd.as_str().ok_or(ScopeBlock::InputMalformed)?;
                        let path = drive_path(Path::new(cwd))?;
                        if !result.paths.contains(&path) {
                            result.paths.push(path);
                        }
                        break;
                    }
                }
                drop(reader);
                file.verify().map_err(|_| ScopeBlock::InputChanged)?;
                result.prefixes.push(HistoryPrefix {
                    file,
                    file_length,
                    length: prefix.len() as u64,
                    digest: crate::version_history::verified_package::sha256(&prefix),
                });
            }
            result.directories.push(HistoryDirectory {
                directory,
                entries: children,
            });
        }
        result.directories.push(HistoryDirectory {
            directory: root,
            entries,
        });
        result.recheck()?;
        Ok(result)
    }
    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }
    pub(crate) fn recheck(&self) -> ScopeResult<()> {
        self.root.recheck()?;
        for directory in &self.directories {
            if history_entries(&directory.directory)? != directory.entries {
                return Err(ScopeBlock::InputChanged);
            }
        }
        for prefix in &self.prefixes {
            prefix.file.verify().map_err(|_| ScopeBlock::InputChanged)?;
            if prefix
                .file
                .file
                .metadata()
                .map_err(|_| ScopeBlock::InputChanged)?
                .len()
                != prefix.file_length
            {
                return Err(ScopeBlock::InputChanged);
            }
            let mut file = &prefix.file.file;
            file.seek(SeekFrom::Start(0))
                .map_err(|_| ScopeBlock::InputChanged)?;
            let mut bytes = vec![0; prefix.length as usize];
            file.read_exact(&mut bytes)
                .map_err(|_| ScopeBlock::InputChanged)?;
            if crate::version_history::verified_package::sha256(&bytes) != prefix.digest {
                return Err(ScopeBlock::InputChanged);
            }
            prefix.file.verify().map_err(|_| ScopeBlock::InputChanged)?;
        }
        Ok(())
    }
}

pub(crate) struct ConfiguredInventory {
    home: PathBuf,
    environment: EnvMap,
    inputs: ScopeInputs,
    paths: Vec<ObservedPath>,
    legacy_history: LegacyProjectInputs,
}
impl ConfiguredInventory {
    pub(crate) fn capture() -> ScopeResult<Self> {
        let home = dirs::home_dir().ok_or(ScopeBlock::InputUnavailable)?;
        let environment: EnvMap = std::env::vars_os().collect();
        let inputs = ScopeInputs::capture(&home.join(".cc-box"))?;
        let mut candidates = configured_candidates(
            &home,
            &environment,
            &inputs.workspace,
            &inputs.config,
            &inputs.projects,
        )?;
        let legacy_history = LegacyProjectInputs::capture(&home)?;
        for path in legacy_history.paths() {
            add_candidate(&mut candidates, path, PathKind::Directory)?;
        }
        let paths = candidates
            .into_iter()
            .map(|candidate| ObservedPath::observe(&candidate.path, candidate.kind))
            .collect::<ScopeResult<Vec<_>>>()?;
        let result = Self {
            home,
            environment,
            inputs,
            paths,
            legacy_history,
        };
        result.recheck()?;
        result.require_disjoint(&[])?;
        Ok(result)
    }
    pub(crate) fn recheck(&self) -> ScopeResult<()> {
        if dirs::home_dir().as_ref() != Some(&self.home)
            || std::env::vars_os().collect::<EnvMap>() != self.environment
        {
            return Err(ScopeBlock::InputChanged);
        }
        self.inputs.recheck()?;
        self.legacy_history.recheck()?;
        for path in &self.paths {
            path.recheck()?;
        }
        Ok(())
    }
    /// Desk is always checked. Supply additional actual UDF/install/recovery/
    /// quarantine roots as held observations; no nominal UDF is invented.
    pub(crate) fn require_disjoint(&self, additional_roots: &[&ObservedPath]) -> ScopeResult<()> {
        self.recheck()?;
        let roots: Vec<_> = std::iter::once(&self.inputs.desk)
            .chain(additional_roots.iter().copied())
            .collect();
        for (index, root) in roots.iter().enumerate() {
            if root.kind != PathKind::Directory {
                return Err(ScopeBlock::PathUnsupported);
            }
            for other in &roots[..index] {
                if root.overlaps(other)? {
                    return Err(ScopeBlock::Overlap);
                }
            }
            for protected in &self.paths {
                if root.overlaps(protected)? {
                    return Err(ScopeBlock::Overlap);
                }
            }
        }
        self.recheck()
    }
    pub(crate) fn desk_root(&self) -> &ObservedPath {
        &self.inputs.desk
    }
}

#[cfg(test)]
pub(crate) fn fixture_history_entry_count(
    directory: &Directory,
    maximum: usize,
) -> ScopeResult<usize> {
    bounded_history_entries(directory, maximum).map(|entries| entries.len())
}
