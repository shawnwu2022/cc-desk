//! UI-thread-only controller/environment lifetime and actual UDF evidence.
//! GetProcessInfos is deliberately NOT treated as completeness evidence: it
//! excludes crashpad. Only the matching BrowserProcessExited event establishes
//! runtime/UDF release, followed separately by exact source-host termination.
use super::{
    blocked,
    durability::DurableRecord,
    files::{ComponentName, Directory, FileIdentity, PrivateDirectory},
    process::{ExactProcess, ProcessIdentity, TerminalReceipt, TerminatedProcess},
    registry,
    security::CurrentUser,
    win_error,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    ffi::OsStr,
    io,
    marker::PhantomData,
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
use windows::Win32::System::{Com::CoTaskMemFree, Threading::GetCurrentProcessId};
use windows_core::{Interface, PWSTR};

/// No COM object or callback is sent to a background thread. Source integration
/// must retain this object on its event-loop thread until the receipt is durable.
pub(crate) struct SourceWebViews {
    thread: ThreadId,
    host: ExactProcess,
    udf: Arc<Directory>,
    environments: Vec<EnvironmentExit>,
    controllers: Vec<ICoreWebView2Controller>,
    closed: bool,
    persisted: bool,
    _ui_only: PhantomData<Rc<()>>,
}
struct EnvironmentExit {
    environment: ICoreWebView2Environment5,
    browser: ExactProcess,
    event: Rc<RefCell<EventState>>,
    token: i64,
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
        expected.recheck()?;
        let host = ExactProcess::capture_observed(unsafe { GetCurrentProcessId() })?;
        let mut environments = vec![];
        let mut controllers = vec![];
        let mut browsers = BTreeSet::new();
        for (environment, controller) in views {
            let actual = actual_udf(&environment)?;
            if actual.identity() != expected.identity() {
                return Err(blocked("source controller uses another UDF"));
            }
            let webview = unsafe { controller.CoreWebView2().map_err(win_error)? };
            let mut pid = 0;
            unsafe {
                webview.BrowserProcessId(&mut pid).map_err(win_error)?;
            }
            if browsers.insert(pid) {
                let browser = ExactProcess::capture_observed(pid)?;
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
                if after != pid || retained.browser.terminal(0)?.is_some() {
                    return Err(blocked("source browser changed during capture"));
                }
                environments.push(retained);
            }
            controllers.push(controller);
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
        self.closed = true; // Partial failure is not replayed.
        for controller in &self.controllers {
            unsafe {
                controller.Close().map_err(win_error)?;
            }
        }
        self.controllers.clear();
        Ok(())
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
            if !event.matched || environment.browser.terminal(0)?.is_none() {
                return Ok(None);
            }
            browsers.push(environment.browser.identity().clone());
        }
        self.udf.recheck()?;
        let binding = WebViewExitBinding {
            schema: 1,
            host: self.host.identity().clone(),
            udf: self.udf.identity().clone(),
            browsers,
        };
        let bytes = serde_json::to_vec(&binding).map_err(io::Error::other)?;
        let name = ComponentName::new(OsStr::new(&format!(
            "webview-exit-{}.json",
            uuid::Uuid::new_v4().simple()
        )))?;
        let record = DurableRecord::create(root, name, &bytes, user)?;
        self.persisted = true;
        Ok(Some(WebViewExitReceipt { record, binding }))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebViewExitBinding {
    schema: u32,
    host: ProcessIdentity,
    udf: FileIdentity,
    browsers: Vec<ProcessIdentity>,
}
pub(crate) struct WebViewExitReceipt {
    record: DurableRecord,
    binding: WebViewExitBinding,
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
    pub(crate) fn verify(&self) -> io::Result<()> {
        self._receipt.record.verify()?;
        self.host.verify()
    }
}
impl WebViewExitReceipt {
    pub(crate) fn open(record: DurableRecord) -> io::Result<Self> {
        record.verify()?;
        let binding: WebViewExitBinding =
            serde_json::from_slice(record.bytes()).map_err(io::Error::other)?;
        if binding.schema != 1 || binding.browsers.is_empty() || binding.browsers.len() > 32 {
            return Err(blocked("invalid persisted WebView exit receipt"));
        }
        binding.host.validate()?;
        for browser in &binding.browsers {
            browser.validate()?;
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
