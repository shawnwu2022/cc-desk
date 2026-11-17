//! 独立真实 Relay + WASM pull 验收；复用原精确 Native owner，不伪造 activation。
use super::*;
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn BrowserSnapshotPull_OriginalRelayWasmHostSmoke_001() {
    if isolated("snapshot_pull::BrowserSnapshotPull_OriginalRelayWasmHostSmoke_001") {
        return;
    }
    browser_case("smoke").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn BrowserSnapshotPull_ClientPullOldLiveRejects_002() {
    if isolated("snapshot_pull::BrowserSnapshotPull_ClientPullOldLiveRejects_002") {
        return;
    }
    browser_case("client-pull").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn BrowserSnapshotPull_HostPullOldLiveRejects_003() {
    if isolated("snapshot_pull::BrowserSnapshotPull_HostPullOldLiveRejects_003") {
        return;
    }
    browser_case("host-pull").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn BrowserSnapshotPull_WithoutApplicationRejects_004() {
    if isolated("snapshot_pull::BrowserSnapshotPull_WithoutApplicationRejects_004") {
        return;
    }
    browser_case("no-application").await;
}
async fn browser_case(case: &str) {
    let (f, mut cleanup) = setup();
    let mut relay = NetworkRelayFixture::start_with_browser(None, 30000, true).await;
    let source = admit(&f);
    let scope = source.scope().unwrap();
    let (consent, mut current, policy) = NativeDelegationConsent::test_network(&source, device());
    policy.test_network_origin(&mut current, relay.origin.clone());
    policy.test_budget_progress();
    policy.test_budget_total(15000);
    let session = Arc::new(
        cleanup
            .host
            .reserve_delegation(DelegatedRunOwner::capture(source, consent).unwrap())
            .unwrap(),
    );
    let (accepted, ready) = tokio::sync::oneshot::channel();
    let builder = renewal_material_duration(&relay, accepted, Duration::from_secs(20));
    let builder = if case == "client-pull" {
        builder.test_live_keepalive()
    } else {
        builder.test_live_snapshot_pull()
    };
    let socket = session
        .test_begin_socket(&current, &f.supervisor, builder)
        .unwrap();
    ready.await.unwrap();
    let (_, spki) = network_identities::identity(false, false);
    let config = serde_json::json!({"case":case,"origin":relay.origin,"scope":scope,"hostSpki":spki.iter().map(|b|format!("{b:02x}")).collect::<String>()}).to_string();
    let child = tokio::task::spawn_blocking(move || {
        let mut process = std::process::Command::new("node")
            .current_dir(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap(),
            )
            .args([
                "--experimental-strip-types",
                "remote/ui-terminal/src/browserSnapshotPullRelay.fixture.ts",
                &config,
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if process.try_wait().unwrap().is_some() {
                return process.wait_with_output().unwrap();
            }
            if Instant::now() >= end {
                let _ = process.kill();
                return process.wait_with_output().unwrap();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let admitted = socket.wait_admitted().await;
    let output = child.await.unwrap();
    socket.request_close();
    let joined = socket.wait_closed().await;
    drop(session);
    let host_closed = cleanup.close();
    let relay_closed = relay.close().await;
    assert!(
        output.status.success()
            && (admitted.is_ok() == (case == "smoke"))
            && joined.is_ok()
            && host_closed
            && relay_closed,
        "status={} admitted={admitted:?} joined={joined:?} host={host_closed} relay={relay_closed}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence = String::from_utf8_lossy(&output.stdout);
    assert!(evidence.contains("originalRetired"));
    eprintln!("TEST_ONLY_BROWSER_LIVE_RESULT {evidence}");
}
