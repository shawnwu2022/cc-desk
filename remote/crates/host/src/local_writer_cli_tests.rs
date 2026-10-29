//! Manual offline --help qualification of an explicitly reviewed installed Codex.
//! No account, credential, prompt, model, shell wrapper or remote-control entry.
use super::*;
use cc_desk_native_runtime::{BackendPtyLaunch, NativeOwnedPtyRun, NativePtySize};
use std::{io::Read, path::PathBuf};

#[test]
#[ignore = "manual installed-CLI qualification; explicit path/hash and empty private profile"]
fn installed_codex_help_uses_empty_home_and_private_pty_reply_gate() {
    if !supervision::run(
        "cli_tests::installed_codex_help_uses_empty_home_and_private_pty_reply_gate",
    ) {
        return;
    }
    let executable = PathBuf::from(std::env::var_os("CC_DESK_TEST_ONLY_CODEX_EXE").unwrap());
    let expected = std::env::var("CC_DESK_TEST_ONLY_CODEX_SHA256").unwrap();
    assert!(executable.is_absolute() && executable.file_name().unwrap() == "codex.exe");
    assert!(expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()));
    let mut file = std::fs::File::open(&executable).unwrap();
    // Reviewed installed0.164 fixture is354558768 bytes. This qualification-only
    // hash-work cap is not a product upload, message or executable admission limit.
    assert!(file.metadata().unwrap().len() <= 512 * 1024 * 1024);
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = [0; 8192];
    loop {
        let n = file.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    let actual = hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        actual,
        expected.to_ascii_lowercase(),
        "installed executable changed since source review"
    );
    let dir = tempfile::tempdir().unwrap();
    let codex_home = dir.path().join("codex-home");
    std::fs::create_dir(&codex_home).unwrap();
    assert!(!codex_home.join(".env").exists() && !codex_home.join("auth.json").exists());
    let system_root = std::env::var_os("SystemRoot").unwrap();
    let mut env = vec![("SystemRoot".into(), system_root)];
    for name in [
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "XDG_DATA_HOME",
    ] {
        env.push((name.into(), dir.path().as_os_str().to_owned()));
    }
    env.push(("CODEX_HOME".into(), codex_home.as_os_str().to_owned()));
    env.push(("NO_COLOR".into(), "1".into()));
    let recipe = BackendPtyLaunch::from_backend_recipe(
        executable,
        dir.path().to_owned(),
        vec!["--help".into()],
        env,
    )
    .unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    let mut run =
        NativeOwnedPtyRun::launch(recipe, NativePtySize::new(30, 100).unwrap(), end).unwrap();
    let mut output = run.take_reader().unwrap();
    let mut expected_key = key();
    expected_key.run = "PUBLIC_TEST_ONLY_CODEX_HELP".into();
    let target = expected_key.target();
    let journal_path = dir.path().join("journal.sqlite");
    let (mut writer, mut cap) = LocalWriter::start_terminal(
        Journal::open(&journal_path).unwrap(),
        run.take_writer().unwrap(),
        target,
        key().device,
        9,
        DigestKey::from_backend_bytes([0x5a; 32]),
    )
    .unwrap();
    let seen = Arc::new(AtomicBool::new(false));
    let ready = seen.clone();
    let da_replies = Arc::new(AtomicUsize::new(0));
    let reply_count = da_replies.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let result = (|| -> std::io::Result<Vec<u8>> {
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            let mut forward = true;
            loop {
                let n = output.read(&mut buf)?;
                if n == 0 {
                    return Ok(bytes);
                }
                if bytes.len() + n > 65536 {
                    return Err(std::io::Error::other("CLI_OUTPUT_CAP"));
                }
                bytes.extend_from_slice(&buf[..n]);
                if bytes.windows(b"Codex CLI".len()).any(|w| w == b"Codex CLI") {
                    ready.store(true, Ordering::Release);
                }
                if forward {
                    match cap.feed(buf[..n].to_vec()) {
                        Ok(p) => match p.wait(Duration::from_secs(1)) {
                            Ok(OutputResult::Processed { replies }) => {
                                reply_count.fetch_add(usize::from(replies), Ordering::Relaxed);
                            }
                            Ok(OutputResult::Rejected(Reject::Sealed)) => forward = false,
                            _ => return Err(std::io::Error::other("CLI_OUTPUT_GATE")),
                        },
                        Err(Reject::Sealed) => forward = false,
                        Err(_) => return Err(std::io::Error::other("CLI_OUTPUT_REFUSED")),
                    }
                }
            }
        })();
        let _ = tx.send(result);
    });
    let mut exited = None;
    while Instant::now() < end {
        exited = run.try_wait_root().unwrap();
        if exited.is_some() && seen.load(Ordering::Acquire) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    writer.seal();
    joined(&mut writer);
    let stopped = run.stop_root_until(end);
    drop(run);
    let bytes = rx
        .recv_timeout(end.saturating_duration_since(Instant::now()))
        .unwrap()
        .unwrap();
    while !reader.is_finished() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    reader.join().unwrap();
    assert!(Instant::now() < end);
    assert_eq!(exited.unwrap().exit_code, 0);
    assert_eq!(stopped.unwrap().exit_code, 0);
    assert!(seen.load(Ordering::Acquire));
    assert!(
        !codex_home.join("auth.json").exists(),
        "help must not enroll authentication"
    );
    assert_eq!(
        Journal::open(&journal_path)
            .unwrap()
            .query(&expected_key)
            .unwrap(),
        ReceiptLookup::Missing
    );
    eprintln!(
        "TEST_ONLY_CODEX_HELP bytes={} logical_da_replies_host_written={} root_exit=0 real_eof=true joined=true credentials_supplied=false private_home=true",
        bytes.len(),
        da_replies.load(Ordering::Relaxed)
    );
}
