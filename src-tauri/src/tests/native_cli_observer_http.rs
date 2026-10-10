use crate::observer_registry::{
    ObserverDelivery, ObserverRegistry, ObserverRun, MAX_OBSERVER_PAYLOAD,
};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Early authentication rejection may close a socket with unread request bytes,
// causing a trailing reset on macOS. Require the entire bounded HTTP response;
// EOF/reset before its declared body is complete is still a test failure.
fn read_framed_response(reader: &mut impl Read) -> std::io::Result<String> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        if response.len() >= 8192 {
            return Err(invalid());
        }
        let mut byte = [0];
        reader.read_exact(&mut byte)?;
        response.push(byte[0]);
    }
    let headers = std::str::from_utf8(&response).map_err(|_| invalid())?;
    let mut lines = headers[..headers.len() - 4].split("\r\n");
    let status = lines.next().ok_or_else(invalid)?;
    let mut parts = status.splitn(3, ' ');
    if parts.next() != Some("HTTP/1.1") {
        return Err(invalid());
    }
    let code = parts.next().ok_or_else(invalid)?;
    if code.len() != 3
        || !code.bytes().all(|byte| byte.is_ascii_digit())
        || !(100..=599).contains(&code.parse::<u16>().map_err(|_| invalid())?)
    {
        return Err(invalid());
    }
    let mut length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("content-length") {
            let value = value.trim();
            if length.is_some()
                || value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(invalid());
            }
            let size = value.parse::<usize>().map_err(|_| invalid())?;
            if size > 65536 {
                return Err(invalid());
            }
            length = Some(size);
        }
    }
    let length = length.ok_or_else(invalid)?;
    let header_length = response.len();
    response.resize(header_length + length, 0);
    reader.read_exact(&mut response[header_length..])?;
    String::from_utf8(response).map_err(|_| invalid())
}

#[test]
fn D13_Http_ResponseFramingRequiresCompleteResponse_003() {
    struct ResetAfterBytes(std::io::Cursor<Vec<u8>>);
    impl Read for ResetAfterBytes {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.0.position() == self.0.get_ref().len() as u64 {
                return Err(std::io::ErrorKind::ConnectionReset.into());
            }
            self.0.read(buffer)
        }
    }
    let complete = b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\n\r\n{\"ok\":false}";
    let mut reset = ResetAfterBytes(std::io::Cursor::new(complete.to_vec()));
    assert_eq!(
        read_framed_response(&mut reset).unwrap().as_bytes(),
        complete
    );
    for incomplete in [
        b"".as_slice(),
        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\n",
        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 12\r\n\r\n{\"ok\":",
    ] {
        assert!(read_framed_response(&mut std::io::Cursor::new(incomplete)).is_err());
        assert!(
            read_framed_response(&mut ResetAfterBytes(std::io::Cursor::new(
                incomplete.to_vec()
            )))
            .is_err()
        );
    }
    for malformed in [
        "HTTP/1.1 401 Unauthorized\r\n\r\n{}",
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nTransfer-Encoding: chunked\r\n\r\n{}",
        "not HTTP\r\nContent-Length: 2\r\n\r\n{}",
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: -1\r\n\r\n{}",
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 65537\r\n\r\n{}",
    ] {
        assert!(read_framed_response(&mut std::io::Cursor::new(malformed)).is_err());
    }
}

fn post(port: u16, path: &str, headers: &str, body: &[u8]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(6)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(6)))
        .unwrap();
    write!(stream, "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n", body.len()).unwrap();
    stream.write_all(body).unwrap();
    read_framed_response(&mut stream).unwrap()
}

#[test]
fn D13_Http_RealLoopbackAuthenticatesAndRejectsLegacyBypass_001() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let registry = Arc::new(ObserverRegistry::new());
    let lease = registry
        .lease(
            ObserverRun {
                run_id: "real-run".into(),
                generation: 4,
            },
            ObserverDelivery::native("main"),
            Arc::new(|| true),
        )
        .unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let output = received.clone();
    let app = crate::observer_http::router(
        registry,
        Arc::new(move |delivery, payload| {
            output.lock().unwrap().push((
                delivery.window_label.clone(),
                serde_json::to_value(payload).unwrap(),
            ));
            Ok(())
        }),
    );
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown, ended) = tokio::sync::oneshot::channel::<()>();
    runtime.spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = ended.await;
            })
            .await
            .unwrap();
    });
    let headers = format!("x-cc-desk-run: real-run\r\nx-cc-desk-generation: 4\r\nx-cc-desk-capability: {}\r\nx-cc-desk-event: event-one\r\nx-cc-desk-observer-source: claude-hook\r\n", lease.binding().capability);
    let body =
        br#"{"hook_event_name":"UserPromptSubmit","session_id":"sid","prompt":"fixture-private"}"#;
    assert!(post(port, "/observer", &headers, body).starts_with("HTTP/1.1 200"));
    assert!(post(port, "/observer", &headers, body).contains("duplicate"));
    assert!(post(port, "/observer", "", b"not even JSON").starts_with("HTTP/1.1 400"));
    let forged = headers.replace(
        &lease.binding().capability,
        "ffffffffffffffffffffffffffffffff",
    );
    assert!(post(port, "/observer", &forged, b"not JSON").starts_with("HTTP/1.1 401"));
    assert!(post(port, "/hook", "X-CC-Box-Session: real-run\r\n", body).starts_with("HTTP/1.1 403"));
    assert!(post(
        port,
        "/observer",
        &(headers.clone() + "Origin: http://evil.invalid\r\n"),
        body
    )
    .starts_with("HTTP/1.1 403"));
    let many = vec![b' '; MAX_OBSERVER_PAYLOAD + 1];
    assert!(post(port, "/observer", &headers, &many).starts_with("HTTP/1.1 413"));
    lease.revoke();
    assert!(post(port, "/observer", &headers, body).starts_with("HTTP/1.1 401"));
    let values = received.lock().unwrap();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].0, "main");
    assert!(!serde_json::to_string(&values[0].1)
        .unwrap()
        .contains("fixture-private"));
    drop(values);
    let _ = shutdown.send(());
}

#[test]
fn D13_Reporter_RealCurlAndScriptReachAuthenticatedHttp_002() {
    use std::process::{Command, Stdio};
    use std::time::Instant;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let registry = Arc::new(ObserverRegistry::new());
    let lease = registry
        .lease(
            ObserverRun {
                run_id: "script-run".into(),
                generation: 9,
            },
            ObserverDelivery::native("main"),
            Arc::new(|| true),
        )
        .unwrap();
    let received = Arc::new(Mutex::new(Vec::new()));
    let output = received.clone();
    let app = crate::observer_http::router(
        registry,
        Arc::new(move |_, payload| {
            output.lock().unwrap().push(payload);
            Ok(())
        }),
    );
    let listener = runtime
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let (shutdown, ended) = tokio::sync::oneshot::channel::<()>();
    runtime.spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = ended.await;
            })
            .await
            .unwrap();
    });
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| {
            path.join("src-tauri/plugin/scripts/report-hook.sh")
                .is_file()
        })
        .unwrap();
    let script = repo
        .join("src-tauri/plugin/scripts/report-hook.sh")
        .to_string_lossy()
        .replace('\\', "/");
    #[cfg(windows)]
    let bash = std::path::PathBuf::from(std::env::var_os("ProgramFiles").unwrap())
        .join("Git/bin/bash.exe");
    #[cfg(not(windows))]
    let bash = std::path::PathBuf::from("/bin/bash");
    for authorized in [true, false] {
        let mut child = Command::new(&bash)
            .arg(&script)
            .env("CC_BOX_HOOK_PORT", port.to_string())
            .env("CC_DESK_OBSERVER_RUN", "script-run")
            .env("CC_DESK_OBSERVER_GENERATION", "9")
            .env(
                "CC_DESK_OBSERVER_CAPABILITY",
                if authorized {
                    &lease.binding().capability
                } else {
                    "ffffffffffffffffffffffffffffffff"
                },
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(br#"{"hook_event_name":"UserPromptSubmit","session_id":"script-session","prompt":"synthetic-private"}"#).unwrap();
        let deadline = Instant::now() + Duration::from_secs(7);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("reporter deadline exceeded");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }
    let values = received.lock().unwrap();
    assert_eq!(
        values.len(),
        1,
        "real reporter must deliver only authenticated metadata"
    );
    assert_eq!(values[0].run_id.as_deref(), Some("script-run"));
    assert!(!serde_json::to_string(&values[0])
        .unwrap()
        .contains("synthetic-private"));
    drop(values);
    let _ = shutdown.send(());
}
