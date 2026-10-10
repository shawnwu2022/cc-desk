//! Ordinary admission before ConPTY, logger, repositories or WebView creation.
//! All feature-bearing copies consult the same private per-user marker root;
//! transaction journals remain separately named and are never reset for reuse.
use super::{
    context::HeldBundle,
    durability::MarkerStore,
    files::{ComponentName, Directory, FileIdentity, PrivateDirectory},
    lease::{ControlLease, ExclusiveLease, LeaseFiles, SharedLease},
    scope::RegisteredInstallation,
    security::CurrentUser,
};
use crate::cli::{profiles::error, types::SafeError};
use crate::version_history::{
    journal::JournalStore,
    maintenance::{ActiveContextMarker, MarkerRead, SharedStartupLease},
    policy::PRODUCT_IDENTIFIER,
    snapshot::SnapshotLimits,
    verified_package::sha256,
};
use std::{ffi::OsStr, os::windows::ffi::OsStrExt, sync::Arc};

const CONTROL_DIRECTORY: &str = "CCDesk-VersionControl";
const BACKUP_DIRECTORY: &str = "CCDesk-VersionBackups";
pub(crate) const MANAGER_BASENAME: &str = "cc-desk-version-manager.exe";
fn unavailable(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_STARTUP_UNAVAILABLE")
}
fn recovery(_: impl std::fmt::Debug) -> SafeError {
    error("HISTORY_RECOVERY_REQUIRED")
}

pub(crate) struct InstallationControl {
    parent: Arc<Directory>,
    root: Arc<PrivateDirectory>,
    leases: LeaseFiles,
    user: CurrentUser,
    // Retain the secured fixed sibling as well as the UUID child's pinned
    // ancestor chain. Only the backend fixed-locator factory sets this origin.
    backup_parent: Option<Arc<PrivateDirectory>>,
}
impl InstallationControl {
    /// A hard-coded private application directory is the only locator. Merely
    /// opening it is not registration, snapshot or installed-image admission.
    pub(crate) fn open(create: bool) -> Result<Arc<Self>, SafeError> {
        let local = dirs::data_local_dir().ok_or_else(|| error("HISTORY_STARTUP_UNAVAILABLE"))?;
        let parent = Directory::open_absolute(&local).map_err(unavailable)?;
        Self::open_under(parent, create)
    }
    fn open_under(parent: Arc<Directory>, create: bool) -> Result<Arc<Self>, SafeError> {
        let user = CurrentUser::capture().map_err(unavailable)?;
        let name = ComponentName::new(OsStr::new(CONTROL_DIRECTORY)).map_err(unavailable)?;
        let root = match PrivateDirectory::open_existing(parent.clone(), name.clone(), &user) {
            Ok(root) => root,
            Err(missing) if create && missing.kind() == std::io::ErrorKind::NotFound => {
                match PrivateDirectory::create_new(parent.clone(), name.clone(), &user) {
                    Ok(root) => root,
                    // Another admitted startup may create the same private
                    // root. Reopening must independently verify owner/ACL/ID.
                    Err(_) => PrivateDirectory::open_existing(parent.clone(), name, &user)
                        .map_err(unavailable)?,
                }
            }
            Err(failure) => return Err(unavailable(failure)),
        };
        let root = Arc::new(root);
        root.verify(&user).map_err(unavailable)?;
        let leases = LeaseFiles::open(root.clone(), &user).map_err(unavailable)?;
        Ok(Arc::new(Self {
            parent,
            root,
            leases,
            user,
            backup_parent: None,
        }))
    }
    /// An ordinary historical install writes its journal and marker only in a
    /// fresh transaction child of this fixed private sibling. The source still
    /// owns its original global shared lease; callers establish independent
    /// shared/exclusive leases on the returned owner.
    pub(crate) fn ordinary_backup(
        self: &Arc<Self>,
        global_control: &ControlLease,
        transaction_id: &str,
    ) -> Result<Arc<Self>, SafeError> {
        if self.is_ordinary_backup() {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        global_control.verify_root(self.root()).map_err(recovery)?;
        self.require_ordinary_global_context()?;
        let backup = Self::backup_under(self.parent.clone(), transaction_id, true)?;
        global_control.verify_root(self.root()).map_err(recovery)?;
        self.require_ordinary_global_context()?;
        Ok(backup)
    }
    pub(crate) fn require_ordinary_global_context(&self) -> Result<(), SafeError> {
        self.root.verify(&self.user).map_err(recovery)?;
        if self.is_ordinary_backup() {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        for name in self
            .root
            .directory()
            .read_children(100_000)
            .map_err(recovery)?
        {
            let name = name.os_string();
            if name != OsStr::new("control.lock") && name != OsStr::new("lifetime.lock") {
                // Even a valid previous terminal marker binds the old source
                // image, so an ordinary installer cannot safely inherit it.
                return Err(error("HISTORY_ORDINARY_EXISTING_RECOVERY"));
            }
        }
        self.root.verify(&self.user).map_err(recovery)
    }
    pub(crate) fn reopen_ordinary_backup(transaction_id: &str) -> Result<Arc<Self>, SafeError> {
        let local = dirs::data_local_dir().ok_or_else(|| error("HISTORY_STARTUP_UNAVAILABLE"))?;
        let parent = Directory::open_absolute(&local).map_err(unavailable)?;
        Self::backup_under(parent, transaction_id, false)
    }
    fn backup_under(
        parent: Arc<Directory>,
        transaction_id: &str,
        create: bool,
    ) -> Result<Arc<Self>, SafeError> {
        crate::version_history::journal::validate_id(transaction_id)?;
        let user = CurrentUser::capture().map_err(unavailable)?;
        let name = ComponentName::new(OsStr::new(BACKUP_DIRECTORY)).map_err(unavailable)?;
        let backup_parent =
            match PrivateDirectory::open_existing(parent.clone(), name.clone(), &user) {
                Ok(root) => root,
                Err(missing) if create && missing.kind() == std::io::ErrorKind::NotFound => {
                    match PrivateDirectory::create_new(parent.clone(), name.clone(), &user) {
                        Ok(root) => root,
                        Err(_) => PrivateDirectory::open_existing(parent.clone(), name, &user)
                            .map_err(recovery)?,
                    }
                }
                Err(failure) => return Err(recovery(failure)),
            };
        let backup_parent = Arc::new(backup_parent);
        backup_parent.verify(&user).map_err(recovery)?;
        let transaction = ComponentName::new(OsStr::new(transaction_id)).map_err(unavailable)?;
        let root = Arc::new(
            if create {
                // A collision preserves all evidence and fails. Unlike the stable
                // sibling root, a transaction child must never be silently reused.
                PrivateDirectory::create_new(backup_parent.directory().clone(), transaction, &user)
            } else {
                PrivateDirectory::open_existing(
                    backup_parent.directory().clone(),
                    transaction,
                    &user,
                )
            }
            .map_err(recovery)?,
        );
        root.verify(&user).map_err(recovery)?;
        let leases = LeaseFiles::open(root.clone(), &user).map_err(recovery)?;
        backup_parent.verify(&user).map_err(recovery)?;
        Ok(Arc::new(Self {
            parent,
            root,
            leases,
            user,
            backup_parent: Some(backup_parent),
        }))
    }
    /// Display the held fixed container containing both journal and complete
    /// data directories. Callers separately prove full backup completion first.
    pub(crate) fn ordinary_backup_location(&self) -> Result<String, SafeError> {
        let parent = self
            .backup_parent
            .as_ref()
            .ok_or_else(|| error("HISTORY_ROOT_CHANGED"))?;
        parent.verify(&self.user).map_err(recovery)?;
        self.root.verify(&self.user).map_err(recovery)?;
        let path =
            super::manager_process::launch_path(parent.directory().raw()).map_err(recovery)?;
        parent.verify(&self.user).map_err(recovery)?;
        path.into_string().map_err(recovery)
    }
    pub(crate) fn is_ordinary_backup(&self) -> bool {
        self.backup_parent.is_some()
    }
    /// This transaction UUID is a lookup observation, never a caller path or a
    /// mode claim. Exactly one fixed root must publish its matching marker.
    pub(crate) fn open_for_manager(transaction_id: &str) -> Result<Arc<Self>, SafeError> {
        let local = dirs::data_local_dir().ok_or_else(|| error("HISTORY_STARTUP_UNAVAILABLE"))?;
        Self::manager_under(
            Directory::open_absolute(&local).map_err(unavailable)?,
            transaction_id,
        )
    }
    fn manager_under(parent: Arc<Directory>, transaction_id: &str) -> Result<Arc<Self>, SafeError> {
        crate::version_history::journal::validate_id(transaction_id)?;
        let user = CurrentUser::capture().map_err(unavailable)?;
        // Probe optional roots without creating any directory or lease files.
        let global = match PrivateDirectory::open_existing(
            parent.clone(),
            ComponentName::new(OsStr::new(CONTROL_DIRECTORY)).map_err(unavailable)?,
            &user,
        ) {
            Ok(_) => Some(Self::open_under(parent.clone(), false)?),
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => None,
            Err(failure) => return Err(recovery(failure)),
        };
        let backup = match PrivateDirectory::open_existing(
            parent.clone(),
            ComponentName::new(OsStr::new(BACKUP_DIRECTORY)).map_err(unavailable)?,
            &user,
        ) {
            Ok(backup_parent) => {
                backup_parent.verify(&user).map_err(recovery)?;
                match PrivateDirectory::open_existing(
                    backup_parent.directory().clone(),
                    ComponentName::new(OsStr::new(transaction_id)).map_err(unavailable)?,
                    &user,
                ) {
                    Ok(_) => Some(Self::backup_under(parent, transaction_id, false)?),
                    Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => None,
                    Err(failure) => return Err(recovery(failure)),
                }
            }
            Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => None,
            Err(failure) => return Err(recovery(failure)),
        };
        let mut selected = None;
        for owner in [global, backup].into_iter().flatten() {
            let control = owner.acquire_control()?;
            let marker =
                MarkerStore::open_existing(owner.root().clone(), &control).map_err(recovery)?;
            let matches = marker
                .as_ref()
                .map(|marker| {
                    let marker = ActiveContextMarker::decode(marker.current().map_err(recovery)?)?;
                    Ok::<_, SafeError>(marker.binding().transaction_id == transaction_id)
                })
                .transpose()?
                .unwrap_or(false);
            drop(marker);
            drop(control);
            if matches && selected.replace(owner).is_some() {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
        }
        selected.ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))
    }
    pub(crate) fn root(&self) -> &Arc<PrivateDirectory> {
        &self.root
    }
    pub(crate) fn acquire_control(&self) -> Result<ControlLease, SafeError> {
        if let Some(parent) = &self.backup_parent {
            parent.verify(&self.user).map_err(recovery)?;
        }
        self.root.verify(&self.user).map_err(unavailable)?;
        self.leases.acquire_control().map_err(unavailable)
    }
    pub(crate) fn leases(&self) -> &LeaseFiles {
        &self.leases
    }
    #[cfg(test)]
    pub(crate) fn fixture(parent: &std::path::Path) -> Result<Arc<Self>, SafeError> {
        Self::open_under(Directory::open_absolute(parent).map_err(unavailable)?, true)
    }
    #[cfg(test)]
    pub(crate) fn fixture_admit(self: &Arc<Self>) -> Result<OrdinaryStartup, SafeError> {
        admit_under(self.clone())
    }
    #[cfg(test)]
    pub(crate) fn fixture_open_for_manager(
        &self,
        transaction_id: &str,
    ) -> Result<Arc<Self>, SafeError> {
        Self::manager_under(self.parent.clone(), transaction_id)
    }
}

/// The copied manager acquires these actual global OS leases only after source
/// exit. A private journal lease never substitutes for installation custody.
pub(crate) struct GlobalLeaseCustody {
    installation: Arc<InstallationControl>,
    control: ControlLease,
    exclusive: ExclusiveLease,
}
impl GlobalLeaseCustody {
    pub(crate) fn acquire(installation: Arc<InstallationControl>) -> Result<Self, SafeError> {
        if installation.is_ordinary_backup() {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        let control = installation.acquire_control()?;
        let exclusive = installation
            .leases()
            .acquire_exclusive(&control)
            .map_err(recovery)?;
        let custody = Self {
            installation,
            control,
            exclusive,
        };
        custody.verify()?;
        Ok(custody)
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.installation
            .root
            .verify(&self.installation.user)
            .map_err(recovery)?;
        self.control
            .verify_root(self.installation.root())
            .map_err(recovery)?;
        self.installation.require_ordinary_global_context()?;
        self.exclusive
            .verify_root(self.installation.root())
            .map_err(recovery)
    }
    pub(crate) fn release_at_installer_handoff(self) -> Result<(), SafeError> {
        self.verify()?;
        drop(self);
        Ok(())
    }
}

pub(crate) struct RegisteredStartupSource {
    installation: RegisteredInstallation,
    bundle: HeldBundle,
}
impl RegisteredStartupSource {
    pub(crate) fn capture() -> Result<Self, SafeError> {
        let installation =
            RegisteredInstallation::capture().map_err(|_| error("HISTORY_SCOPE_UNREGISTERED"))?;
        let bundle =
            HeldBundle::capture_registered_source(&installation, SnapshotLimits::default())
                .map_err(recovery)?;
        Ok(Self {
            installation,
            bundle,
        })
    }
    pub(crate) fn installation(&self) -> &RegisteredInstallation {
        &self.installation
    }
    pub(crate) fn bundle(&self) -> &HeldBundle {
        &self.bundle
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransactionDataReference {
    schema: u32,
    transaction_id: String,
    identity: FileIdentity,
}
/// Private sibling of the stable control directory. Complete bundle/package/
/// UDF/receipt material cannot pollute the journal's strict flat namespace.
pub(crate) struct TransactionDataRoot {
    root: Arc<PrivateDirectory>,
    reference: TransactionDataReference,
    _installation: Arc<InstallationControl>,
}
impl TransactionDataRoot {
    fn parent(installation: &InstallationControl) -> Arc<Directory> {
        installation
            .backup_parent
            .as_ref()
            .map(|parent| parent.directory().clone())
            .unwrap_or_else(|| installation.parent.clone())
    }
    fn name(transaction_id: &str) -> Result<ComponentName, SafeError> {
        crate::version_history::journal::validate_id(transaction_id)?;
        ComponentName::new(OsStr::new(&format!("CCDesk-VersionData-{transaction_id}")))
            .map_err(unavailable)
    }
    pub(crate) fn create(
        installation: Arc<InstallationControl>,
        control: &ControlLease,
        transaction_id: &str,
    ) -> Result<Self, SafeError> {
        control.verify_root(installation.root()).map_err(recovery)?;
        let root = Arc::new(
            PrivateDirectory::create_new(
                Self::parent(&installation),
                Self::name(transaction_id)?,
                &installation.user,
            )
            .map_err(unavailable)?,
        );
        root.verify(&installation.user).map_err(recovery)?;
        control.verify_root(installation.root()).map_err(recovery)?;
        let reference = TransactionDataReference {
            schema: 1,
            transaction_id: transaction_id.into(),
            identity: root.directory().identity().clone(),
        };
        Ok(Self {
            root,
            reference,
            _installation: installation,
        })
    }
    /// The reference must come from the protected transaction material. It is a
    /// lookup observation only: this factory reopens the fixed name, checks its
    /// actual current-user private ACL and matches the exact retained root ID.
    pub(crate) fn reopen(
        installation: Arc<InstallationControl>,
        control: &ControlLease,
        transaction_id: &str,
        reference: TransactionDataReference,
    ) -> Result<Self, SafeError> {
        crate::version_history::journal::validate_id(transaction_id)?;
        if reference.schema != 1 || reference.transaction_id != transaction_id {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        control.verify_root(installation.root()).map_err(recovery)?;
        let root = Arc::new(
            PrivateDirectory::open_existing(
                Self::parent(&installation),
                Self::name(&reference.transaction_id)?,
                &installation.user,
            )
            .map_err(recovery)?,
        );
        if root.directory().identity() != &reference.identity {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        root.verify(&installation.user).map_err(recovery)?;
        control.verify_root(installation.root()).map_err(recovery)?;
        Ok(Self {
            root,
            reference,
            _installation: installation,
        })
    }
    pub(crate) fn root(&self) -> &Arc<PrivateDirectory> {
        &self.root
    }
    pub(crate) fn transaction_id(&self) -> &str {
        &self.reference.transaction_id
    }
    pub(crate) fn reference(&self) -> &TransactionDataReference {
        &self.reference
    }
    pub(crate) fn verify(&self) -> Result<(), SafeError> {
        self.root
            .verify(&self._installation.user)
            .map_err(recovery)?;
        if self.root.directory().identity() != &self.reference.identity {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        Ok(())
    }
    pub(crate) fn verify_installation(
        &self,
        installation: &InstallationControl,
    ) -> Result<(), SafeError> {
        self.verify()?;
        if self._installation.root().directory().identity()
            != installation.root().directory().identity()
        {
            return Err(error("HISTORY_ROOT_CHANGED"));
        }
        Ok(())
    }
}
pub(crate) struct OrdinaryStartupGuards {
    control: Arc<InstallationControl>,
    shared: SharedLease,
    // Registry handles remain local until startup admission completes. The
    // managed lifetime retains only the actual complete bundle and OS leases.
    source: Option<HeldBundle>,
}
pub(crate) struct OrdinaryStartup {
    guards: Arc<OrdinaryStartupGuards>,
    _admission: Option<SharedStartupLease>,
}
impl OrdinaryStartup {
    pub(crate) fn control(&self) -> &Arc<InstallationControl> {
        &self.guards.control
    }
    pub(crate) fn shared(&self) -> &SharedLease {
        &self.guards.shared
    }
    pub(crate) fn retained_startup_bundle(&self) -> Result<&HeldBundle, SafeError> {
        let source = self
            .guards
            .source
            .as_ref()
            .ok_or_else(|| error("HISTORY_SCOPE_UNREGISTERED"))?;
        source.tree().verify().map_err(recovery)?;
        Ok(source)
    }
    pub(crate) fn capture_switch_source(&self) -> Result<RegisteredStartupSource, SafeError> {
        self.guards
            .shared
            .verify_root(self.guards.control.root())
            .map_err(recovery)?;
        RegisteredStartupSource::capture()
    }
}

/// Only this module can construct the evidence. The maintenance factory consumes
/// the actual registered image/bundle, root and OS guards, not caller digests.
pub(crate) struct StartupLeaseEvidence {
    user_installation: String,
    actual_bundle: Option<String>,
    control: ControlLease,
    guards: Arc<OrdinaryStartupGuards>,
}
pub(crate) struct StartupLeaseParts {
    pub(crate) user_installation: String,
    pub(crate) actual_bundle: Option<String>,
    pub(crate) control: Box<dyn Send + Sync>,
    pub(crate) shared: Box<dyn Send + Sync>,
}
impl StartupLeaseEvidence {
    fn capture(
        guards: Arc<OrdinaryStartupGuards>,
        control: ControlLease,
        registered: Option<&RegisteredInstallation>,
    ) -> Result<Self, SafeError> {
        let root = guards.control.root();
        control.verify_root(root).map_err(recovery)?;
        guards.shared.verify_root(root).map_err(recovery)?;
        let (user_installation, actual_bundle) = match (&guards.source, registered) {
            (Some(bundle), Some(installation)) => {
                installation.recheck().map_err(recovery)?;
                bundle.tree().verify().map_err(recovery)?;
                root.directory()
                    .require_disjoint(&[installation.directory().clone()])
                    .map_err(recovery)?;
                (
                    sha256(
                        &serde_json::to_vec(&(
                            PRODUCT_IDENTIFIER,
                            guards.control.user.sid_text(),
                            installation
                                .original_path()
                                .as_os_str()
                                .encode_wide()
                                .collect::<Vec<_>>(),
                        ))
                        .map_err(recovery)?,
                    ),
                    Some(bundle.manifest().logical_digest().map_err(recovery)?),
                )
            }
            (None, None) => (
                // User/control-root binding only, without a switch-scope claim.
                sha256(
                    &serde_json::to_vec(&(
                        PRODUCT_IDENTIFIER,
                        guards.control.user.sid_text(),
                        root.directory().identity(),
                    ))
                    .map_err(recovery)?,
                ),
                None,
            ),
            _ => return Err(error("HISTORY_SCOPE_UNREGISTERED")),
        };
        control.verify_root(root).map_err(recovery)?;
        guards.shared.verify_root(root).map_err(recovery)?;
        Ok(Self {
            user_installation,
            actual_bundle,
            control,
            guards,
        })
    }
    pub(crate) fn into_parts(self) -> StartupLeaseParts {
        StartupLeaseParts {
            user_installation: self.user_installation,
            actual_bundle: self.actual_bundle,
            control: Box::new(self.control),
            shared: Box::new(self.guards),
        }
    }
}

pub(crate) fn admit_ordinary() -> Result<OrdinaryStartup, SafeError> {
    let executable = std::env::current_exe().map_err(unavailable)?;
    if executable
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case(MANAGER_BASENAME))
    {
        return Err(error("HISTORY_MANAGER_ENTRY_REQUIRED"));
    }
    let installation_control = InstallationControl::open(true)?;
    admit_under(installation_control)
}
fn admit_under(
    installation_control: Arc<InstallationControl>,
) -> Result<OrdinaryStartup, SafeError> {
    let control = installation_control.acquire_control()?;
    let shared = installation_control
        .leases
        .acquire_shared(&control)
        .map_err(unavailable)?;
    let marker_bytes = {
        let marker = MarkerStore::open_existing(installation_control.root.clone(), &control)
            .map_err(recovery)?;
        marker
            .map(|marker| marker.current().map(<[u8]>::to_vec))
            .transpose()
            .map_err(recovery)?
    };
    // A missing marker next to existing transaction evidence is not a fresh
    // installation. Preserve it and require reconciliation, never recreate it.
    if marker_bytes.is_none() {
        for name in installation_control
            .root
            .directory()
            .read_children(100_000)
            .map_err(recovery)?
        {
            let name = name.os_string();
            if name != OsStr::new("control.lock") && name != OsStr::new("lifetime.lock") {
                return Err(error("HISTORY_RECOVERY_REQUIRED"));
            }
        }
    }
    // Unsupported switching scope must not disable ordinary release/debug
    // startup when the actual root has no marker or orphan transaction data.
    if marker_bytes.is_none() {
        let guards = Arc::new(OrdinaryStartupGuards {
            control: installation_control,
            shared,
            source: None,
        });
        let evidence = StartupLeaseEvidence::capture(guards.clone(), control, None)?;
        let admission = crate::version_history::maintenance::admit_windows_startup(
            evidence,
            MarkerRead::Absent,
        )?;
        return Ok(OrdinaryStartup {
            guards,
            _admission: Some(admission),
        });
    }
    let marker = ActiveContextMarker::decode(
        marker_bytes
            .as_deref()
            .ok_or_else(|| error("HISTORY_RECOVERY_REQUIRED"))?,
    )?;
    if !marker.is_terminal() {
        return Err(error("HISTORY_RECOVERY_REQUIRED"));
    }
    let source = RegisteredInstallation::capture().and_then(|installation| {
        let bundle =
            HeldBundle::capture_registered_source(&installation, SnapshotLimits::default())
                .map_err(|_| super::scope::ScopeBlock::InputChanged)?;
        Ok(RegisteredStartupSource {
            installation,
            bundle,
        })
    });
    let RegisteredStartupSource {
        installation,
        bundle,
    } = source.map_err(|_| error("HISTORY_SCOPE_UNREGISTERED"))?;
    let guards = Arc::new(OrdinaryStartupGuards {
        control: installation_control.clone(),
        shared,
        source: Some(bundle),
    });
    let journal = {
        let journal = JournalStore::open_windows_transaction(
            installation_control.root.clone(),
            &marker.binding().transaction_id,
        )?;
        let inspection = journal.inspect(marker.binding())?;
        Some((journal, inspection))
    };
    let marker_read = match (&marker_bytes, &journal) {
        (None, _) => MarkerRead::Absent,
        (Some(bytes), Some((_, inspection))) => MarkerRead::Present {
            bytes,
            journal: Some(inspection),
        },
        _ => MarkerRead::Unreadable,
    };
    // This exact local scope (including registry guards) is still held through
    // the final marker/journal decision. It is not sent into managed state.
    installation.recheck().map_err(recovery)?;
    let evidence = StartupLeaseEvidence::capture(guards.clone(), control, Some(&installation))?;
    let admission =
        crate::version_history::maintenance::admit_windows_startup(evidence, marker_read)?;
    installation.recheck().map_err(recovery)?;
    Ok(OrdinaryStartup {
        guards,
        _admission: Some(admission),
    })
}
