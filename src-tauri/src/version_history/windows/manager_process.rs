//! Dedicated initial-manager handoff. This does not borrow an exclusive lease:
//! the original source is alive and retains its real shared lifetime guard.
//! Every launch is create-new, suspended, durably identified and resumed once.
use super::{
    blocked,
    files::{FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    lease::{ControlLease, SharedLease},
    manager_bundle::{
        name, valid_transaction, ManagerBundle, ManagerRecord, ManagerRecordReference,
        MANAGER_BASENAME,
    },
    own,
    package::RetainedPackage,
    process::{ExactProcess, ProcessIdentity},
    scope::RegisteredInstallation,
    security::CurrentUser,
    win_error,
};
use crate::version_history::{catalog::CatalogService, verified_package::sha256};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    io,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::OwnedHandle,
    },
    sync::Arc,
};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    Storage::FileSystem::{GetFinalPathNameByHandleW, VOLUME_NAME_DOS},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{
            CreateProcessW, GetCurrentProcess, GetCurrentProcessId, ResumeThread, TerminateProcess,
            WaitForSingleObject, CREATE_BREAKAWAY_FROM_JOB, CREATE_SUSPENDED,
            CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION, STARTUPINFOW,
        },
    },
};
use windows_core::{BOOL, PCWSTR, PWSTR};

const LAUNCH: &str = "manager-launch.json";
const PROCESS: &str = "manager-process.json";
const RESUME: &str = "manager-resume.json";
const READY: &str = "manager-ready.json";

// Launch spelling comes only from a retained object. Context paths use volume
// GUIDs for identity comparison; process observation requires a DOS spelling.
// UNC and unmapped-volume results block rather than discovering another path.
pub(super) fn launch_path(file: HANDLE) -> io::Result<OsString> {
    let mut path = vec![0u16; 32768];
    let length = unsafe { GetFinalPathNameByHandleW(file, &mut path, VOLUME_NAME_DOS) } as usize;
    if length == 0 {
        return Err(io::Error::last_os_error());
    }
    if length >= path.len() {
        return Err(blocked("manager image path exceeds capacity"));
    }
    path.truncate(length);
    let prefix: Vec<_> = "\\\\?\\".encode_utf16().collect();
    if path.starts_with(&prefix) {
        path = path.split_off(prefix.len());
    }
    super::files::validate_absolute(&path)?;
    Ok(OsString::from_wide(&path))
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaunchBinding {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    bundle: ManagerRecordReference,
    package_root: FileIdentity,
    package_digest: String,
    source: ProcessIdentity,
    command_digest: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessBinding {
    schema: u32,
    transaction: String,
    launch: ManagerRecordReference,
    process: ProcessIdentity,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeBinding {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    launch: ManagerRecordReference,
    process: ManagerRecordReference,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyBinding {
    schema: u32,
    transaction: String,
    data_root: FileIdentity,
    admission: ManagerRecordReference,
    manager: ProcessIdentity,
    source: ProcessIdentity,
    bundle: ManagerRecordReference,
    package_digest: String,
}

fn manager_command(image: &OsStr, transaction: &str) -> io::Result<String> {
    valid_transaction(transaction)?;
    let path = image
        .to_str()
        .ok_or_else(|| blocked("manager path is unrepresentable"))?;
    let units: Vec<_> = image.encode_wide().collect();
    super::files::validate_absolute(&units)?;
    if path.contains(['"', '\r', '\n'])
        || !std::path::Path::new(image)
            .file_name()
            .is_some_and(|name| name == OsStr::new(MANAGER_BASENAME))
    {
        return Err(blocked("manager executable spelling differs"));
    }
    let command = format!("\"{path}\" --version-manager {transaction}");
    if command.encode_utf16().count() + 1 > 32767 {
        return Err(blocked("manager command exceeds capacity"));
    }
    Ok(command)
}
fn running(process: &ExactProcess) -> io::Result<()> {
    if process.terminal(0)?.is_some() {
        return Err(blocked("required handoff process already exited"));
    }
    Ok(())
}
fn stop_suspended(process: &OwnedHandle) -> io::Result<()> {
    unsafe { TerminateProcess(handle(process), 1) }.map_err(win_error)?;
    if unsafe { WaitForSingleObject(handle(process), 5000) } != WAIT_OBJECT_0 {
        return Err(blocked("never-resumed manager cleanup remains unknown"));
    }
    Ok(())
}
fn in_job(process: HANDLE) -> io::Result<bool> {
    let mut assigned = BOOL(0);
    unsafe { IsProcessInJob(process, None, &mut assigned) }.map_err(win_error)?;
    Ok(assigned.as_bool())
}
struct NeverResumed(Option<OwnedHandle>);
impl Drop for NeverResumed {
    fn drop(&mut self) {
        if let Some(process) = &self.0 {
            let _ = stop_suspended(process);
        }
    }
}

// Private production wrapper: no arbitrary executable arguments or shell.
fn create_manager_process(
    image: PinnedFile,
    transaction: &str,
) -> io::Result<(ExactProcess, OwnedHandle, NeverResumed)> {
    image.verify()?;
    let application = launch_path(handle(&image.file))?;
    let command = manager_command(&application, transaction)?;
    let app: Vec<_> = application.encode_wide().chain(Some(0)).collect();
    let mut line: Vec<_> = command.encode_utf16().chain(Some(0)).collect();
    let cwd: Vec<_> = launch_path(image.parent.raw())?
        .encode_wide()
        .chain(Some(0))
        .collect();
    let startup = STARTUPINFOW {
        cb: size_of::<STARTUPINFOW>() as u32,
        ..Default::default()
    };
    let mut created = PROCESS_INFORMATION::default();
    // A manager inside a launcher's kill-on-close job would die when its source
    // exits. Require real breakaway where necessary; unsupported containment
    // fails creation instead of falsely acknowledging durable independence.
    let mut flags = CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT;
    if in_job(unsafe { GetCurrentProcess() })? {
        flags |= CREATE_BREAKAWAY_FROM_JOB;
    }
    unsafe {
        CreateProcessW(
            PCWSTR(app.as_ptr()),
            Some(PWSTR(line.as_mut_ptr())),
            None,
            None,
            false,
            flags,
            None,
            PCWSTR(cwd.as_ptr()),
            &startup,
            &mut created,
        )
    }
    .map_err(win_error)?;
    let pending = NeverResumed(Some(unsafe { own(created.hProcess) }));
    let thread = unsafe { own(created.hThread) };
    if in_job(handle(pending.0.as_ref().expect("created process")))? {
        return Err(blocked("manager remains dependent on a source job"));
    }
    let process = ExactProcess::from_created(
        pending.0.as_ref().expect("created process").try_clone()?,
        image,
    )?;
    if process.pid() != created.dwProcessId {
        return Err(blocked("created manager identity differs"));
    }
    running(&process)?;
    Ok((process, thread, pending))
}

/// Owns the initial manager, never a generic command or installer job. Dropping
/// before any resume intent may stop only its definitely suspended own child.
/// Once an intent can have been published, Drop never kills or resumes it.
pub(crate) struct PreparedManager<'a> {
    bundle: Arc<ManagerBundle>,
    source: ExactProcess,
    process: ExactProcess,
    thread: OwnedHandle,
    suspended: NeverResumed,
    launch_record: ManagerRecord,
    process_record: ManagerRecord,
    launch: LaunchBinding,
    control_root: Arc<PrivateDirectory>,
    shared: &'a SharedLease,
    resume_prepared: bool,
    resume_called: bool,
    resume: Option<ManagerRecord>,
}
pub(crate) struct ManagerResumeAdmission {
    reference: ManagerRecordReference,
    transaction: String,
    process: ProcessIdentity,
}
impl ManagerResumeAdmission {
    pub(crate) fn reference(&self) -> &ManagerRecordReference {
        &self.reference
    }
    pub(super) fn transaction(&self) -> &str {
        &self.transaction
    }
}
impl<'a> PreparedManager<'a> {
    pub(crate) fn create_suspended(
        bundle: Arc<ManagerBundle>,
        installation: &RegisteredInstallation,
        package: &RetainedPackage,
        control_root: Arc<PrivateDirectory>,
        control: &ControlLease,
        shared: &'a SharedLease,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        user.require_unelevated()?;
        control.verify_root(&control_root)?;
        shared.verify_root(&control_root)?;
        bundle.verify(user)?;
        installation
            .recheck()
            .map_err(|_| blocked("source installation changed"))?;
        bundle.verify_source_image(installation.image())?;
        let source = ExactProcess::capture_observed(unsafe { GetCurrentProcessId() })?;
        source.verify_held_image(installation.image())?;
        running(&source)?;
        package
            .verify_retained()
            .map_err(|_| blocked("retained package changed"))?;
        if package.transaction_id() != bundle.transaction() {
            return Err(blocked("package transaction differs"));
        }
        let package_root = PrivateDirectory::open_existing(
            bundle.data_root().directory().clone(),
            name("package")?,
            user,
        )?;
        if package_root.directory().identity() != package.root_identity() {
            return Err(blocked("package root differs"));
        }
        let image = bundle.reopen_image(user)?;
        let application = launch_path(handle(&image.file))?;
        let command = manager_command(&application, bundle.transaction())?;
        let launch = LaunchBinding {
            schema: 1,
            transaction: bundle.transaction().into(),
            data_root: bundle.data_root().directory().identity().clone(),
            bundle: bundle.reference().clone(),
            package_root: package.root_identity().clone(),
            package_digest: package.record_digest(),
            source: source.identity().clone(),
            command_digest: sha256(command.as_bytes()),
        };
        let root = bundle.data_root().clone();
        // The fixed create-new intent prevents replay after a lost creation
        // result. A later invocation must reconcile, not call CreateProcess.
        let launch_record = ManagerRecord::create(root.clone(), LAUNCH, &launch, user)?;
        control.verify_root(&control_root)?;
        shared.verify_root(&control_root)?;
        bundle.verify(user)?;
        source.verify_held_image(installation.image())?;
        running(&source)?;
        let (process, thread, pending) = create_manager_process(image, bundle.transaction())?;
        if process.identity().session() != source.identity().session() {
            return Err(blocked("created manager session differs"));
        }
        let process_record = ManagerRecord::create(
            root,
            PROCESS,
            &ProcessBinding {
                schema: 1,
                transaction: launch.transaction.clone(),
                launch: launch_record.reference().clone(),
                process: process.identity().clone(),
            },
            user,
        )?;
        Ok(Self {
            bundle,
            source,
            process,
            thread,
            suspended: pending,
            launch_record,
            process_record,
            launch,
            control_root,
            shared,
            resume_prepared: false,
            resume_called: false,
            resume: None,
        })
    }
    fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.shared.verify_root(&self.control_root)?;
        self.bundle.verify(user)?;
        self.launch_record.verify(user)?;
        self.process_record.verify(user)?;
        self.process.verify_held_image(self.bundle.image())?;
        running(&self.source)?;
        running(&self.process)
    }
    pub(crate) fn launch_reference(&self) -> &ManagerRecordReference {
        self.launch_record.reference()
    }
    pub(crate) fn process_reference(&self) -> &ManagerRecordReference {
        self.process_record.reference()
    }
    /// Publish the returned admission through publish_initial_handoff before
    /// resume_once. Even a partial resume-intent write poisons this owner.
    pub(crate) fn prepare_resume(
        &mut self,
        control: &ControlLease,
        user: &CurrentUser,
    ) -> io::Result<ManagerResumeAdmission> {
        if self.resume_prepared || self.resume_called {
            return Err(blocked("manager resume already attempted"));
        }
        self.verify(user)?;
        control.verify_root(&self.control_root)?;
        self.resume_prepared = true;
        drop(self.suspended.0.take());
        let resume = ManagerRecord::create(
            self.bundle.data_root().clone(),
            RESUME,
            &ResumeBinding {
                schema: 1,
                transaction: self.launch.transaction.clone(),
                data_root: self.launch.data_root.clone(),
                launch: self.launch_record.reference().clone(),
                process: self.process_record.reference().clone(),
            },
            user,
        )?;
        let admission = ManagerResumeAdmission {
            reference: resume.reference().clone(),
            transaction: self.launch.transaction.clone(),
            process: self.process.identity().clone(),
        };
        self.resume = Some(resume);
        Ok(admission)
    }
    pub(crate) fn resume_once(
        &mut self,
        published: super::manager_handoff::PublishedManagerHandoff,
        user: &CurrentUser,
    ) -> io::Result<()> {
        let admission = published.admission();
        self.verify(user)?;
        let resume = self
            .resume
            .as_ref()
            .ok_or_else(|| blocked("manager resume lacks durable admission"))?;
        resume.verify(user)?;
        if !self.resume_prepared
            || self.resume_called
            || admission.transaction != self.launch.transaction
            || admission.process != *self.process.identity()
            || &admission.reference != resume.reference()
        {
            return Err(blocked("manager resume admission differs"));
        }
        self.resume_called = true;
        if unsafe { ResumeThread(handle(&self.thread)) } != 1 {
            return Err(blocked("manager resume outcome unknown"));
        }
        Ok(())
    }
    pub(crate) fn observe_ready(
        &self,
        user: &CurrentUser,
    ) -> io::Result<Option<ManagerReadyReceipt>> {
        self.verify(user)?;
        if !self.resume_called {
            return Err(blocked("manager has not been resumed"));
        }
        let admission = self
            .resume
            .as_ref()
            .ok_or_else(|| blocked("manager admission missing"))?;
        let record = match ManagerRecord::observe(self.bundle.data_root().clone(), READY, user) {
            Ok(record) => record,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let observation = ManagerReadyObservation::validate(
            record,
            &self.launch,
            admission.reference(),
            self.process.identity(),
            user,
        )?;
        self.verify(user)?;
        let receipt = ManagerReadyReceipt::retain_live(
            observation,
            self.source.identity(),
            self.process.identity(),
        )?;
        Ok(Some(receipt))
    }
}

/// Actual child ownership and re-admitted files stay alive after publishing the
/// receipt. No caller boolean or deserialized observation can construct this.
pub(crate) struct ManagerChildAdmission {
    bundle: ManagerBundle,
    package: RetainedPackage,
    manager: ExactProcess,
    source: Option<ExactProcess>,
    _launch: ManagerRecord,
    _process: ManagerRecord,
    _resume: ManagerRecord,
    launch: LaunchBinding,
    ui: Option<super::manager_ui::ManagerUiReady>,
    ready_attempted: bool,
    ready: Option<ManagerRecord>,
}
impl ManagerChildAdmission {
    pub(crate) fn admit(
        data_root: Arc<PrivateDirectory>,
        transaction: &str,
        expected_admission: &ManagerRecordReference,
        catalog: &CatalogService,
    ) -> io::Result<Self> {
        valid_transaction(transaction)?;
        let user = CurrentUser::capture()?;
        user.require_unelevated()?;
        data_root.verify(&user)?;
        let resume_record =
            ManagerRecord::open(data_root.clone(), RESUME, expected_admission, &user)?;
        let resume: ResumeBinding = resume_record.decode(&user)?;
        if resume.schema != 1
            || resume.transaction != transaction
            || resume.data_root != *data_root.directory().identity()
        {
            return Err(blocked("manager resume belongs to another transaction"));
        }
        let launch_record = ManagerRecord::open(data_root.clone(), LAUNCH, &resume.launch, &user)?;
        let launch: LaunchBinding = launch_record.decode(&user)?;
        let process_record =
            ManagerRecord::open(data_root.clone(), PROCESS, &resume.process, &user)?;
        let process: ProcessBinding = process_record.decode(&user)?;
        if launch.schema != 1
            || process.schema != 1
            || launch.transaction != transaction
            || process.transaction != transaction
            || process.launch != resume.launch
            || launch.data_root != resume.data_root
        {
            return Err(blocked("manager receipt chain differs"));
        }
        launch.source.validate()?;
        process.process.validate()?;
        let bundle = ManagerBundle::reopen(data_root.clone(), transaction, &launch.bundle, &user)?;
        let current = ExactProcess::capture_observed(unsafe { GetCurrentProcessId() })?;
        current.verify_held_image(bundle.image())?;
        if current.identity() != &process.process
            || current.identity().session() != launch.source.session()
            || sha256(
                manager_command(&launch_path(handle(&bundle.image().file))?, transaction)?
                    .as_bytes(),
            ) != launch.command_digest
        {
            return Err(blocked("current manager is not the exact launched child"));
        }
        running(&current)?;
        if in_job(unsafe { GetCurrentProcess() })? {
            return Err(blocked("manager lifetime is still job-dependent"));
        }
        let source = ExactProcess::reopen(&launch.source)?;
        running(&source)?;
        let package_root = Arc::new(PrivateDirectory::open_existing(
            data_root.directory().clone(),
            name("package")?,
            &user,
        )?);
        if package_root.directory().identity() != &launch.package_root {
            return Err(blocked("retained package root changed"));
        }
        // This factory fetches fresh official metadata and verifies the actual
        // retained bytes/signature with the production key. Transfer is not
        // payload/install authority, and no installer is launched here.
        let package =
            RetainedPackage::reopen(package_root, transaction, &launch.package_digest, catalog)
                .map_err(|_| blocked("manager package re-admission failed"))?;
        bundle.verify(&user)?;
        package
            .verify_retained()
            .map_err(|_| blocked("manager package changed"))?;
        running(&source)?;
        running(&current)?;
        Ok(Self {
            bundle,
            package,
            manager: current,
            source: Some(source),
            _launch: launch_record,
            _process: process_record,
            _resume: resume_record,
            launch,
            ui: None,
            ready_attempted: false,
            ready: None,
        })
    }
    /// Consumes the live UI proof so a published ready receipt never outlives
    /// this owner's actual UDF/document/controller guards. Failure is no-replay.
    pub(crate) fn publish_ready(
        &mut self,
        ui: super::manager_ui::ManagerUiReady,
    ) -> io::Result<&ManagerRecordReference> {
        if self.ready_attempted {
            return Err(blocked("manager readiness already attempted"));
        }
        ui.require_publication_thread()?;
        self.verify_material()?;
        ui.verify_for(
            &self.launch.transaction,
            &self.manager,
            self.bundle.data_root(),
        )?;
        running(self.source()?)?;
        running(&self.manager)?;
        self.ui = Some(ui);
        self.ready_attempted = true;
        let user = CurrentUser::capture()?;
        let ready = ManagerRecord::create(
            self.bundle.data_root().clone(),
            READY,
            &ReadyBinding {
                schema: 1,
                transaction: self.launch.transaction.clone(),
                data_root: self.launch.data_root.clone(),
                admission: self._resume.reference().clone(),
                manager: self.manager.identity().clone(),
                source: self.source()?.identity().clone(),
                bundle: self.launch.bundle.clone(),
                package_digest: self.launch.package_digest.clone(),
            },
            &user,
        )?;
        self.ui.as_ref().expect("retained UI proof").verify_for(
            &self.launch.transaction,
            &self.manager,
            self.bundle.data_root(),
        )?;
        running(self.source()?)?;
        self.ready = Some(ready);
        self.ready_reference()
    }
    pub(crate) fn ready_reference(&self) -> io::Result<&ManagerRecordReference> {
        self.ready
            .as_ref()
            .map(ManagerRecord::reference)
            .ok_or_else(|| blocked("manager UI has not published readiness"))
    }
    pub(crate) fn source(&self) -> io::Result<&ExactProcess> {
        self.source
            .as_ref()
            .ok_or_else(|| blocked("source terminal evidence already transferred"))
    }
    /// The caller may then combine this exact terminal source handle with its
    /// matching WebView exit receipt. No PID disappearance is accepted.
    pub(crate) fn take_exited_source(&mut self) -> io::Result<ExactProcess> {
        let source = self.source()?;
        if source.terminal(0)?.is_none() {
            return Err(blocked("original source is still alive"));
        }
        self.ui
            .as_ref()
            .ok_or_else(|| blocked("manager UI readiness is absent"))?
            .release_after_source_exit(source)?;
        Ok(self.source.take().expect("checked source"))
    }
    pub(crate) fn selection(&self) -> &crate::version_history::catalog::SelectionMetadata {
        self.package.selection()
    }
    pub(crate) fn source_terminal(&self) -> io::Result<bool> {
        self.source()?
            .terminal(0)
            .map(|terminal| terminal.is_some())
    }
    pub(crate) fn release_ui_after_source_exit(&self) -> io::Result<()> {
        self.ui
            .as_ref()
            .ok_or_else(|| blocked("manager UI readiness is absent"))?
            .release_after_source_exit(self.source()?)
    }
    pub(crate) fn verify_material(&self) -> io::Result<()> {
        let user = CurrentUser::capture()?;
        if in_job(unsafe { GetCurrentProcess() })? {
            return Err(blocked("manager lifetime became job-dependent"));
        }
        self.bundle.verify(&user)?;
        self.package
            .verify_retained()
            .map_err(|_| blocked("manager package changed"))?;
        self.manager.verify_held_image(self.bundle.image())?;
        if let Some(ui) = &self.ui {
            ui.verify_for(
                &self.launch.transaction,
                &self.manager,
                self.bundle.data_root(),
            )?;
        }
        if let Some(ready) = &self.ready {
            ready.verify(&user)?;
        }
        Ok(())
    }
}

/// A historical acknowledgement is distinct from current handoff permission.
pub(crate) struct ManagerReadyObservation {
    record: ManagerRecord,
}
impl ManagerReadyObservation {
    fn validate(
        record: ManagerRecord,
        launch: &LaunchBinding,
        admission: &ManagerRecordReference,
        manager: &ProcessIdentity,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let ready: ReadyBinding = record.decode(user)?;
        ready.source.validate()?;
        ready.manager.validate()?;
        crate::version_history::journal::validate_digest(&launch.package_digest)
            .map_err(|_| blocked("invalid retained package digest"))?;
        crate::version_history::journal::validate_digest(&launch.command_digest)
            .map_err(|_| blocked("invalid retained manager command digest"))?;
        if ready.schema != 1
            || ready.transaction != launch.transaction
            || ready.data_root != launch.data_root
            || &ready.admission != admission
            || &ready.manager != manager
            || ready.source != launch.source
            || ready.bundle != launch.bundle
            || ready.package_digest != launch.package_digest
        {
            return Err(blocked("manager readiness belongs to another handoff"));
        }
        Ok(Self { record })
    }
    pub(crate) fn reference(&self) -> &ManagerRecordReference {
        self.record.reference()
    }
    /// Reopen a journal-bound child acknowledgement for recovery observation.
    /// This never replays creation/resume and never proves a dead PID succeeded.
    pub(crate) fn reopen(
        data_root: Arc<PrivateDirectory>,
        transaction: &str,
        expected_admission: &ManagerRecordReference,
        expected_ready: &ManagerRecordReference,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        valid_transaction(transaction)?;
        let resume_record =
            ManagerRecord::open(data_root.clone(), RESUME, expected_admission, user)?;
        let resume: ResumeBinding = resume_record.decode(user)?;
        if resume.schema != 1
            || resume.transaction != transaction
            || resume.data_root != *data_root.directory().identity()
        {
            return Err(blocked("manager readiness root differs"));
        }
        let launch_record = ManagerRecord::open(data_root.clone(), LAUNCH, &resume.launch, user)?;
        let launch: LaunchBinding = launch_record.decode(user)?;
        let process_record =
            ManagerRecord::open(data_root.clone(), PROCESS, &resume.process, user)?;
        let process: ProcessBinding = process_record.decode(user)?;
        if launch.schema != 1
            || launch.transaction != transaction
            || launch.data_root != resume.data_root
            || process.schema != 1
            || process.transaction != transaction
            || process.launch != resume.launch
        {
            return Err(blocked("manager readiness receipt chain differs"));
        }
        let record = ManagerRecord::open(data_root, READY, expected_ready, user)?;
        Self::validate(record, &launch, expected_admission, &process.process, user)
    }
}

/// This owner retains both exact living participants in addition to the
/// child's durable acknowledgement. Verify immediately before source close.
pub(crate) struct ManagerReadyReceipt {
    observation: ManagerReadyObservation,
    source: ExactProcess,
    manager: ExactProcess,
}
impl ManagerReadyReceipt {
    pub(crate) fn verify_for_source(
        &self,
        transaction: &str,
        source: &ExactProcess,
        data_root: &PrivateDirectory,
    ) -> io::Result<()> {
        self.verify()?;
        let user = CurrentUser::capture()?;
        data_root.verify(&user)?;
        let ready: ReadyBinding = self.observation.record.decode(&user)?;
        if ready.transaction != transaction
            || &ready.source != source.identity()
            || &ready.data_root != data_root.directory().identity()
        {
            return Err(blocked(
                "manager readiness belongs to another source transaction",
            ));
        }
        Ok(())
    }
    fn retain_live(
        observation: ManagerReadyObservation,
        source: &ProcessIdentity,
        manager: &ProcessIdentity,
    ) -> io::Result<Self> {
        let result = Self {
            observation,
            source: ExactProcess::reopen(source)?,
            manager: ExactProcess::reopen(manager)?,
        };
        result.verify()?;
        Ok(result)
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self.observation.record.verify(&CurrentUser::capture()?)?;
        running(&self.source)?;
        running(&self.manager)
    }
    /// Reconcile a lost source-side completion only in the exact still-living
    /// original source. Historical observations alone never authorize closing
    /// some later process, and this path never creates or resumes a manager.
    pub(crate) fn reopen_live(
        data_root: Arc<PrivateDirectory>,
        transaction: &str,
        admission: &ManagerRecordReference,
        ready: &ManagerRecordReference,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let observation =
            ManagerReadyObservation::reopen(data_root, transaction, admission, ready, user)?;
        let binding: ReadyBinding = observation.record.decode(user)?;
        let current = ExactProcess::capture_observed(unsafe { GetCurrentProcessId() })?;
        if current.identity() != &binding.source {
            return Err(blocked(
                "only the exact original source may reconcile live readiness",
            ));
        }
        Self::retain_live(observation, &binding.source, &binding.manager)
    }
    pub(crate) fn reference(&self) -> &ManagerRecordReference {
        self.observation.reference()
    }
}

#[cfg(test)]
#[path = "../../tests/version_history_manager_process_windows.rs"]
#[allow(non_snake_case)]
mod tests;
