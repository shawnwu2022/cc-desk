//! UI-thread-only controller/environment lifetime and actual UDF evidence.
//! GetProcessInfos is deliberately NOT treated as completeness evidence: it
//! excludes crashpad. Only the matching BrowserProcessExited event establishes
//! runtime/UDF release, followed separately by exact source-host termination.
use super::{
    blocked,
    durability::DurableRecord,
    files::{ComponentName, Directory, FileIdentity, PrivateDirectory},
    process::{
        creation_time, session_id, ExactProcess, ProcessIdentity, TerminalReceipt,
        TerminatedProcess,
    },
    registry,
    security::CurrentUser,
    win_error,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    io,
    marker::PhantomData,
    os::windows::io::OwnedHandle,
    path::Path,
    rc::Rc,
    sync::Arc,
    thread::{self, ThreadId},
};
use webview2_com::{
    BrowserProcessExitedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller, ICoreWebView2Environment, ICoreWebView2Environment5,
        ICoreWebView2Environment7,
    },
};
use windows::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        Com::CoTaskMemFree,
        Threading::{
            GetCurrentProcessId, GetProcessId, OpenProcess, WaitForSingleObject,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        },
    },
};
use windows_core::{Interface, PWSTR};

/// No COM object or callback is sent to a background thread. Source integration
/// must retain this object on its event-loop thread until the receipt is durable.
pub(crate) struct SourceWebViews {
    thread: ThreadId,
    host: ExactProcess,
    udf: Arc<Directory>,
    environments: Vec<EnvironmentExit>,
    controllers: Vec<ControllerBinding>,
    closed: bool,
    persisted: bool,
    _ui_only: PhantomData<Rc<()>>,
}
struct ControllerBinding {
    controller: ICoreWebView2Controller,
    browser_index: usize,
}
struct EnvironmentExit {
    environment: ICoreWebView2Environment5,
    browser: BrowserProcess,
    event: Rc<RefCell<EventState>>,
    token: i64,
}

/// A retained controller-observed browser process proves only exact lifetime.
/// It grants no image admission, launch, mutation, termination or PID reopen.
struct BrowserProcess {
    process: OwnedHandle,
    identity: BrowserIdentity,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserIdentity {
    pid: u32,
    created: u64,
    session: u32,
}
impl BrowserProcess {
    fn capture(pid: u32) -> io::Result<Self> {
        if pid == 0 {
            return Err(blocked("missing controller browser process"));
        }
        let process = unsafe {
            super::own(
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
                .map_err(win_error)?,
            )
        };
        let observed = unsafe { GetProcessId(super::handle(&process)) };
        let session = session_id(pid)?;
        if observed != pid || session != session_id(unsafe { GetCurrentProcessId() })? {
            return Err(blocked("controller browser identity or session differs"));
        }
        let identity = BrowserIdentity {
            pid,
            created: creation_time(super::handle(&process))?,
            session,
        };
        identity.validate()?;
        let result = Self { process, identity };
        if result.terminal()? {
            return Err(blocked("controller browser already exited"));
        }
        Ok(result)
    }
    fn terminal(&self) -> io::Result<bool> {
        match unsafe { WaitForSingleObject(super::handle(&self.process), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(blocked("controller browser wait is unresolved")),
        }
    }
}
impl BrowserIdentity {
    fn validate(&self) -> io::Result<()> {
        if self.pid == 0 || self.created == 0 {
            return Err(blocked("invalid browser lifetime identity"));
        }
        Ok(())
    }
}
#[derive(Default)]
struct EventState {
    matched: bool,
    invalid: bool,
}
impl Drop for EnvironmentExit {
    fn drop(&mut self) {
        unsafe {
            let _ = self.environment.remove_BrowserProcessExited(self.token);
        }
    }
}

pub(crate) fn reject_manager_overrides() -> io::Result<()> {
    // Additional arguments can redirect --user-data-dir; do not remove them.
    for name in [
        "WEBVIEW2_USER_DATA_FOLDER",
        "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
        "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
        "WEBVIEW2_PIPE_FOR_SCRIPT_DEBUGGER",
        "WEBVIEW2_WAIT_FOR_SCRIPT_DEBUGGER",
    ] {
        if std::env::var_os(name).is_some() {
            return Err(blocked("WebView environment override is configured"));
        }
    }
    registry::reject_webview_overrides()
}
pub(crate) fn actual_udf(environment: &ICoreWebView2Environment) -> io::Result<Arc<Directory>> {
    let environment: ICoreWebView2Environment7 = environment.cast().map_err(win_error)?;
    let mut path = PWSTR::null();
    unsafe {
        environment.UserDataFolder(&mut path).map_err(win_error)?;
    }
    struct ComText(PWSTR);
    impl Drop for ComText {
        fn drop(&mut self) {
            unsafe {
                CoTaskMemFree(Some(self.0 .0.cast()));
            }
        }
    }
    let path = ComText(path);
    if path.0.is_null() {
        return Err(blocked("WebView returned no UDF"));
    }
    // WebView2 owns the terminated COM string. Bound the scan; never use the
    // webview2-com helper that silently replaces malformed UTF-16.
    let mut length = 0;
    unsafe {
        while length < 32768 && *path.0 .0.add(length) != 0 {
            length += 1;
        }
    }
    if length == 32768 {
        return Err(blocked("WebView UDF exceeds path limit"));
    }
    let units = unsafe { std::slice::from_raw_parts(path.0 .0, length) };
    let text = String::from_utf16(units).map_err(|_| blocked("WebView UDF is not exact UTF-16"))?;
    Directory::open_absolute(Path::new(&text))
}
pub(crate) fn verify_manager_udf(
    environment: &ICoreWebView2Environment,
    expected: &PrivateDirectory,
) -> io::Result<()> {
    let actual = actual_udf(environment)?;
    if actual.identity() != expected.directory().identity() {
        return Err(blocked("manager WebView selected another UDF"));
    }
    Ok(())
}
impl SourceWebViews {
    /// Call inside Tauri with_webview on the UI thread for every frozen source
    /// controller. Prevent future creation first; completeness of that controller
    /// registry and automatic-exit prevention are coordinator responsibilities.
    pub(crate) fn capture(
        views: Vec<(ICoreWebView2Environment, ICoreWebView2Controller)>,
        expected: Arc<Directory>,
    ) -> io::Result<Self> {
        if views.is_empty() || views.len() > 32 {
            return Err(blocked("unsupported source controller set"));
        }
        expected
            .recheck()
            .map_err(|error| capture_diagnostic("expectedUdf", error))?;
        let host = ExactProcess::capture_observed(unsafe { GetCurrentProcessId() })
            .map_err(|error| capture_diagnostic("host", error))?;
        let mut environments = vec![];
        let mut controllers = vec![];
        let mut browsers = BTreeMap::new();
        for (environment, controller) in views {
            let actual =
                actual_udf(&environment).map_err(|error| capture_diagnostic("actualUdf", error))?;
            if actual.identity() != expected.identity() {
                return Err(blocked("source controller uses another UDF"));
            }
            let webview = unsafe { controller.CoreWebView2().map_err(win_error)? };
            let mut pid = 0;
            unsafe {
                webview.BrowserProcessId(&mut pid).map_err(win_error)?;
            }
            let browser_index = if let Some(index) = browsers.get(&pid) {
                *index
            } else {
                #[cfg(test)]
                match ExactProcess::capture_observed(pid) {
                    Ok(previous) => {
                        drop(previous);
                        eprintln!("source WebView strict capture: stage=browser, reason=accepted");
                    }
                    Err(error) => eprintln!("{}", capture_diagnostic("browser", error)),
                }
                let browser = BrowserProcess::capture(pid)
                    .map_err(|error| capture_diagnostic("browser", error))?;
                let environment: ICoreWebView2Environment5 =
                    environment.cast().map_err(win_error)?;
                let event = Rc::new(RefCell::new(EventState::default()));
                let state = event.clone();
                let callback =
                    BrowserProcessExitedEventHandler::create(Box::new(move |_, args| {
                        let mut seen = 0;
                        let result = args
                            .as_ref()
                            .map(|args| unsafe { args.BrowserProcessId(&mut seen) });
                        let mut state = state.borrow_mut();
                        match result {
                            Some(Ok(())) if seen == pid => state.matched = true,
                            _ => state.invalid = true,
                        }
                        Ok(())
                    }));
                let mut token = 0;
                unsafe {
                    environment
                        .add_BrowserProcessExited(&callback, &mut token)
                        .map_err(win_error)?;
                }
                let retained = EnvironmentExit {
                    environment,
                    browser,
                    event,
                    token,
                };
                // The process observed before registration must still be the live
                // controller browser. Missing/raced events remain a blocker.
                let mut after = 0;
                unsafe {
                    webview.BrowserProcessId(&mut after).map_err(win_error)?;
                }
                if after != pid || retained.browser.terminal()? {
                    return Err(blocked("source browser changed during capture"));
                }
                let index = environments.len();
                environments.push(retained);
                browsers.insert(pid, index);
                index
            };
            controllers.push(ControllerBinding {
                controller,
                browser_index,
            });
        }
        Ok(Self {
            thread: thread::current().id(),
            host,
            udf: expected,
            environments,
            controllers,
            closed: false,
            persisted: false,
            _ui_only: PhantomData,
        })
    }
    fn on_ui_thread(&self) -> io::Result<()> {
        if self.thread != thread::current().id() {
            return Err(blocked("WebView evidence requires its UI thread"));
        }
        Ok(())
    }
    pub(crate) fn close_controllers(&mut self) -> io::Result<()> {
        self.on_ui_thread()?;
        if self.closed {
            return Err(blocked("source controllers were already closed"));
        }
        self.closed = true; // Stale generation and partial failure are not replayed.
        for controller in &self.controllers {
            self.verify_controller(controller)?;
        }
        for controller in &self.controllers {
            self.verify_controller(controller)?;
            unsafe {
                controller.controller.Close().map_err(win_error)?;
            }
        }
        self.controllers.clear();
        Ok(())
    }
    fn verify_controller(&self, controller: &ControllerBinding) -> io::Result<()> {
        let environment = self
            .environments
            .get(controller.browser_index)
            .ok_or_else(|| blocked("source controller lacks its browser binding"))?;
        let mut current = 0;
        unsafe {
            controller
                .controller
                .CoreWebView2()
                .map_err(win_error)?
                .BrowserProcessId(&mut current)
                .map_err(win_error)?;
        }
        let event = environment.event.borrow();
        check_browser_generation(
            environment.browser.identity.pid,
            current,
            environment.browser.terminal()?,
            event.matched,
            event.invalid,
        )
    }
    pub(crate) fn persist_when_exited(
        &mut self,
        root: Arc<PrivateDirectory>,
        user: &CurrentUser,
    ) -> io::Result<Option<WebViewExitReceipt>> {
        self.on_ui_thread()?;
        if !self.closed || !self.controllers.is_empty() || self.persisted {
            return Err(blocked("source WebView handoff is not ready"));
        }
        let mut browsers = vec![];
        for environment in &self.environments {
            let event = environment.event.borrow();
            if event.invalid {
                return Err(blocked("source WebView exit event is ambiguous"));
            }
            if !event.matched || !environment.browser.terminal()? {
                return Ok(None);
            }
            browsers.push(environment.browser.identity.clone());
        }
        self.udf.recheck()?;
        let binding = WebViewExitBinding {
            schema: 2,
            host: self.host.identity().clone(),
            udf: self.udf.identity().clone(),
            browsers,
        };
        let bytes = serde_json::to_vec(&binding).map_err(io::Error::other)?;
        let name = ComponentName::new(OsStr::new(&format!(
            "webview-exit-{}.json",
            uuid::Uuid::new_v4().simple()
        )))?;
        self.persisted = true; // An uncertain write is never automatically replayed.
        let record = DurableRecord::create(root, name, &bytes, user)?;
        Ok(Some(WebViewExitReceipt { record, binding }))
    }
}
/// Inputs are sampled from the retained controller, kernel handle and matching
/// callback immediately before close, never accepted from a frontend caller.
pub(crate) fn check_browser_generation(
    expected: u32,
    current: u32,
    terminal: bool,
    matched: bool,
    invalid: bool,
) -> io::Result<()> {
    if expected == 0 || current != expected || terminal || matched || invalid {
        return Err(blocked("source controller browser generation changed"));
    }
    Ok(())
}
fn capture_diagnostic(stage: &'static str, error: io::Error) -> io::Error {
    // Only fixed call-site stages and predicate categories cross this boundary;
    // never include queried image paths, SIDs or other ambient error text.
    io::Error::new(
        error.kind(),
        format!(
            "source WebView capture failed: stage={stage}, reason={}",
            super::files::metadata_rejection(&error)
        ),
    )
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebViewExitBinding {
    schema: u32,
    host: ProcessIdentity,
    udf: FileIdentity,
    browsers: Vec<BrowserIdentity>,
}
pub(crate) struct WebViewExitReceipt {
    record: DurableRecord,
    binding: WebViewExitBinding,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WebViewExitReference {
    name: String,
    identity: FileIdentity,
    digest: String,
}
pub(crate) struct SourceExitEvidence {
    _receipt: WebViewExitReceipt,
    _host: ExactProcess,
    _terminal: TerminalReceipt,
}
pub(crate) struct SourceExitFenceEvidence {
    _receipt: WebViewExitReceipt,
    host: TerminatedProcess,
}
impl SourceExitEvidence {
    pub(crate) fn release_image_for_fence(self) -> io::Result<SourceExitFenceEvidence> {
        self._receipt.record.verify()?;
        let host = self._host.release_terminated_image()?;
        host.verify()?;
        Ok(SourceExitFenceEvidence {
            _receipt: self._receipt,
            host,
        })
    }
}
impl SourceExitFenceEvidence {
    pub(crate) fn host_identity(&self) -> &ProcessIdentity {
        &self._receipt.binding.host
    }
    pub(crate) fn udf_identity(&self) -> &FileIdentity {
        &self._receipt.binding.udf
    }
    pub(crate) fn verify(&self) -> io::Result<()> {
        self._receipt.record.verify()?;
        self.host.verify()
    }
}
impl WebViewExitReceipt {
    pub(crate) fn reference(&self) -> io::Result<WebViewExitReference> {
        self.record.verify()?;
        Ok(WebViewExitReference {
            name: self
                .record
                .name()
                .os_string()
                .into_string()
                .map_err(|_| blocked("invalid WebView receipt name"))?,
            identity: self.record.file_identity().clone(),
            digest: self.record.digest().into(),
        })
    }
    pub(crate) fn verify_live_source(
        &self,
        source: &ExactProcess,
        udf: &Directory,
    ) -> io::Result<()> {
        self.record.verify()?;
        udf.recheck()?;
        if source.identity() != &self.binding.host
            || source.terminal(0)?.is_some()
            || udf.identity() != &self.binding.udf
        {
            return Err(blocked(
                "WebView exit receipt belongs to another live source",
            ));
        }
        Ok(())
    }
    pub(crate) fn reopen(
        root: Arc<PrivateDirectory>,
        reference: &WebViewExitReference,
        user: &CurrentUser,
    ) -> io::Result<Self> {
        let record = DurableRecord::open(
            root,
            ComponentName::new(OsStr::new(&reference.name))?,
            &reference.digest,
            user,
        )?;
        if record.file_identity() != &reference.identity {
            return Err(blocked("WebView exit record identity changed"));
        }
        Self::open(record)
    }
    pub(crate) fn open(record: DurableRecord) -> io::Result<Self> {
        record.verify()?;
        let binding: WebViewExitBinding =
            serde_json::from_slice(record.bytes()).map_err(io::Error::other)?;
        if binding.schema != 2 || binding.browsers.is_empty() || binding.browsers.len() > 32 {
            return Err(blocked("invalid persisted WebView exit receipt"));
        }
        binding.host.validate()?;
        let mut observed = BTreeSet::new();
        for browser in &binding.browsers {
            browser.validate()?;
            if browser.session != binding.host.session() || !observed.insert(browser.pid) {
                return Err(blocked("browser receipt session or uniqueness differs"));
            }
        }
        Ok(Self { record, binding })
    }
    /// Run in the manager with the host handle captured before handoff, not a
    /// missing-PID inference. No receipt/event means this path stays unavailable.
    pub(crate) fn after_host_exit(self, host: ExactProcess) -> io::Result<SourceExitEvidence> {
        self.record.verify()?;
        if host.identity() != &self.binding.host {
            return Err(blocked("WebView receipt belongs to another source host"));
        }
        let terminal = host
            .terminal(0)?
            .ok_or_else(|| blocked("source host is still running"))?;
        Ok(SourceExitEvidence {
            _receipt: self,
            _host: host,
            _terminal: terminal,
        })
    }
}
