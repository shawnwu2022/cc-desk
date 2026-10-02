//! Disposable, explicitly scoped evidence fixture. Never a production admission table.
//! The two ignored entrypoints are selected exactly; no normal application startup occurs.
#[path = "version_history_payload/inventory.rs"]
mod inventory;
#[path = "version_history_payload/token.rs"]
mod token;

use crate::cli::{profiles::error, snapshot::CallerIdentity, types::WireU64};
use crate::version_history::{
    catalog::{parse_release, CatalogService, CatalogSource, OfficialGitHub, ReleaseMetadata},
    download::PrepareService,
    policy::HostPlatform,
    verified_package::{sha256, verify_fixture_payload},
    windows::{
        durability::DurableRecord,
        files::{ComponentName, Directory, FileAccess, PrivateDirectory},
        lease::LeaseFiles,
        process::{CommandLine, JobKind, PreparedProcess},
        security::CurrentUser,
    },
};
use serde_json::json;
use std::{
    ffi::OsStr,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const INPUT: &str =
    include_str!("../../../tests/fixtures/version-history-payload/v0.17.7-selection.json");
const PROVENANCE: &str =
    include_str!("../../../tests/fixtures/version-history-payload/provenance.json");
const INSTALLER_HASH: &str = "e9ffbc5ba627f0c133a4385db404342a7344729339185e6f9b8ee6b5969086ac";
const SIGNATURE_HASH: &str = "30b26f21c76d8c30bf4ca042ff699f1dd5d181af54f0e8956a3bff10650b80e2";
const MAX_REPORT: usize = 4 * 1024 * 1024;

fn blocked(message: &'static str) -> io::Error {
    io::Error::other(message)
}
fn name(value: &str) -> io::Result<ComponentName> {
    ComponentName::new(OsStr::new(value))
}
fn safe<T>(value: Result<T, crate::cli::types::SafeError>) -> io::Result<T> {
    value.map_err(|e| io::Error::other(e.code))
}
fn write_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(blocked("fixture file budget exceeded"));
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}
fn report(root: &Path, file: &str, value: &impl serde::Serialize) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if bytes.len() > MAX_REPORT {
        return Err(blocked("fixture report budget exceeded"));
    }
    write_new(&root.join(file), &bytes)
}
fn fixture_root() -> io::Result<PathBuf> {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || std::env::var("RUNNER_ENVIRONMENT").as_deref() != Ok("github-hosted")
        || std::env::var("CC_DESK_PAYLOAD_FIXTURE").as_deref() != Ok("v0.17.7-evidence-only")
        || !cfg!(target_arch = "x86_64")
    {
        return Err(blocked(
            "only the explicit disposable Windows x64 workflow may run this fixture",
        ));
    }
    let temp = PathBuf::from(
        std::env::var_os("RUNNER_TEMP")
            .ok_or_else(|| blocked("missing runner temporary directory"))?,
    );
    Directory::open_absolute(&temp)?;
    Ok(temp.join("ccdesk-v0.17.7-payload-evidence"))
}

// 检查显式隔离作业通过真实受限子进程令牌门禁后才能进入安装证据采集。
#[test]
#[ignore = "only the dedicated disposable payload workflow; mutates fixture-owned HKCU slots"]
fn HistoryPayload_Controller_001() {
    let root = fixture_root().expect("dedicated disposable workflow gate");
    let user = CurrentUser::capture().unwrap();
    let parent = Directory::open_absolute(root.parent().unwrap()).unwrap();
    let owned = PrivateDirectory::create_new(
        parent,
        name(root.file_name().unwrap().to_str().unwrap()).unwrap(),
        &user,
    )
    .unwrap();
    let outcome = token::run_worker(&root);
    report(&root, "controller-outcome.json", &json!({
        "schema":1, "status":if outcome.is_ok() {"worker-completed"} else {"blocked"},
        "error":outcome.as_ref().err().map(ToString::to_string),
        "productionAdmission":false, "historicalApplicationLaunched":false,
        "limits":"payload/effect observation only; no switch/return or shared-data compatibility claim"
    })).unwrap();
    owned.verify(&user).unwrap();
    outcome.expect(
        "restricted-token fixture failed; preserve all evidence and do not fall back to elevation",
    );
}

// 检查实际官方签名安装包在正向缺失或 fixture-owned 状态中生成的完整文件与已知安装副作用。
#[test]
#[ignore = "private child entrypoint; must pass the controller token receipt"]
fn HistoryPayload_Worker_002() {
    let root = fixture_root().unwrap();
    let result = worker(&root);
    report(&root, "worker-outcome.json", &json!({
        "schema":1, "status":if result.is_ok() {"observed-for-review"} else {"blocked"},
        "error":result.as_ref().err().map(ToString::to_string), "productionAdmission":false,
        "effectsCoverage":"complete owned install tree plus explicitly listed registration/shortcut/data locations; not a whole-system trace"
    })).unwrap();
    result.expect("fixture stopped; unknown results are evidence, never admission");
}

fn worker(root: &Path) -> io::Result<()> {
    token::verify_worker(root)?; // Before download, profile seeding or installer effects.
    report(
        root,
        "environment.json",
        &json!({
            "schema":1, "fixtureCase":std::env::var("CC_DESK_PAYLOAD_CASE").ok(),
            "sourceCommit":std::env::var("CC_DESK_PAYLOAD_SOURCE_SHA").ok(),
            "runId":std::env::var("GITHUB_RUN_ID").ok(),
            "runAttempt":std::env::var("GITHUB_RUN_ATTEMPT").ok(),
            "runnerImage":std::env::var("ImageOS").ok(),
            "runnerImageVersion":std::env::var("ImageVersion").ok(),
            "architecture":std::env::consts::ARCH,"builtPackageVersion":env!("CARGO_PKG_VERSION"),
            "observedAtUtc":chrono::Utc::now().to_rfc3339(),
            "profileRedirection":false,"accountOrPolicyChanges":false
        }),
    )?;
    let user = CurrentUser::capture()?;
    user.require_unelevated()?;
    let parent = Directory::open_absolute(root.parent().unwrap())?;
    let owned = Arc::new(PrivateDirectory::open_existing(
        parent,
        name(root.file_name().unwrap().to_str().unwrap())?,
        &user,
    )?);
    report(
        root,
        "source-provenance.json",
        &serde_json::from_str::<serde_json::Value>(PROVENANCE)?,
    )?;
    let case = std::env::var("CC_DESK_PAYLOAD_CASE").unwrap_or_default();
    if !matches!(case.as_str(), "clean" | "seeded-existing") {
        return Err(blocked("unknown fixture case"));
    }
    // Source controls were reviewed separately from the measured output. No
    // installer-derived data or metadata adds a production policy entry.
    let scope = inventory::Scope::capture()?;
    scope.require_absent()?;
    report(root, "initial-absence.json", &scope.absence_report()?)?;
    let runtime = inventory::existing_webview()?;
    report(root, "existing-webview.json", &runtime)?;
    let install_name = "CC Desk 历史 payload";
    let install =
        PrivateDirectory::create_new(owned.directory().clone(), name(install_name)?, &user)?;
    let install_path = root.join(install_name);
    let control = Arc::new(PrivateDirectory::create_new(
        owned.directory().clone(),
        name("control")?,
        &user,
    )?);
    let packages =
        PrivateDirectory::create_new(owned.directory().clone(), name("packages")?, &user)?;
    let package_path = root.join("packages");
    let source = Arc::new(OfficialGitHub::new().map_err(|e| io::Error::other(e.code))?);
    let expected = safe(parse_release(INPUT.as_bytes()))?;
    let actual = safe(source.release(expected.id))?;
    check_selected_release(&expected, &actual)?;
    report(
        root,
        "selection.json",
        &serde_json::from_str::<serde_json::Value>(INPUT)?,
    )?;
    let catalog = Arc::new(CatalogService::new(
        source.clone(),
        HostPlatform::WindowsX64,
    ));
    let caller = CallerIdentity {
        instance_id: "payload-evidence-fixture".into(),
        window_label: "fixture".into(),
        webview_epoch: WireU64::parse("1").unwrap(),
    };
    let mut cursor = None;
    let selected = loop {
        let page = safe(catalog.list(&caller, cursor.as_deref()))?;
        if let Some(row) = page
            .rows
            .into_iter()
            .find(|r| r.asset_id.as_deref() == Some("576637999"))
        {
            break safe(catalog.select(&caller, &row.release_id, "576637999"))?;
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Err(blocked("pinned public release was not observed"));
        }
    };
    let parent_dir =
        cap_std::fs::Dir::open_ambient_dir(&package_path, cap_std::ambient_authority())?;
    let prepare = safe(PrepareService::production(
        catalog,
        parent_dir,
        Arc::new(|_| Ok(())),
    ))?;
    let ticket = safe(prepare.begin_prepare(&caller, &selected.selection_token))?;
    safe(prepare.prepare_history(&caller, &ticket.transaction_id))?;
    check_selected_release(&expected, &safe(source.release(expected.id))?)?;
    let executable = root.join("official-v0.17.7.exe");
    let pinned = safe(
        prepare.with_verified_package(&caller, &ticket.transaction_id, |package| {
            if package.sha256() != INSTALLER_HASH
                || package.size() != 4_966_193
                || package.selection().signature().id() != 576_637_991
                || package.selection().signature().sha256() != SIGNATURE_HASH
                || package.payload_identity_authenticated()
            {
                return Err(error("HISTORY_FIXTURE_SELECTION_CHANGED"));
            }
            write_new(&executable, package.bytes())
                .map_err(|_| error("HISTORY_FIXTURE_WRITE_FAILED"))?;
            let pinned = owned
                .directory()
                .open_file(
                    name("official-v0.17.7.exe")
                        .map_err(|_| error("HISTORY_FIXTURE_WRITE_FAILED"))?,
                    FileAccess::Read,
                )
                .map_err(|_| error("HISTORY_FIXTURE_WRITE_FAILED"))?;
            if pinned
                .digest()
                .map_err(|_| error("HISTORY_FIXTURE_WRITE_FAILED"))?
                != package.sha256()
            {
                return Err(error("HISTORY_FIXTURE_COPY_CHANGED"));
            }
            Ok(pinned)
        }),
    )?;
    // Preserve detached bytes from the bounded, private production store while
    // the service still retains its original verified package file object.
    let signature = find_signature(packages.directory().clone(), 0)?;
    if signature.len() != 420 || sha256(&signature) != SIGNATURE_HASH {
        return Err(blocked("retained signature identity differs"));
    }
    let config: serde_json::Value = serde_json::from_str(include_str!("../../tauri.conf.json"))?;
    let key = config["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or_else(|| blocked("missing committed publisher key"))?;
    let bytes = bounded_read(&executable, 4_966_193)?;
    safe(verify_fixture_payload(
        &bytes,
        &signature,
        key,
        INSTALLER_HASH,
        4_966_193,
    ))?;
    write_new(&root.join("official-v0.17.7.exe.sig"), &signature)?;
    report(
        root,
        "verified-bytes.json",
        &json!({"installerSha256":INSTALLER_HASH,"installerSize":bytes.len(),"signatureSha256":SIGNATURE_HASH,"signatureSize":signature.len(),"committedPublicKeySha256":sha256(key.as_bytes()),"publisherKeyId":"4990489074065B06","verification":"production PrepareService and Minisign; exact copied file pinned through process consumption","installerFileIdentity":pinned.identity(),"payloadAdmission":false}),
    )?;
    drop(bytes);
    scope.require_absent()?; // Downloads/build prep cannot silently bless changed profile state.
    if case == "seeded-existing" {
        scope.seed(&install_path)?;
    }
    report(root, "before.json", &scope.snapshot(root, "before")?)?;
    let before = inventory::capture_tree(install.directory().clone())?;
    report(root, "install-before.json", &before)?;
    let leases = LeaseFiles::open(control.clone(), &user)?;
    let admission = leases.acquire_control()?;
    let mut lease = leases.acquire_exclusive(&admission)?;
    drop(admission);
    let command = CommandLine::nsis(executable.as_os_str(), install_path.as_os_str())?;
    report(
        root,
        "command.json",
        &json!({"application":executable,"commandLine":command.text(),"case":case,"installRoot":install_path,"sourceCommit":"77707e3b03187aa2ed96f5ab780f62f14c1e4ffc","historicalAppLaunch":false}),
    )?;
    let intent = DurableRecord::create(
        control.clone(),
        name("fixture-effect-intent.json")?,
        &serde_json::to_vec(
            &json!({"installerSha256":INSTALLER_HASH,"case":case,"installIdentity":install.directory().identity(),"effectScope":"fresh disposable owned fixture only"}),
        )?,
        &user,
    )?;
    intent.verify()?;
    let mut process = PreparedProcess::create_suspended(
        pinned,
        command,
        JobKind::Installer,
        control,
        &user,
        &mut lease,
    )?;
    let identity = process.persist_identity(&user)?;
    let observed = process.probe_exact()?;
    let installer_token = token::require_process(observed.pid(), &token::current()?)?;
    report(
        root,
        "installer-token.json",
        &json!({"process": observed.identity(), "token": installer_token}),
    )?;
    process.resume(&identity)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    let terminal = loop {
        if let Some(receipt) = process.wait_terminal(100)? {
            if process.active_processes()? == 0 {
                break receipt;
            }
        }
        if Instant::now() >= deadline {
            return Err(blocked(
                "installer terminal/job-empty receipt unavailable within 120 seconds",
            ));
        }
    };
    let terminal_record = process.persist_terminal(&user)?;
    report(
        root,
        "terminal.json",
        &json!({"process":observed.identity(),"exitCode":terminal.exit_code(),"activeProcesses":0,"durableReceiptSha256":terminal_record.digest()}),
    )?;
    // Preserve after-state even when exit is nonzero. No cleanup or retry.
    let captured = inventory::capture_tree(install.directory().clone());
    report(root, "after.json", &scope.snapshot(root, "after")?)?;
    let tree = captured?;
    report(root, "install-after.json", &tree)?;
    inventory::export_tree(&install_path, root, &tree)?;
    let pe = inventory::pe_identity(&install_path.join("cc-desk.exe"))?;
    report(root, "installed-pe.json", &pe)?;
    if terminal.exit_code() != 0 {
        return Err(blocked(
            "official installer returned nonzero; preserved output is not admitted",
        ));
    }
    inventory::check_observations(&case, &tree, &pe, &scope, &install_path)?;
    user.require_unelevated()?;
    packages.verify(&user)?;
    report(
        root,
        "review-required.json",
        &json!({"schema":1,"version":"0.17.7","installerSha256":INSTALLER_HASH,"completeOwnedInstallInventory":true,"knownEffectsCaptured":true,"case":case,"productionAdmission":false,"unknowns":["compiled installer source correspondence is not independently attested","effects outside enumerated locations are not a whole-system trace","generated uninstaller reproducibility requires cross-case review","real switch and return remain a separate gate"]}),
    )
}

fn check_selected_release(expected: &ReleaseMetadata, actual: &ReleaseMetadata) -> io::Result<()> {
    let mut selected = actual.clone();
    selected
        .assets
        .retain(|a| [576_637_999, 576_637_991].contains(&a.id));
    selected.assets.sort_by_key(|a| a.id);
    let mut expected = expected.clone();
    expected.assets.sort_by_key(|a| a.id);
    if selected != expected {
        return Err(blocked(
            "official release or full selected asset tuple changed",
        ));
    }
    Ok(())
}
fn bounded_read(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > max {
        return Err(blocked("file budget exceeded"));
    }
    let mut bytes = Vec::new();
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(blocked("file grew beyond budget"));
    }
    Ok(bytes)
}
fn find_signature(dir: Arc<Directory>, depth: usize) -> io::Result<Vec<u8>> {
    if depth > 2 {
        return Err(blocked("unexpected package store layout"));
    }
    let mut found = None;
    for child in dir.read_children(4)? {
        let path = PathBuf::from(dir.path()?).join(child.os_string());
        if child.os_string() == OsStr::new("signature.bin") {
            let guard = dir.open_file(child, FileAccess::Read)?;
            let bytes = bounded_read(&path, 420)?;
            guard.verify()?;
            if found.replace(bytes).is_some() {
                return Err(blocked("ambiguous signature"));
            }
        } else if fs::symlink_metadata(path)?.is_dir() {
            let bytes = find_signature(dir.open_directory(child)?, depth + 1)?;
            if found.replace(bytes).is_some() {
                return Err(blocked("ambiguous signature"));
            }
        }
    }
    found.ok_or_else(|| blocked("production signature file missing"))
}

// 检查签名或安装包完整元数据变化拒绝固定版本证据。
#[test]
fn HistoryPayload_Metadata_003() {
    let expected = parse_release(INPUT.as_bytes()).unwrap();
    check_selected_release(&expected, &expected).unwrap();
    for field in ["id", "size", "updated_at", "digest", "browser_download_url"] {
        let mut value: serde_json::Value = serde_json::from_str(INPUT).unwrap();
        let asset = &mut value["assets"][1];
        asset[field] = match field {
            "id" => json!(576637992),
            "size" => json!(421),
            "updated_at" => json!("2026-09-20T10:44:57Z"),
            "digest" => json!(format!("sha256:{}", "0".repeat(64))),
            _ => {
                json!("https://github.com/shawnwu2022/cc-desk/releases/download/v0.17.7/other.sig")
            }
        };
        let changed: ReleaseMetadata = serde_json::from_value(value).unwrap();
        assert!(
            check_selected_release(&expected, &changed).is_err(),
            "changed {field} must block"
        );
    }
}
