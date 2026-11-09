//! 阶段11测试宿主；正式应用启动、用户目录及默认工作区不进入此路径。
use super::*;
use crate::cli::{
    document::DOCUMENT_HEADER,
    native_runtime::{take_main_config, NativeRuntime},
};
use crate::version_history::windows::{
    files::Directory,
    webview::{actual_udf, reject_manager_overrides},
};
use parking_lot::Mutex;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tauri::ipc::{InvokeBody, Request};
use tauri::utils::config::{Csp, FrontendDist, WindowConfig};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, State, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const CHECKPOINTS: &[(&str, usize, usize)] = &[
    ("real-scope-history", 0, 0),
    ("review-only", 0, 0),
    ("cancelled-no-confirm", 0, 0),
    ("independent-second-confirmation", 0, 0),
    ("backend-missing-ack", 2, 0),
    ("wrong-document-source-peer", 5, 0),
    ("changed-source-ticket", 6, 0),
    ("native-delete-result", 7, 0),
    ("duplicate-confirm-receipt", 9, 0),
    ("inspect-without-replay", 9, 1),
    ("expiry-wait-started", 9, 1),
    ("expired-ticket", 10, 1),
];
fn checkpoint_rule(index: usize, name: &str, confirm: usize, inspect: usize, native: bool) -> bool {
    (native || index < 7)
        && CHECKPOINTS
            .get(index)
            .is_some_and(|expected| *expected == (name, confirm, inspect))
}

#[test]
fn NativeDeleteWebView_CheckpointRejectsReorderedOrExtraDispatch_003() {
    assert!(checkpoint_rule(0, "real-scope-history", 0, 0, false));
    assert!(!checkpoint_rule(0, "review-only", 0, 0, false));
    assert!(!checkpoint_rule(4, "backend-missing-ack", 3, 0, false));
    assert!(!checkpoint_rule(7, "native-delete-result", 7, 0, false));
    assert!(checkpoint_rule(8, "duplicate-confirm-receipt", 9, 0, true));
    assert!(!checkpoint_rule(9, "inspect-without-replay", 9, 0, true));
}

struct AssetBundle {
    files: BTreeMap<String, Vec<u8>>,
    csp: String,
    manifest: serde_json::Value,
}
impl AssetBundle {
    fn load(root: &Path) -> io::Result<Self> {
        use crate::cli::native_projection::scoped_fs::{Budget, Limits};
        use std::os::windows::fs::MetadataExt;
        let metadata = fs::symlink_metadata(root)?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("ASSET_DIRECTORY_UNSAFE"));
        }
        let held = Root::open(root).map_err(io::Error::other)?;
        let mut budget = Budget::new(Limits {
            file_bytes: 2 * 1024 * 1024,
            total_bytes: 8 * 1024 * 1024,
            entries: 128,
        });
        let raw = held
            .read(Path::new("manifest.json"), &mut budget)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("ASSET_MANIFEST_MISSING"))?;
        if raw.len() > 256 * 1024 {
            return Err(io::Error::other("ASSET_MANIFEST_LIMIT"));
        }
        let manifest: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|_| io::Error::other("ASSET_MANIFEST_INVALID"))?;
        if manifest["schema"] != 1 || manifest["appVersion"] != env!("CARGO_PKG_VERSION") {
            return Err(io::Error::other("ASSET_BUILD_MISMATCH"));
        }
        let head = manifest["buildHead"]
            .as_str()
            .ok_or_else(|| io::Error::other("ASSET_BUILD_MISMATCH"))?;
        if head.len() != 40 || !head.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(io::Error::other("ASSET_BUILD_MISMATCH"));
        }
        let csp = manifest["csp"]
            .as_str()
            .filter(|s| {
                !s.is_empty() && s.len() <= 4096 && s.is_ascii() && !s.contains(['\r', '\n', '\0'])
            })
            .ok_or_else(|| io::Error::other("ASSET_CSP_INVALID"))?
            .to_owned();
        let records = manifest["files"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 64)
            .ok_or_else(|| io::Error::other("ASSET_FILE_LIMIT"))?;
        let mut files = BTreeMap::new();
        for item in records {
            let path = item["path"]
                .as_str()
                .filter(|p| safe_asset_path(p))
                .ok_or_else(|| io::Error::other("ASSET_PATH_INVALID"))?;
            let length = item["bytes"]
                .as_u64()
                .filter(|n| *n <= 2 * 1024 * 1024)
                .ok_or_else(|| io::Error::other("ASSET_FILE_LIMIT"))?;
            let bytes = held
                .read(Path::new(path), &mut budget)
                .map_err(io::Error::other)?
                .ok_or_else(|| io::Error::other("ASSET_FILE_MISSING"))?;
            if bytes.len() as u64 != length
                || item["sha256"] != format!("{:x}", Sha256::digest(&bytes))
            {
                return Err(io::Error::other("ASSET_DIGEST_MISMATCH"));
            }
            if files.insert(path.to_owned(), bytes).is_some() {
                return Err(io::Error::other("ASSET_DUPLICATE"));
            }
        }
        if !files.contains_key("index.html") {
            return Err(io::Error::other("ASSET_INDEX_MISSING"));
        }
        held.source_current().map_err(io::Error::other)?;
        Ok(Self {
            files,
            csp,
            manifest,
        })
    }
    fn verify_production_sources(&self) -> io::Result<()> {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let modules = self.manifest["modules"]
            .as_array()
            .filter(|m| !m.is_empty() && m.len() <= 128)
            .ok_or_else(|| io::Error::other("PRODUCTION_MODULES_MISSING"))?;
        let mut panel = false;
        let held = Root::open(repository).map_err(io::Error::other)?;
        let mut budget = crate::cli::native_projection::scoped_fs::Budget::new(Default::default());
        for item in modules {
            let path = item["path"]
                .as_str()
                .filter(|p| p.starts_with("src/") && safe_asset_path(p))
                .ok_or_else(|| io::Error::other("PRODUCTION_MODULE_PATH_INVALID"))?;
            let bytes = held
                .read(Path::new(path), &mut budget)
                .map_err(io::Error::other)?
                .ok_or_else(|| io::Error::other("PRODUCTION_MODULE_MISSING"))?;
            if item["sha256"] != format!("{:x}", Sha256::digest(bytes)) {
                return Err(io::Error::other("PRODUCTION_MODULE_CHANGED"));
            }
            panel |= path == "src/components/sessions/NativeSessionDeletePanel.vue";
        }
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(repository.join("src-tauri/tauri.conf.json"))?)
                .map_err(io::Error::other)?;
        if !panel || config["app"]["security"]["csp"] != self.csp {
            return Err(io::Error::other("PRODUCTION_ASSET_CONTRACT_MISMATCH"));
        }
        let drivers = self.manifest["drivers"]
            .as_array()
            .filter(|v| v.len() == 3)
            .ok_or_else(|| io::Error::other("DRIVER_SOURCES_MISSING"))?;
        let mut expected = std::collections::BTreeSet::from([
            "tests/native-delete-webview/main.ts",
            "tests/native-delete-webview/index.html",
            "scripts/build-native-delete-webview.mjs",
        ]);
        for item in drivers {
            let path = item["path"]
                .as_str()
                .filter(|p| expected.remove(p))
                .ok_or_else(|| io::Error::other("DRIVER_SOURCE_INVALID"))?;
            let bytes = held
                .read(Path::new(path), &mut budget)
                .map_err(io::Error::other)?
                .ok_or_else(|| io::Error::other("DRIVER_SOURCE_MISSING"))?;
            if item["sha256"] != format!("{:x}", Sha256::digest(bytes)) {
                return Err(io::Error::other("DRIVER_SOURCE_CHANGED"));
            }
        }
        Ok(())
    }
}

fn safe_asset_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-/ .".contains(&b) && b != b' ')
        && path
            .split('/')
            .all(|part| !part.is_empty() && !part.starts_with('.') && part != "..")
}

struct AssetServer {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl AssetServer {
    fn start(bundle: Arc<AssetBundle>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = std::thread::spawn(move || {
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if let Err(error) = Self::serve(&mut stream, &bundle, &address.to_string())
                        {
                            eprintln!(
                                "STAGE11_ASSET_IO {:?} {:?}",
                                error.kind(),
                                error.raw_os_error()
                            );
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            url: format!("http://{address}/index.html"),
            stop,
            worker: Some(worker),
        })
    }
    fn route<'a>(bundle: &'a AssetBundle, target: &str) -> Option<(&'a [u8], &'static str)> {
        let path = if target == "/" {
            "index.html"
        } else {
            target.strip_prefix('/')?
        };
        if !safe_asset_path(path) {
            return None;
        }
        let bytes = bundle.files.get(path)?;
        let mime = if path.ends_with(".html") {
            "text/html; charset=utf-8"
        } else if path.ends_with(".js") {
            "text/javascript; charset=utf-8"
        } else if path.ends_with(".css") {
            "text/css; charset=utf-8"
        } else {
            "application/octet-stream"
        };
        Some((bytes, mime))
    }
    fn serve(stream: &mut TcpStream, bundle: &AssetBundle, host: &str) -> io::Result<()> {
        // Windows accept 继承监听套接字的非阻塞模式；已接收连接须恢复有界阻塞读取。
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") && header.len() <= 8192 {
            let mut byte = [0];
            stream.read_exact(&mut byte)?;
            header.push(byte[0]);
        }
        let text = std::str::from_utf8(&header).unwrap_or("");
        let mut parts = text.lines().next().unwrap_or("").split_whitespace();
        let method = parts.next().unwrap_or("");
        let target = parts.next().unwrap_or("");
        let host_ok = text.lines().any(|line| {
            line.split_once(':').is_some_and(|(key, value)| {
                key.eq_ignore_ascii_case("host") && value.trim() == host
            })
        });
        let (status, body, mime) = if header.len() > 8192 {
            (431, &b""[..], "text/plain")
        } else if !host_ok || parts.next() != Some("HTTP/1.1") {
            (400, &b""[..], "text/plain")
        } else if !matches!(method, "GET" | "HEAD") {
            (405, &b""[..], "text/plain")
        } else if let Some((body, mime)) = Self::route(bundle, target) {
            (200, body, mime)
        } else {
            (404, &b""[..], "text/plain")
        };
        write!(stream,"HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nContent-Type: {mime}\r\nContent-Security-Policy: {}\r\nX-Content-Type-Options: nosniff\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",body.len(),bundle.csp)?;
        if method != "HEAD" {
            stream.write_all(body)?;
        }
        stream.flush()
    }
}
impl Drop for AssetServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn NativeDeleteWebView_ServerServesOnlyPinnedMemory_002() {
    fn request(server: &AssetServer, path: &str) -> Vec<u8> {
        let url = tauri::Url::parse(&server.url).unwrap();
        let host = format!("127.0.0.1:{}", url.port().unwrap());
        let mut stream = TcpStream::connect(&host).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut bytes = vec![];
        stream.take(65536).read_to_end(&mut bytes).unwrap();
        bytes
    }
    let bundle = Arc::new(AssetBundle {
        files: BTreeMap::from([("index.html".into(), b"pinned-body".to_vec())]),
        csp: "default-src 'self'".into(),
        manifest: serde_json::Value::Null,
    });
    let server = AssetServer::start(bundle).unwrap();
    let page = request(&server, "/index.html");
    assert!(page.starts_with(b"HTTP/1.1 200 "));
    assert!(page.ends_with(b"pinned-body"));
    assert!(String::from_utf8(page)
        .unwrap()
        .contains("Content-Security-Policy: default-src 'self'"));
    for path in [
        "/../outside",
        "/%2e%2e/outside",
        "//index.html",
        "/index.html?outside",
        "/missing",
    ] {
        let response = request(&server, path);
        assert!(
            response.starts_with(b"HTTP/1.1 404 "),
            "path={path}, status={:?}",
            String::from_utf8_lossy(response.split(|b| *b == b'\r').next().unwrap_or_default())
        );
    }
}

#[test]
fn NativeDeleteWebView_SplitHeaderRemainsBounded_006() {
    let bundle = AssetBundle {
        files: BTreeMap::from([("index.html".into(), b"pinned-body".to_vec())]),
        csp: "default-src 'self'".into(),
        manifest: serde_json::Value::Null,
    };
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = loop {
            match listener.accept() {
                Ok(value) => break value,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => std::thread::yield_now(),
                Err(error) => panic!("accept: {error}"),
            }
        };
        accepted_tx.send(()).unwrap();
        AssetServer::serve(&mut socket, &bundle, &address.to_string())
    });
    let mut client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    client.write_all(b"GET /index.html HTTP/1.1\r\n").unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let _ = write!(client, "Host: {address}\r\n\r\n");
    let mut response = Vec::new();
    let read = client.read_to_end(&mut response);
    let served = server.join().unwrap();
    assert!(
        served.is_ok(),
        "split header serve={served:?}, read={read:?}"
    );
    assert!(response.starts_with(b"HTTP/1.1 200 "));
}

#[test]
fn NativeDeleteWebView_AssetsAreBoundedAndPinned_001() {
    let fixture = Fixture::new().unwrap();
    let root = fixture.root.join("ui-assets");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("index.html"), b"fixture-page").unwrap();
    let mut manifest = serde_json::json!({"schema":1,"appVersion":env!("CARGO_PKG_VERSION"),"buildHead":"0000000000000000000000000000000000000000","csp":"default-src 'self'","modules":[],"files":[{"path":"index.html","bytes":12,"sha256":format!("{:x}",Sha256::digest(b"fixture-page"))}]});
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let bundle = AssetBundle::load(&root).unwrap();
    assert_eq!(bundle.files["index.html"], b"fixture-page");
    fs::write(root.join("index.html"), b"tampered").unwrap();
    assert!(AssetBundle::load(&root).is_err());
    manifest["files"][0]["path"] = serde_json::json!("../outside");
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(AssetBundle::load(&root).is_err());
}

fn webview_configuration_absent() -> io::Result<()> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, KEY_WOW64_32KEY};
    let name: Vec<_> = r"Software\Policies\Microsoft\Edge\WebView2"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let mut key = HKEY::default();
            let status = unsafe {
                RegOpenKeyExW(
                    hive,
                    PCWSTR(name.as_ptr()),
                    Some(REG_OPTION_OPEN_LINK.0),
                    KEY_READ | view,
                    &mut key,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                continue;
            }
            if status.is_ok() {
                let _ = unsafe { RegCloseKey(key) };
            }
            return Err(io::Error::other("WEBVIEW_POLICY_NOT_PROVEN_ABSENT"));
        }
    }
    reject_manager_overrides()
}

fn parent_child_leases(parent: &Fixture, path: &Path) -> io::Result<Vec<Root>> {
    let temporary = parent.root.join("temporary");
    let relative = path
        .strip_prefix(&temporary)
        .map_err(|_| io::Error::other("CHILD_ROOT_OUTSIDE_TASK"))?;
    let mut components = relative.components();
    let name = match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(name)), None) => name.to_str().filter(|s| {
            s.starts_with("v027-codex-delete-live-")
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        }),
        _ => None,
    }
    .ok_or_else(|| io::Error::other("CHILD_ROOT_INVALID"))?;
    if name.len() > 80 {
        return Err(io::Error::other("CHILD_ROOT_INVALID"));
    }
    let mut leases = vec![];
    for relative in [
        "",
        "codex-home",
        "codex-home/sessions",
        "codex-home/archived_sessions",
        "webview-main",
        "webview-peer",
    ] {
        use std::os::windows::fs::MetadataExt;
        let path = path.join(relative);
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("CHILD_DIRECTORY_UNSAFE"));
        }
        leases.push(Root::open(&path).map_err(io::Error::other)?);
    }
    Ok(leases)
}

fn mark_creation(root: &Path, label: &str) -> io::Result<()> {
    if !matches!(label, "main" | "peer") {
        return Err(io::Error::other("CREATION_LABEL_INVALID"));
    }
    // 文件先于任何 Wry 创建持久可见，stdout 只作诊断，不能靠日志到达顺序释放租约。
    fs::write(
        root.join(format!("reports/creation-{label}-pending")),
        b"pending\n",
    )?;
    println!("STAGE11_CREATION_PENDING {label}");
    io::stdout().flush()
}
fn pending_creations(
    root: &Path,
    verified: &std::collections::BTreeSet<String>,
) -> io::Result<std::collections::BTreeSet<String>> {
    let mut pending = std::collections::BTreeSet::new();
    for label in ["main", "peer"] {
        match fs::read(root.join(format!("reports/creation-{label}-pending"))) {
            Ok(bytes) if bytes == b"pending\n" => {
                if !verified.contains(label) {
                    pending.insert(label.to_owned());
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(io::Error::other("CREATION_MARKER_UNKNOWN")),
        }
    }
    Ok(pending)
}

#[test]
fn NativeDeleteWebView_PrePidFailureRetainsCreationBarrier_005() {
    let fixture = Fixture::new().unwrap();
    let mut verified = std::collections::BTreeSet::new();
    mark_creation(&fixture.root, "main").unwrap();
    assert_eq!(
        pending_creations(&fixture.root, &verified).unwrap(),
        std::collections::BTreeSet::from(["main".to_owned()])
    );
    // 任意失败本身不清除 marker；只有父层实际验证过的 label 可以排除。
    assert!(!pending_creations(&fixture.root, &verified)
        .unwrap()
        .is_empty());
    verified.insert("main".into());
    assert!(pending_creations(&fixture.root, &verified)
        .unwrap()
        .is_empty());
    mark_creation(&fixture.root, "peer").unwrap();
    assert!(pending_creations(&fixture.root, &verified)
        .unwrap()
        .contains("peer"));
    fs::write(
        fixture.root.join("reports/creation-peer-pending"),
        b"partial",
    )
    .unwrap();
    assert!(pending_creations(&fixture.root, &verified).is_err());
}

#[test]
fn NativeDeleteWebView_ParentPinsOnlyFreshContainedChild_004() {
    let parent = Fixture::new().unwrap();
    let child = parent.root.join("temporary/v027-codex-delete-live-child");
    for name in [
        "codex-home/sessions",
        "codex-home/archived_sessions",
        "webview-main",
        "webview-peer",
    ] {
        fs::create_dir_all(child.join(name)).unwrap();
    }
    let leases = parent_child_leases(&parent, &child).unwrap();
    assert_eq!(leases.len(), 6);
    assert!(parent_child_leases(&parent, &parent.root).is_err());
    assert!(parent_child_leases(&parent, &child.join("../v027-codex-delete-live-child")).is_err());
    assert!(fs::rename(&child, parent.root.join("moved-child")).is_err());
}

struct TaskBackendFixture {
    backend: Mutex<BackendFixture>,
    runtime: Arc<NativeRuntime>,
    native: bool,
    expiry_id: String,
    controls: Vec<(PathBuf, Vec<u8>)>,
    expected_main: Arc<Directory>,
    expected_peer: Arc<Directory>,
    actual_directories: Mutex<Vec<Arc<Directory>>>,
    main_loaded: AtomicBool,
    peer_loaded: AtomicBool,
    started: AtomicBool,
    peer_created: AtomicBool,
    peer_script: Mutex<Option<String>>,
    peer_reply: Mutex<Option<mpsc::Sender<bool>>>,
    confirmations: AtomicUsize,
    inspections: AtomicUsize,
    checkpoints: Mutex<Vec<String>>,
    captures: Mutex<Vec<String>>,
    failure: Mutex<Option<&'static str>>,
    finished: AtomicBool,
    success: AtomicBool,
    url: String,
}
impl TaskBackendFixture {
    fn context(&self) -> serde_json::Value {
        let b = self.backend.lock();
        serde_json::json!({"cli":"codex","profileId":"native-delete-fixture","profileRevision":b.repository.get_profile("native-delete-fixture").unwrap().revision,
            "projectId":b.project_id,"projectPath":b.fixture.project})
    }
    fn admit(&self, webview: &Webview, request: &Request<'_>) -> Result<CallerIdentity, String> {
        self.runtime
            .binding()
            .and_then(|binding| binding.admit_native(webview, request.headers()))
            .map_err(|e| e.code)
    }
    fn root(&self) -> PathBuf {
        self.backend.lock().fixture.root.clone()
    }
    fn fail(&self, app: &AppHandle, code: &'static str) {
        *self.failure.lock() = Some(code);
        self.finished.store(true, Ordering::Release);
        app.exit(1);
    }
    fn verify_files(&self, deleted: bool) -> Result<(), String> {
        let b = self.backend.lock();
        if deleted {
            if !matches!(fs::symlink_metadata(&b.rollout),Err(ref e)if e.kind()==io::ErrorKind::NotFound)
            {
                return Err("ORIGINAL_NOT_DELETED".into());
            }
        } else if fs::read(&b.rollout).ok().as_ref() != Some(&b.original) {
            return Err("ORIGINAL_CHANGED".into());
        }
        for (path, bytes) in &self.controls {
            if fs::read(path).ok().as_ref() != Some(bytes) {
                return Err("CONTROL_CHANGED".into());
            }
        }
        if b.repository.admission().check_metadata_idle().is_err() {
            return Err("OWNER_NOT_SETTLED".into());
        }
        if !deleted
            && (b.fixture.root.join("codex-home/installation_id").exists()
                || fs::read_dir(b.fixture.root.join("sqlite"))
                    .map_err(|_| "FIXTURE_SQLITE_UNAVAILABLE")?
                    .next()
                    .is_some())
        {
            return Err("UNEXPECTED_CLI_INITIALIZATION".into());
        }
        Ok(())
    }
}

fn query<T: serde::de::DeserializeOwned>(request: &Request<'_>) -> Result<T, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("RAW_REQUIRED".into());
    };
    if bytes.len() > 8192 {
        return Err("HELPER_BODY_LIMIT".into());
    }
    serde_json::from_slice(bytes).map_err(|_| "HELPER_INVALID_REQUEST".into())
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Named {
    name: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Finished {
    success: bool,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PeerRequest {
    confirmation: ConfirmRequest,
    proof: String,
}

#[tauri::command]
async fn s11_fixture(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<serde_json::Value, String> {
    state.admit(&webview, &request)?;
    let _: Empty = query(&request)?;
    let context = state.context();
    let b = state.backend.lock();
    Ok(
        serde_json::json!({"context":context,"targetId":b.id,"controlId":b.active_id,"expiryId":state.expiry_id,"native":state.native}),
    )
}
#[tauri::command]
async fn s11_checkpoint(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    state.admit(&webview, &request)?;
    let q: Named = query(&request)?;
    let mut records = state.checkpoints.lock();
    if !checkpoint_rule(
        records.len(),
        &q.name,
        state.confirmations.load(Ordering::Acquire),
        state.inspections.load(Ordering::Acquire),
        state.native,
    ) {
        return Err("CHECKPOINT_ORDER_OR_DISPATCH_FAILED".into());
    }
    state.verify_files(records.len() >= 7)?;
    if state.native {
        system_configuration_absent().map_err(|_| "CLI_ISOLATION_CHANGED")?;
    }
    println!("STAGE11_CHECKPOINT {}", q.name);
    records.push(q.name);
    Ok(())
}
#[tauri::command]
async fn s11_proof(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<String, String> {
    state.admit(&webview, &request)?;
    let _: Empty = query(&request)?;
    request
        .headers()
        .get(DOCUMENT_HEADER)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned)
        .ok_or_else(|| "FORBIDDEN".into())
}
#[tauri::command]
async fn s11_change_source(
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<serde_json::Value, String> {
    state.admit(&webview, &request)?;
    let _: Empty = query(&request)?;
    {
        let b = state.backend.lock();
        b.repository
            .apply(
                b.repository.read().map_err(|e| e.code)?.revision,
                Patch::Update {
                    id: "native-delete-fixture".into(),
                    changes: serde_json::json!({"name":"stage11-changed-source"})
                        .as_object()
                        .unwrap()
                        .clone(),
                },
            )
            .map_err(|e| e.code)?;
    }
    Ok(state.context())
}
#[tauri::command]
async fn s11_finish(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    state.admit(&webview, &request)?;
    let q: Finished = query(&request)?;
    let complete =
        state.checkpoints.lock().len() == if state.native { CHECKPOINTS.len() } else { 7 };
    let success = q.success && complete;
    state.success.store(success, Ordering::Release);
    state.finished.store(true, Ordering::Release);
    if !success {
        *state.failure.lock() = Some("FRONTEND_ACCEPTANCE_FAILED");
    }
    app.exit(if success { 0 } else { 1 });
    Ok(())
}
#[tauri::command]
async fn s11_peer(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    state.admit(&webview, &request)?;
    let q: PeerRequest = query(&request)?;
    if q.proof
        != request
            .headers()
            .get(DOCUMENT_HEADER)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
        || state.peer_created.swap(true, Ordering::AcqRel)
    {
        return Err("PEER_REQUEST_INVALID".into());
    }
    webview_configuration_absent().map_err(|_| "WEBVIEW_ISOLATION_CHANGED")?;
    let (sender, receiver) = mpsc::channel();
    *state.peer_reply.lock() = Some(sender);
    let confirmation = serde_json::json!({
        "operationId":q.confirmation.operation_id,"previewId":q.confirmation.preview_id,
        "acknowledgePermanentDelete":q.confirmation.acknowledge_permanent_delete,
        "acknowledgeNativeSubtree":q.confirmation.acknowledge_native_subtree
    })
    .to_string();
    let proof = serde_json::to_string(&q.proof).map_err(|_| "PEER_REQUEST_INVALID")?;
    *state.peer_script.lock()=Some(format!("(async()=>{{const n=window.__TAURI_INTERNALS__;const b=new TextEncoder().encode(JSON.stringify({confirmation}));const c=await n.invoke('cli_confirm_session_delete',b,{{headers:{{'x-cc-desk-document':{proof}}}}}).then(()=>null,e=>e.code);await n.invoke('s11_peer_done',{{accepted:c==='FORBIDDEN'}});}})().catch(()=>window.__TAURI_INTERNALS__.invoke('s11_peer_done',{{accepted:false}}));"));
    mark_creation(&state.root(), "peer").map_err(|_| "PEER_CREATION_MARKER_FAILED")?;
    WebviewWindowBuilder::new(&app, "peer", WebviewUrl::App("index.html".into()))
        .visible(false)
        .data_directory(state.root().join("webview-peer"))
        .build()
        .map_err(|_| "PEER_BUILD_FAILED")?;
    let accepted = tauri::async_runtime::spawn_blocking(move || {
        receiver.recv_timeout(Duration::from_secs(25))
    })
    .await
    .map_err(|_| "PEER_WAIT_FAILED")?
    .map_err(|_| "PEER_TIMEOUT")?;
    if accepted {
        Ok(())
    } else {
        Err("PEER_NOT_FORBIDDEN".into())
    }
}
#[tauri::command]
fn s11_peer_done(
    webview: Webview,
    accepted: bool,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    if webview.label() != "peer"
        || webview.url().map_err(|_| "PEER_URL_UNKNOWN")?.as_str() != state.url
    {
        return Err("PEER_CALLBACK_FORBIDDEN".into());
    }
    let sender = state
        .peer_reply
        .lock()
        .take()
        .ok_or("PEER_CALLBACK_UNEXPECTED")?;
    sender.send(accepted).map_err(|_| "PEER_CALLBACK_CLOSED")?;
    Ok(())
}

#[tauri::command]
async fn s11_capture(
    app: AppHandle,
    webview: Webview,
    request: Request<'_>,
    state: State<'_, Arc<TaskBackendFixture>>,
) -> Result<(), String> {
    use base64::Engine;
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    state.admit(&webview, &request)?;
    let q: Named = query(&request)?;
    let names = ["review", "confirmation", "result"];
    if names.get(state.captures.lock().len()).copied() != Some(q.name.as_str()) {
        return Err("CAPTURE_ORDER_INVALID".into());
    }
    let window = app.get_webview_window("main").ok_or("MAIN_MISSING")?;
    let expected = state.expected_main.clone();
    let path = state.root().join(format!("reports/{}.png", q.name));
    let (sender, receiver) = mpsc::channel();
    window
        .with_webview(move |platform| {
            let result = (|| -> Result<(), String> {
                let actual =
                    actual_udf(&platform.environment()).map_err(|_| "CAPTURE_UDF_UNKNOWN")?;
                if actual.identity() != expected.identity() {
                    return Err("CAPTURE_UDF_MISMATCH".into());
                }
                let core = unsafe { platform.controller().CoreWebView2() }
                    .map_err(|_| "CAPTURE_CONTROLLER_UNKNOWN")?;
                let completed = sender.clone();
                let callback = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                    move |status, json| {
                        let result = (|| -> Result<(), String> {
                            status.map_err(|_| "CAPTURE_PROTOCOL_FAILED")?;
                            if json.len() > 16 * 1024 * 1024 {
                                return Err("CAPTURE_LIMIT".into());
                            }
                            let value: serde_json::Value = serde_json::from_str(&json)
                                .map_err(|_| "CAPTURE_RESPONSE_INVALID")?;
                            let bytes = base64::engine::general_purpose::STANDARD
                                .decode(value["data"].as_str().ok_or("CAPTURE_DATA_MISSING")?)
                                .map_err(|_| "CAPTURE_DATA_INVALID")?;
                            if bytes.len() > 8 * 1024 * 1024
                                || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                            {
                                return Err("CAPTURE_DATA_INVALID".into());
                            }
                            fs::write(path, bytes).map_err(|_| "CAPTURE_WRITE_FAILED")?;
                            Ok(())
                        })();
                        let _ = completed.send(result);
                        Ok(())
                    },
                ));
                unsafe {
                    core.CallDevToolsProtocolMethod(
                        windows::core::w!("Page.captureScreenshot"),
                        windows::core::w!("{\"format\":\"png\",\"captureBeyondViewport\":false}"),
                        &callback,
                    )
                }
                .map_err(|_| "CAPTURE_PROTOCOL_UNAVAILABLE")?;
                Ok(())
            })();
            if let Err(error) = result {
                let _ = sender.send(Err(error));
            }
        })
        .map_err(|_| "CAPTURE_THREAD_FAILED")?;
    tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(Duration::from_secs(15)))
        .await
        .map_err(|_| "CAPTURE_WAIT_FAILED")?
        .map_err(|_| "CAPTURE_TIMEOUT")??;
    state.captures.lock().push(q.name);
    Ok(())
}

// COM 对象只在 UI 线程使用；父进程验证 browser 确实在自己的 Job 后才允许页面发起测试。
fn browser_ready(window: WebviewWindow, state: Arc<TaskBackendFixture>, main: bool) {
    let expected = if main {
        state.expected_main.clone()
    } else {
        state.expected_peer.clone()
    };
    let app = window.app_handle().clone();
    let retained = window.clone();
    let failed = state.clone();
    if window.with_webview(move|platform| {
        let result=(||->io::Result<()> {
            let actual=actual_udf(&platform.environment())?;
            if actual.identity()!=expected.identity(){return Err(io::Error::other("ACTUAL_UDF_MISMATCH"));}
            expected.recheck()?;state.actual_directories.lock().push(actual);
            let core=unsafe{platform.controller().CoreWebView2()}.map_err(io::Error::other)?;
            let mut pid=0;unsafe{core.BrowserProcessId(&mut pid)}.map_err(io::Error::other)?;
            if pid==0{return Err(io::Error::other("BROWSER_PID_MISSING"));}
            let label=if main{"main"}else{"peer"};
            println!("STAGE11_BROWSER_READY {label} {pid}");let _=io::stdout().flush();
            let root=state.root();let state=state.clone();let app=app.clone();let window=retained.clone();
            std::thread::spawn(move|| {
                let deadline=Instant::now()+Duration::from_secs(25);
                loop {
                    if root.join(format!("reports/browser-{label}-rejected")).exists(){state.fail(&app,"BROWSER_JOB_MEMBERSHIP_REJECTED");return;}
                    if root.join(format!("reports/browser-{label}-held")).exists()&&state.runtime.binding().is_ok(){
                        let script=if main {
                            if state.started.swap(true,Ordering::AcqRel){return;}
                            "(()=>{const end=performance.now()+15000;const t=setInterval(()=>{if(window.__STAGE11__){clearInterval(t);window.__STAGE11__.start().catch(()=>window.__CC_DESK_DOCUMENT__?.invoke('s11_finish',{success:false}));}else if(performance.now()>end){clearInterval(t);window.__CC_DESK_DOCUMENT__?.invoke('s11_finish',{success:false});}},20);})()".to_owned()
                        } else {match state.peer_script.lock().take(){Some(script)=>script,None=>{state.fail(&app,"PEER_SCRIPT_MISSING");return;}}};
                        println!("STAGE11_PAGE_START {label}");
                        if window.eval(script).is_err(){state.fail(&app,"PAGE_START_FAILED");}return;
                    }
                    if Instant::now()>=deadline {state.fail(&app,"BROWSER_OR_DOCUMENT_READY_TIMEOUT");return;}
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
            Ok(())
        })();if result.is_err(){state.fail(&app,"ACTUAL_UDF_OR_BROWSER_UNKNOWN");}
    }).is_err(){failed.fail(window.app_handle(),"WEBVIEW_UI_THREAD_UNAVAILABLE");}
}

fn worker(native: bool) {
    webview_configuration_absent().unwrap();
    let fixture = Arc::new(Fixture::new().unwrap());
    for name in ["webview-main", "webview-peer"] {
        fs::create_dir(fixture.root.join(name)).unwrap();
    }
    let expected_main = Directory::open_absolute(&fixture.root.join("webview-main")).unwrap();
    let expected_peer = Directory::open_absolute(&fixture.root.join("webview-peer")).unwrap();
    println!("STAGE11_CHILD_READY {}", fixture.root.display());
    io::stdout().flush().unwrap();
    let ready_deadline = Instant::now() + Duration::from_secs(25);
    while !fixture.root.join("reports/parent-held").exists() {
        assert!(
            Instant::now() < ready_deadline,
            "PARENT_DIRECTORY_LEASE_TIMEOUT"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let assets = Arc::new(
        AssetBundle::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".audit/v027-stage11-webview-dist"),
        )
        .unwrap(),
    );
    assets.verify_production_sources().unwrap();
    let _configuration = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0)
        .open(fixture.root.join("codex-home/config.toml"))
        .unwrap();
    let _executable = if native {
        system_configuration_absent().unwrap();
        Some(verified_executable().unwrap())
    } else {
        None
    };
    if native {
        assert!(probe(
            &fixture,
            "stage11-strict-config",
            &["app-server", "--strict-config", "--print-startup-config"]
        )
        .unwrap()
        .success());
    }
    let backend = BackendFixture::new(
        fixture.clone(),
        if native {
            BackendProgram::InstalledPinned
        } else {
            BackendProgram::UnavailableFixture
        },
    );
    let expiry: serde_json::Value =
        serde_json::from_slice(&fs::read(&backend.unrelated_rollout).unwrap()).unwrap();
    let expiry_id = expiry["payload"]["id"].as_str().unwrap().to_owned();
    let controls = [
        &backend.active_rollout,
        &backend.unrelated_rollout,
        &backend.other_rollout,
    ]
    .into_iter()
    .map(|path| (path.clone(), fs::read(path).unwrap()))
    .collect();
    let runtime = Arc::new(NativeRuntime::new(backend.launch.clone()));
    let server = AssetServer::start(assets.clone()).unwrap();
    let state = Arc::new(TaskBackendFixture {
        backend: Mutex::new(backend),
        runtime: runtime.clone(),
        native,
        expiry_id,
        controls,
        expected_main,
        expected_peer,
        actual_directories: Mutex::new(vec![]),
        main_loaded: AtomicBool::new(false),
        peer_loaded: AtomicBool::new(false),
        started: AtomicBool::new(false),
        peer_created: AtomicBool::new(false),
        peer_script: Mutex::new(None),
        peer_reply: Mutex::new(None),
        confirmations: AtomicUsize::new(0),
        inspections: AtomicUsize::new(0),
        checkpoints: Mutex::new(vec![]),
        captures: Mutex::new(vec![]),
        failure: Mutex::new(None),
        finished: AtomicBool::new(false),
        success: AtomicBool::new(false),
        url: server.url.clone(),
    });
    let mut context = tauri::generate_context!("src/tests/fixtures/document/tauri.conf.json");
    let url: tauri::Url = server.url.parse().unwrap();
    context.config_mut().build.dev_url = Some(url.clone());
    context.config_mut().build.frontend_dist = Some(FrontendDist::Url(url));
    context.config_mut().app.security.csp = Some(Csp::Policy(assets.csp.clone()));
    context.config_mut().app.windows.push(WindowConfig {
        label: "main".into(),
        url: WebviewUrl::App("index.html".into()),
        title: "CC Desk task-isolated synthetic deletion".into(),
        width: 1200.,
        height: 1000.,
        visible: true,
        data_directory: Some(fixture.root.join("webview-main")),
        ..Default::default()
    });
    let main = take_main_config(context.config_mut()).unwrap();
    let setup = runtime.clone();
    let creation_root = fixture.root.clone();
    let pages = state.clone();
    let dispatch = state.clone();
    let formal: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        crate::commands::native_get_scope,
        crate::commands::native_list_resources,
        crate::cli::commands::cli_preview_session_delete,
        crate::cli::commands::cli_confirm_session_delete,
        crate::cli::commands::cli_inspect_session_delete,
        s11_fixture,
        s11_checkpoint,
        s11_proof,
        s11_peer,
        s11_peer_done,
        s11_change_source,
        s11_capture,
        s11_finish
    ];
    let app = tauri::Builder::default()
        .any_thread()
        .manage(runtime)
        .manage(state.clone())
        .invoke_handler(move |invoke| {
            let command = invoke.message.command();
            if command == "cli_confirm_session_delete" {
                dispatch.confirmations.fetch_add(1, Ordering::AcqRel);
                if native && system_configuration_absent().is_err() {
                    dispatch.fail(
                        invoke.message.webview_ref().app_handle(),
                        "CLI_ISOLATION_PRECHECK_FAILED",
                    );
                    invoke.resolver.reject("CLI_ISOLATION_PRECHECK_FAILED");
                    return true;
                }
            }
            if command == "cli_inspect_session_delete" {
                dispatch.inspections.fetch_add(1, Ordering::AcqRel);
            }
            formal(invoke)
        })
        .setup(move |app| {
            webview_configuration_absent()?;
            mark_creation(&creation_root, "main")?;
            setup.initialize_main(app, &main)?;
            Ok(())
        })
        .on_page_load(move |webview, payload| {
            if !matches!(payload.event(), PageLoadEvent::Finished)
                || payload.url().as_str() != pages.url
            {
                return;
            }
            let main = webview.label() == "main";
            let first = if main {
                !pages.main_loaded.swap(true, Ordering::AcqRel)
            } else {
                webview.label() == "peer" && !pages.peer_loaded.swap(true, Ordering::AcqRel)
            };
            if first {
                if let Some(window) = webview.app_handle().get_webview_window(webview.label()) {
                    browser_ready(window, pages.clone(), main);
                }
            }
        })
        .build(context)
        .unwrap();
    let watchdog = state.clone();
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(if native { 210 } else { 70 });
        while !watchdog.finished.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if !watchdog.finished.load(Ordering::Acquire) {
            let code = if watchdog.checkpoints.lock().is_empty() {
                "DOCUMENT_OR_ASSETS_NOT_READY"
            } else {
                "FRONTEND_COMPLETION_TIMEOUT"
            };
            watchdog.fail(&handle, code);
        }
    });
    let exit = app.run_return(|_, _| {});
    while state
        .backend
        .lock()
        .repository
        .admission()
        .check_metadata_idle()
        .is_err()
    {
        println!("STAGE11_METADATA_OWNER_PENDING");
        std::thread::sleep(Duration::from_secs(1));
    }
    webview_configuration_absent().unwrap();
    if native {
        system_configuration_absent().unwrap();
    }
    let report = serde_json::json!({"native":native,"success":state.success.load(Ordering::Acquire),"failure":*state.failure.lock(),"exitCode":exit,
        "checkpoints":*state.checkpoints.lock(),"confirmIpcCount":state.confirmations.load(Ordering::Acquire),"inspectIpcCount":state.inspections.load(Ordering::Acquire),
        "actualUdfCount":state.actual_directories.lock().len(),"engineVersion":tauri::webview_version().unwrap(),"captures":*state.captures.lock(),
        "assetBuildHead":assets.manifest["buildHead"],"assetFiles":assets.manifest["files"],"productionModules":assets.manifest["modules"],"driverSources":assets.manifest["drivers"],"metadataOwnerSettled":true});
    fs::write(
        fixture.root.join("reports/webview-result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("STAGE11_RESULT {report}");
    assert_eq!(exit, 0);
    assert_eq!(report["success"], true);
    assert_eq!(report["actualUdfCount"], 2);
}

fn driver(native: bool) {
    use windows::Win32::System::{
        JobObjects::IsProcessInJob,
        Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        },
    };
    let _desktop = crate::tests::NATIVE_WEBVIEW_TEST_LEASE.lock();
    assert_eq!(
        std::env::var("CC_DESK_NATIVE_DELETE_WEBVIEW_GO").as_deref(),
        Ok(if native {
            "stage11-native"
        } else {
            "stage11-no-native"
        })
    );
    webview_configuration_absent().unwrap();
    let parent = Fixture::new().unwrap();
    let log_path = parent.root.join("reports/worker.log");
    let log = File::create(&log_path).unwrap();
    let job = ProbeJob::new().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "tests::native_cli_delete_live::webview::NativeDeleteWebView_Worker_099",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .envs(&parent.environment)
        .env(
            "CC_DESK_STAGE11_WORKER",
            if native { "native" } else { "no-native" },
        )
        .current_dir(&parent.project)
        .stdin(Stdio::null())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .creation_flags(0x08000000 | 0x4);
    let mut child = command.spawn().unwrap();
    let assigned = job.assign_and_resume(&child);
    if assigned.is_err() {
        let _ = child.kill();
    }
    let deadline = Instant::now() + Duration::from_secs(240);
    let mut status = None;
    let mut forced = false;
    let mut failed = assigned.is_err();
    let mut child_root: Option<PathBuf> = None;
    let mut leases = vec![];
    let mut browsers: Vec<(String, ProbeJob)> = vec![];
    let mut unresolved_browser = false;
    let mut verified_browsers = std::collections::BTreeSet::new();
    let mut creation_pending = std::collections::BTreeSet::new();
    let mut pending_notice = Instant::now();
    let mut seen_lines = 0;
    loop {
        let observed = (|| -> io::Result<()> {
            let mut bytes = vec![];
            File::open(&log_path)?
                .take(4 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(io::Error::other("WORKER_LOG_LIMIT"));
            }
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<_> = text
                .split_inclusive('\n')
                .filter(|line| line.ends_with('\n'))
                .collect();
            for line in lines.iter().skip(seen_lines) {
                if let Some(path) = line.trim_end().strip_prefix("STAGE11_CHILD_READY ") {
                    if child_root.is_some() {
                        return Err(io::Error::other("DUPLICATE_CHILD_READY"));
                    }
                    let root = PathBuf::from(path);
                    leases = parent_child_leases(&parent, &root)?;
                    fs::write(root.join("reports/parent-held"), b"held\n")?;
                    child_root = Some(root);
                }
                if let Some(browser) = line.trim_end().strip_prefix("STAGE11_BROWSER_READY ") {
                    let (label, pid) = browser
                        .split_once(' ')
                        .ok_or_else(|| io::Error::other("BROWSER_SIGNAL_INVALID"))?;
                    if !matches!(label, "main" | "peer")
                        || browsers.iter().any(|(seen, _)| seen == label)
                    {
                        return Err(io::Error::other("BROWSER_SIGNAL_INVALID"));
                    }
                    let pid: u32 = pid
                        .parse()
                        .map_err(|_| io::Error::other("BROWSER_PID_INVALID"))?;
                    let opened = unsafe {
                        OpenProcess(
                            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                            false,
                            pid,
                        )
                    };
                    let browser = match opened {
                        Ok(handle) => ProbeJob(handle),
                        Err(_) => {
                            unresolved_browser = true;
                            return Err(io::Error::other("BROWSER_HANDLE_UNAVAILABLE"));
                        }
                    };
                    let mut contained = windows::core::BOOL(0);
                    let verified =
                        unsafe { IsProcessInJob(browser.0, Some(job.0), &mut contained) }.is_ok()
                            && contained.as_bool();
                    browsers.push((label.into(), browser));
                    let root = child_root
                        .as_ref()
                        .ok_or_else(|| io::Error::other("BROWSER_BEFORE_CHILD"))?;
                    fs::write(
                        root.join(format!(
                            "reports/browser-{label}-{}",
                            if verified { "held" } else { "rejected" }
                        )),
                        b"observed\n",
                    )?;
                    if !verified {
                        unresolved_browser = true;
                        println!("STAGE11_BROWSER_OUTSIDE_OWN_JOB {label}");
                        return Err(io::Error::other("BROWSER_NOT_IN_OWN_JOB"));
                    }
                    verified_browsers.insert(label.to_owned());
                }
            }
            seen_lines = lines.len();
            Ok(())
        })();
        failed |= observed.is_err();
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => status = value,
                Err(_) => failed = true,
            }
        }
        // 先观察根进程退出，再读取创建标记，避免退出前最后一次创建遗漏于收口判断。
        if let Some(root) = child_root.as_ref() {
            match pending_creations(root, &verified_browsers) {
                Ok(pending) => creation_pending = pending,
                Err(_) => {
                    failed = true;
                    unresolved_browser = true;
                }
            }
        }
        let group_empty = job.empty().unwrap_or(false);
        let browsers_exited=browsers.iter().all(|(_,process)|unsafe{WaitForSingleObject(process.0,0)}==windows::Win32::Foundation::WAIT_OBJECT_0);
        if status.is_some()
            && group_empty
            && browsers_exited
            && !unresolved_browser
            && creation_pending.is_empty()
        {
            break;
        }
        if !forced && (failed || Instant::now() >= deadline) {
            let _ = job.terminate();
            let _ = child.kill();
            forced = true;
        }
        for lease in &leases {
            failed |= lease.source_current().is_err();
        }
        if (unresolved_browser || !creation_pending.is_empty())
            && pending_notice.elapsed() >= Duration::from_secs(5)
        {
            println!("STAGE11_BROWSER_LIFETIME_UNPROVEN_PENDING");
            pending_notice = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // try_wait 已确认根退出；wait 只取缓存状态，并明确收口 std::Child 的生命周期。
    let reaped = child.wait().unwrap();
    assert_eq!(status, Some(reaped));
    let root = child_root.as_ref().expect("CHILD_ROOT_NOT_ADMITTED");
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("reports/webview-result.json")).unwrap())
            .unwrap();
    println!(
        "STAGE11_DRIVER_RESULT {}",
        serde_json::json!({"native":native,"forced":forced,"isolationFailed":failed,"workerRoot":root,"workerExit":status.unwrap().code(),"rootReaped":true,"jobEmpty":true,"browserCount":browsers.len(),"workerSuccess":report["success"]})
    );
    assert!(!forced && !failed);
    assert!(status.unwrap().success());
    assert_eq!(browsers.len(), 2);
    assert_eq!(report["success"], true);
}

#[test]
#[ignore = "requires frozen-source NoNative WebView Go"]
fn NativeDeleteWebView_NoNative_090() {
    driver(false);
}
#[test]
#[ignore = "requires separate fresh-fixture native WebView Go"]
fn NativeDeleteWebView_Native_091() {
    driver(true);
}
#[test]
#[ignore = "only the owned stage11 parent driver may invoke this worker"]
fn NativeDeleteWebView_Worker_099() {
    let mode = std::env::var("CC_DESK_STAGE11_WORKER").unwrap();
    assert!(matches!(mode.as_str(), "native" | "no-native"));
    worker(mode == "native");
}
