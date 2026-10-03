//! Actual isolated manager UI readiness. This module creates no ordinary App,
//! runtime, stores or CLI resources and accepts no frontend path/URL authority.
use super::{
    blocked,
    files::{ComponentName, Directory, FileIdentity, PinnedFile, PrivateDirectory},
    handle,
    manager_process::launch_path,
    process::{ExactProcess, ProcessIdentity},
    security::CurrentUser,
    startup::TransactionDataRoot,
    webview::{actual_udf, reject_manager_overrides, verify_manager_udf},
};
use crate::version_history::manager_document::{ManagerDocumentBinding, MANAGER_LABEL};
use std::{ffi::OsStr, io, path::PathBuf, sync::Arc};
use tauri::{utils::config::WindowConfig, WebviewUrl, WebviewWindow};
use windows::{
    Wdk::Storage::FileSystem::FILE_OPEN_IF,
    Win32::Storage::FileSystem::{
        FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_READ, FILE_WRITE_DATA, READ_CONTROL,
        SYNCHRONIZE,
    },
};

fn name(text: &str) -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(text))
}
struct ManagerInstanceLease {
    file: PinnedFile,
    root: Arc<PrivateDirectory>,
}
impl ManagerInstanceLease {
    fn acquire(root: Arc<PrivateDirectory>, user: &CurrentUser) -> io::Result<Self> {
        root.verify(user)?;
        let name = name("manager-owner.lock")?;
        let descriptor = user.descriptor(false)?;
        // No sharing of writes/deletion: a second manager cannot create its
        // controller while this process owns the same permanent file object.
        let file = root.directory().open_relative(
            &name,
            FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | READ_CONTROL | SYNCHRONIZE,
            FILE_SHARE_READ,
            FILE_OPEN_IF,
            false,
            Some(&descriptor),
        )?;
        user.verify_private_file(handle(&file), false)?;
        Ok(Self {
            file: PinnedFile::from_file(root.directory().clone(), name, file)?,
            root,
        })
    }
    fn verify(&self, user: &CurrentUser) -> io::Result<()> {
        self.root.verify(user)?;
        self.file.verify()?;
        user.verify_private_file(handle(&self.file.file), false)
    }
}
pub(crate) struct ManagerUiEnvironment {
    transaction_id: String,
    data_root: Arc<PrivateDirectory>,
    udf: Arc<PrivateDirectory>,
    instance: Arc<ManagerInstanceLease>,
}
/// Minted only by the actual controller environment, on its owning UI thread.
struct ManagerUdfObservation {
    transaction_id: String,
    data_root: FileIdentity,
    actual: Arc<Directory>,
    expected: Arc<PrivateDirectory>,
    instance: Arc<ManagerInstanceLease>,
    thread: std::thread::ThreadId,
}
impl ManagerUiEnvironment {
    pub(crate) fn prepare(data: &TransactionDataRoot) -> io::Result<Self> {
        // Must precede controller creation; never clear or ignore overrides.
        reject_manager_overrides()?;
        data.verify()
            .map_err(|_| blocked("transaction data root changed"))?;
        let user = CurrentUser::capture()?;
        user.require_unelevated()?;
        let data_root = data.root().clone();
        let instance = Arc::new(ManagerInstanceLease::acquire(data_root.clone(), &user)?);
        let udf = match PrivateDirectory::open_existing(
            data_root.directory().clone(),
            name("manager-webview")?,
            &user,
        ) {
            Ok(udf) => udf,
            Err(error) if error.kind() == io::ErrorKind::NotFound => PrivateDirectory::create_new(
                data_root.directory().clone(),
                name("manager-webview")?,
                &user,
            )?,
            Err(error) => return Err(error),
        };
        let udf = Arc::new(udf);
        udf.verify(&user)?;
        instance.verify(&user)?;
        Ok(Self {
            transaction_id: data.transaction_id().into(),
            data_root,
            udf,
            instance,
        })
    }
    pub(crate) fn data_directory(&self) -> io::Result<PathBuf> {
        self.instance.verify(&CurrentUser::capture()?)?;
        Ok(PathBuf::from(launch_path(self.udf.directory().raw())?))
    }
    pub(crate) fn window_config(&self) -> WindowConfig {
        WindowConfig {
            label: MANAGER_LABEL.into(),
            title: "CC Desk Version Manager".into(),
            url: WebviewUrl::App("version-manager.html".into()),
            width: 760.0,
            height: 600.0,
            min_width: Some(640.0),
            min_height: Some(480.0),
            ..WindowConfig::default()
        }
    }
    /// The native callback derives the UDF and document witness from the same
    /// retained WebviewWindow. No caller can pair a separate environment with
    /// this document or nominate another thread as its UI thread.
    pub(crate) fn capture_ready(
        &self,
        window: WebviewWindow,
        document: Arc<ManagerDocumentBinding>,
        source: &ExactProcess,
        completed: impl FnOnce(io::Result<ManagerUiReady>) + Send + 'static,
    ) -> io::Result<()> {
        let transaction_id = self.transaction_id.clone();
        let data_root = self.data_root.directory().identity().clone();
        let expected = self.udf.clone();
        let instance = self.instance.clone();
        let source = source.identity().clone();
        let retained_window = window.clone();
        window
            .with_webview(move |platform| {
                let result = (|| {
                    let user = CurrentUser::capture()?;
                    instance.verify(&user)?;
                    let environment = platform.environment();
                    verify_manager_udf(&environment, &expected)?;
                    let actual = actual_udf(&environment)?;
                    if actual.identity() != expected.directory().identity() {
                        return Err(blocked("manager UDF differs"));
                    }
                    let source_process = ExactProcess::reopen(&source)?;
                    if source_process.terminal(0)?.is_some() {
                        return Err(blocked("manager readiness requires its live source"));
                    }
                    if document
                        .ready_for_window(&retained_window)
                        .map_err(|_| blocked("manager document is not ready"))?
                        != transaction_id
                    {
                        return Err(blocked("manager document belongs to another transaction"));
                    }
                    let manager = ExactProcess::capture_observed(std::process::id())?;
                    let identity = manager.identity().clone();
                    let ready = ManagerUiReady {
                        observation: ManagerUdfObservation {
                            transaction_id,
                            data_root,
                            actual,
                            expected,
                            instance,
                            thread: std::thread::current().id(),
                        },
                        document,
                        window: retained_window,
                        manager,
                        identity,
                        source,
                    };
                    ready.verify_current()?;
                    ready
                        .document
                        .pin_initial_handoff_window(&ready.window)
                        .map_err(|_| blocked("manager document cannot be pinned"))?;
                    Ok(ready)
                })();
                // Initial readiness must be published inside this callback,
                // on the same native UI thread that observed the controller.
                completed(result);
            })
            .map_err(|_| blocked("manager native controller unavailable"))
    }
}
pub(crate) struct ManagerUiReady {
    observation: ManagerUdfObservation,
    document: Arc<ManagerDocumentBinding>,
    window: WebviewWindow,
    manager: ExactProcess,
    identity: ProcessIdentity,
    source: ProcessIdentity,
}
impl ManagerUiReady {
    pub(crate) fn require_publication_thread(&self) -> io::Result<()> {
        if std::thread::current().id() != self.observation.thread {
            return Err(blocked(
                "manager readiness publication requires its UI thread",
            ));
        }
        self.verify_current()
    }
    pub(crate) fn release_after_source_exit(&self, source: &ExactProcess) -> io::Result<()> {
        if source.identity() != &self.source || source.terminal(0)?.is_none() {
            return Err(blocked("source handoff is not terminal"));
        }
        self.document.release_initial_handoff();
        Ok(())
    }
    fn verify_current(&self) -> io::Result<()> {
        let user = CurrentUser::capture()?;
        self.observation.instance.verify(&user)?;
        self.observation.expected.verify(&user)?;
        self.observation.actual.recheck()?;
        if self.observation.actual.identity() != self.observation.expected.directory().identity()
            || self.manager.identity() != &self.identity
            || self.manager.terminal(0)?.is_some()
            || self
                .document
                .ready_for_window(&self.window)
                .map_err(|_| blocked("manager document changed"))?
                != self.observation.transaction_id
        {
            return Err(blocked("manager UI is no longer ready"));
        }
        Ok(())
    }
    pub(crate) fn verify_for(
        &self,
        transaction: &str,
        manager: &ExactProcess,
        data_root: &PrivateDirectory,
    ) -> io::Result<()> {
        self.verify_current()?;
        if transaction != self.observation.transaction_id
            || manager.identity() != &self.identity
            || manager.terminal(0)?.is_some()
            || data_root.directory().identity() != &self.observation.data_root
        {
            return Err(blocked("manager UI proof belongs to another owner"));
        }
        data_root.verify(&CurrentUser::capture()?)
    }
}
