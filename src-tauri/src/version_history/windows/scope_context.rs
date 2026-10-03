//! Final configured scope is decoded from the complete durable M0 itself.
//! The borrow prevents cutover until it has been consumed into external guards.
use super::*;
use crate::version_history::{
    journal::RootKind,
    snapshot::{ContextReader, EntryType},
    verified_package::sha256,
    windows::{
        context::{HeldContext, HeldRoot},
        files::PrivateDirectory,
    },
};

const INPUTS: [(&str, usize); 3] = [
    ("cli-workspace.v1.json", MAX_INPUT_BYTES),
    ("config.json", MAX_CONFIG_BYTES),
    ("projects.json", MAX_INPUT_BYTES),
];

struct HostObservation {
    home: PathBuf,
    environment: EnvMap,
}
impl HostObservation {
    fn capture() -> ScopeResult<Self> {
        Ok(Self {
            home: dirs::home_dir().ok_or(ScopeBlock::InputUnavailable)?,
            environment: std::env::vars_os().collect(),
        })
    }
    fn verify(&self) -> ScopeResult<()> {
        if dirs::home_dir().as_ref() != Some(&self.home)
            || std::env::vars_os().collect::<EnvMap>() != self.environment
        {
            return Err(ScopeBlock::InputChanged);
        }
        Ok(())
    }
}

pub(crate) struct ContextConfiguredInventory<'a> {
    context: &'a HeldContext,
    host: HostObservation,
    paths: Vec<ObservedPath>,
    legacy_history: LegacyProjectInputs,
    configuration_identity: String,
}

/// A nonserializable consumed observation. Source bytes/absence stay historical
/// after rotation; live external objects and the original root guards remain.
pub(crate) struct ConfiguredExclusions {
    host: HostObservation,
    paths: Vec<ObservedPath>,
    legacy_history: LegacyProjectInputs,
    roots: BTreeMap<RootKind, HeldRoot>,
    original_locations: BTreeMap<RootKind, String>,
    original_manifests: BTreeMap<RootKind, String>,
    configuration_identity: String,
    installation: Arc<Directory>,
    recovery: Arc<PrivateDirectory>,
}

impl ConfiguredInventory {
    /// Initial diagnostic readers must be dropped first. Complete durable M0
    /// capture is only observation/flush; no copy or root mutation is authorized
    /// until these exact bytes have supplied the final exclusion capability.
    pub(crate) fn capture_for_context(
        context: &mut HeldContext,
    ) -> ScopeResult<ContextConfiguredInventory<'_>> {
        let host = HostObservation::capture()?;
        let home = host.home.clone();
        let environment = host.environment.clone();
        capture(context, &home, &environment, host)
    }

    /// Isolated input locations only. Real NTFS/context operations and actual
    /// host-environment rechecks still run; this is not a registered-source proof.
    #[cfg(test)]
    pub(crate) fn fixture_for_context<'a>(
        context: &'a mut HeldContext,
        home: &Path,
        environment: EnvMap,
    ) -> ScopeResult<ContextConfiguredInventory<'a>> {
        capture(context, home, &environment, HostObservation::capture()?)
    }
}

fn capture<'a>(
    context: &'a mut HeldContext,
    home: &Path,
    environment: &EnvMap,
    host: HostObservation,
) -> ScopeResult<ContextConfiguredInventory<'a>> {
    context
        .verify_durable()
        .map_err(|_| ScopeBlock::InputChanged)?;
    let home_root = Directory::open_absolute(home).map_err(|_| ScopeBlock::InputUnavailable)?;
    require_desk_location(context.tree(RootKind::Desk).root(), &home_root)?;
    for kind in [RootKind::Desk, RootKind::WebView] {
        if let HeldRoot::Present(root) = context.tree(kind).root() {
            root.require_renameable()
                .map_err(|_| ScopeBlock::PathUnsupported)?;
        }
    }
    let mut bound_entries = Vec::new();
    let mut inputs = Vec::new();
    for (name, maximum) in INPUTS {
        let mut matches = Vec::new();
        for entry in &context.tree(RootKind::Desk).manifest().entries {
            if !entry.metadata.path.contains('/')
                && ordinal_equal(OsStr::new(&entry.metadata.path), OsStr::new(name))?
            {
                matches.push(entry.clone());
            }
        }
        if matches.len() > 1 {
            return Err(ScopeBlock::InputMalformed);
        }
        let entry = matches.pop();
        let bytes = if let Some(entry) = &entry {
            if entry.metadata.kind != EntryType::File {
                return Err(ScopeBlock::InputMalformed);
            }
            if entry.metadata.size > maximum as u64 {
                return Err(ScopeBlock::InputLimit);
            }
            let mut bytes = Vec::new();
            context
                .open_file(RootKind::Desk, &entry.metadata)
                .map_err(|_| ScopeBlock::InputChanged)?
                .take(maximum as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ScopeBlock::InputChanged)?;
            if bytes.len() > maximum {
                return Err(ScopeBlock::InputLimit);
            }
            if bytes.len() as u64 != entry.metadata.size
                || entry.sha256.as_deref() != Some(sha256(&bytes).as_str())
            {
                return Err(ScopeBlock::InputChanged);
            }
            Some(bytes)
        } else {
            None
        };
        bound_entries.push(entry);
        inputs.push(bytes);
    }
    let workspace = if let Some(bytes) = &inputs[0] {
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
    let config = ConfigScopeInputs::decode(inputs[1].as_deref().unwrap_or(b"{}"))?;
    let projects = ProjectsScopeInputs::decode(inputs[2].as_deref().unwrap_or(b"{}"))?;
    validate_project_locations(&workspace)?;
    let mut candidates = configured_candidates(home, environment, &workspace, &config, &projects)?;
    let legacy_history = LegacyProjectInputs::capture(home)?;
    for path in legacy_history.paths() {
        add_candidate(&mut candidates, path, PathKind::Directory)?;
    }
    let paths = candidates
        .into_iter()
        .map(|candidate| ObservedPath::observe(&candidate.path, candidate.kind))
        .collect::<ScopeResult<Vec<_>>>()?;
    let result = ContextConfiguredInventory {
        context,
        host,
        paths,
        legacy_history,
        configuration_identity: sha256(
            &serde_json::to_vec(&bound_entries).map_err(|_| ScopeBlock::InputMalformed)?,
        ),
    };
    result.recheck()?;
    Ok(result)
}

fn require_desk_location(root: &HeldRoot, home: &Arc<Directory>) -> ScopeResult<()> {
    let (parent, name) = match root {
        HeldRoot::Present(root) => root.held_location().map_err(|_| ScopeBlock::InputChanged)?,
        HeldRoot::Absent { parent, name } => (parent.clone(), name.clone()),
    };
    parent.recheck().map_err(|_| ScopeBlock::InputChanged)?;
    if parent.identity() != home.identity()
        || !ordinal_equal(&name.os_string(), OsStr::new(".cc-box"))?
    {
        return Err(ScopeBlock::InputChanged);
    }
    Ok(())
}

impl ContextConfiguredInventory<'_> {
    pub(crate) fn recheck(&self) -> ScopeResult<()> {
        self.host.verify()?;
        self.context
            .verify_durable()
            .map_err(|_| ScopeBlock::InputChanged)?;
        self.legacy_history.recheck()?;
        for path in &self.paths {
            path.recheck()?;
        }
        Ok(())
    }
    pub(crate) fn into_exclusions(
        self,
        installation: Arc<Directory>,
        recovery: Arc<PrivateDirectory>,
    ) -> ScopeResult<ConfiguredExclusions> {
        self.recheck()?;
        let mut roots = BTreeMap::new();
        let mut original_manifests = BTreeMap::new();
        for kind in [RootKind::Desk, RootKind::WebView] {
            roots.insert(kind, self.context.tree(kind).root().clone());
            original_manifests.insert(
                kind,
                self.context
                    .tree(kind)
                    .manifest()
                    .digest()
                    .map_err(|_| ScopeBlock::InputChanged)?,
            );
        }
        let result = ConfiguredExclusions {
            host: self.host,
            paths: self.paths,
            legacy_history: self.legacy_history,
            roots,
            original_locations: self.context.root_identities(),
            original_manifests,
            configuration_identity: self.configuration_identity,
            installation,
            recovery,
        };
        let locations = result.root_components()?;
        for (index, left) in locations.iter().enumerate() {
            for right in &locations[..index] {
                if components_overlap(left, right)? {
                    return Err(ScopeBlock::Overlap);
                }
            }
        }
        result.verify_context(self.context)?;
        Ok(result)
    }
}

impl ConfiguredExclusions {
    #[cfg(all(feature = "history-roundtrip-acceptance", not(test)))]
    pub(crate) fn acceptance_require_disjoint(&self, evidence: &ObservedPath) -> ScopeResult<()> {
        self.verify_external()?;
        let evidence = evidence.components()?;
        for root in self.root_components()? {
            if components_overlap(&root, &evidence)? {
                return Err(ScopeBlock::Overlap);
            }
        }
        for path in &self.paths {
            if components_overlap(&path.components()?, &evidence)? {
                return Err(ScopeBlock::Overlap);
            }
        }
        self.verify_external()
    }
    /// Rechecks the SAME complete durable M0 before source boundary admission.
    /// The original bytes cannot be replaced with an equal caller-supplied hash.
    pub(crate) fn verify_context(&self, context: &HeldContext) -> ScopeResult<()> {
        self.verify_external()?;
        context
            .verify_durable()
            .map_err(|_| ScopeBlock::InputChanged)?;
        for kind in [RootKind::Desk, RootKind::WebView] {
            if !same_root(&self.roots[&kind], context.tree(kind).root())
                || context.tree(kind).manifest().location_identity != self.original_locations[&kind]
                || context
                    .tree(kind)
                    .manifest()
                    .digest()
                    .map_err(|_| ScopeBlock::InputChanged)?
                    != self.original_manifests[&kind]
            {
                return Err(ScopeBlock::InputChanged);
            }
        }
        self.verify_external()
    }
    pub(crate) fn verify(&self) -> ScopeResult<()> {
        self.verify_external()
    }
    pub(crate) fn verify_external(&self) -> ScopeResult<()> {
        self.host.verify()?;
        self.legacy_history.recheck()?;
        self.recovery
            .verify(&CurrentUser::capture().map_err(|_| ScopeBlock::InputUnavailable)?)
            .map_err(|_| ScopeBlock::InputChanged)?;
        let roots = self.root_components()?;
        for path in &self.paths {
            let protected = path.components()?;
            for root in &roots {
                if components_overlap(root, &protected)? {
                    return Err(ScopeBlock::Overlap);
                }
            }
        }
        self.host.verify()
    }
    pub(crate) fn source_root_identity(&self, kind: RootKind) -> &str {
        &self.original_locations[&kind]
    }
    pub(crate) fn configuration_identity(&self) -> &str {
        &self.configuration_identity
    }
    fn root_components(&self) -> ScopeResult<Vec<Vec<Vec<u16>>>> {
        let mut roots = self
            .roots
            .values()
            .map(root_components)
            .collect::<ScopeResult<Vec<_>>>()?;
        roots.push(directory_components(&self.installation)?);
        roots.push(directory_components(self.recovery.directory())?);
        Ok(roots)
    }
}

fn same_root(left: &HeldRoot, right: &HeldRoot) -> bool {
    match (left, right) {
        (HeldRoot::Present(left), HeldRoot::Present(right)) => Arc::ptr_eq(left, right),
        (
            HeldRoot::Absent {
                parent: left,
                name: left_name,
            },
            HeldRoot::Absent {
                parent: right,
                name: right_name,
            },
        ) => Arc::ptr_eq(left, right) && left_name == right_name,
        _ => false,
    }
}
fn directory_components(root: &Directory) -> ScopeResult<Vec<Vec<u16>>> {
    root.recheck().map_err(|_| ScopeBlock::InputChanged)?;
    Ok(root
        .path()
        .map_err(|_| ScopeBlock::InputChanged)?
        .encode_wide()
        .collect::<Vec<_>>()
        .split(|unit| *unit == b'\\' as u16)
        .filter(|part| !part.is_empty())
        .map(Vec::from)
        .collect())
}
fn root_components(root: &HeldRoot) -> ScopeResult<Vec<Vec<u16>>> {
    match root {
        HeldRoot::Present(root) => directory_components(root),
        HeldRoot::Absent { parent, name } => {
            // Historical absence becomes a fresh owned root later. Keep the
            // exact original slot and ancestor guard, not a false live absence.
            let mut result = directory_components(parent)?;
            result.push(name.os_string().encode_wide().collect());
            Ok(result)
        }
    }
}
fn components_overlap(left: &[Vec<u16>], right: &[Vec<u16>]) -> ScopeResult<bool> {
    for (left, right) in left.iter().zip(right) {
        let result = unsafe { CompareStringOrdinal(left, right, true) };
        if result.0 == 0 {
            return Err(ScopeBlock::PathUnsupported);
        }
        if result != CSTR_EQUAL {
            return Ok(false);
        }
    }
    Ok(true)
}
