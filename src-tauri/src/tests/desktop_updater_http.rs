use crate::updater_http::{bytes, http_client, probe_proxy, validate_probe_manifest};
use crate::updater_policy::validated_proxy;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn proxy_server(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let thread = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut connection = loop {
            match listener.accept() {
                Ok((connection, _)) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "update did not contact its configured proxy"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fixture accept failed: {error}"),
            }
        };
        let request = read_proxy_request(&mut connection);
        connection.write_all(response.as_bytes()).unwrap();
        request
    });
    (address, thread)
}

fn read_proxy_request(connection: &mut TcpStream) -> String {
    // Winsock accept inherits the listener's nonblocking mode. A read timeout
    // bounds blocking I/O; it does not restore blocking mode on that stream.
    connection.set_nonblocking(false).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    let mut chunk = [0; 512];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
        let count = connection.read(&mut chunk).unwrap();
        assert!(count > 0 && request.len() < 16 * 1024);
        request.extend_from_slice(&chunk[..count]);
    }
    String::from_utf8(request).unwrap()
}

#[test]
fn proxy_fixture_waits_for_delayed_and_fragmented_headers() {
    const PREFIX: &[u8] =
        b"GET http://update.invalid/readiness HTTP/1.1\r\nHost: update.invalid\r\n";
    const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (mut connection, _) = listener.accept().unwrap();
    // Winsock inherits this mode from the listener. Force the same boundary on
    // every platform, even where accept creates a blocking stream by default.
    connection.set_nonblocking(true).unwrap();
    let writer = std::thread::spawn(move || {
        client.set_nonblocking(true).unwrap();
        let mut response = [0; 1];
        for fragment in [PREFIX, b"\r".as_slice(), b"\n".as_slice()] {
            // Hold each fragment while checking readiness without a blocking
            // receive timeout, which leaves Winsock connections indeterminate.
            let held_until = Instant::now() + Duration::from_millis(100);
            loop {
                assert!(
                    matches!(client.peek(&mut response), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                    "fixture closed or responded before complete request headers"
                );
                if Instant::now() >= held_until {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            client.set_nonblocking(false).unwrap();
            client.write_all(fragment).unwrap();
            client.set_nonblocking(true).unwrap();
        }
        client.set_nonblocking(false).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).unwrap();
        assert_eq!(response, RESPONSE);
    });
    let request = read_proxy_request(&mut connection);
    connection.write_all(RESPONSE).unwrap();
    drop(connection);
    writer.join().unwrap();
    assert_eq!(request.as_bytes(), [PREFIX, b"\r\n"].concat());
}

#[tokio::test]
async fn explicit_proxy_is_used_for_the_real_http_request() {
    let (address, server) =
        proxy_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    let proxy = validated_proxy(Some(&address)).unwrap().unwrap();
    let data = bytes(
        &http_client(Some(&proxy)).unwrap(),
        "http://update.invalid/manifest",
        16,
    )
    .await
    .unwrap();
    assert_eq!(data, b"{}");
    let request = server.join().unwrap();
    assert!(request.starts_with("GET http://update.invalid/manifest HTTP/1.1\r\n"));
}

#[test]
fn inherited_proxy_is_used_by_the_update_client() {
    if std::env::var_os("CC_DESK_UPDATER_PROXY_FIXTURE_CHILD").is_some() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let data = runtime
            .block_on(bytes(
                &http_client(None).unwrap(),
                "http://update.invalid/inherited",
                16,
            ))
            .unwrap();
        assert_eq!(data, b"{}");
        return;
    }
    // The child owns its environment. Other ordinary tests keep their own proxy.
    let (address, server) =
        proxy_server("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "inherited_proxy_is_used_by_the_update_client",
            "--test-threads=1",
        ])
        .env("CC_DESK_UPDATER_PROXY_FIXTURE_CHILD", "1")
        .env("NO_PROXY", "")
        .env("no_proxy", "");
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env(name, &address);
    }
    assert!(command.output().unwrap().status.success());
    assert!(server
        .join()
        .unwrap()
        .starts_with("GET http://update.invalid/inherited HTTP/1.1\r\n"));
}

#[tokio::test]
async fn http_limits_and_rejections_return_only_safe_codes() {
    for (response, maximum, code) in [
        (
            "HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            16,
            "UPDATER_RATE_LIMITED",
        ),
        (
            "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            16,
            "UPDATER_HTTP_REJECTED",
        ),
        (
            "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nlong",
            2,
            "UPDATER_MANIFEST_INVALID",
        ),
    ] {
        let (address, server) = proxy_server(response);
        let proxy = validated_proxy(Some(&address)).unwrap().unwrap();
        let error = bytes(
            &http_client(Some(&proxy)).unwrap(),
            "http://update.invalid/bounded",
            maximum,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.stage, "provenance");
        server.join().unwrap();
    }
}

// 检测仅连接固定官方更新源，不使用用户提供的目标 URL。
#[tokio::test]
#[allow(non_snake_case)]
async fn ProxyProbe_OfficialTarget_001() {
    let (address, server) =
        proxy_server("HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let error = probe_proxy(Some(&address)).await.unwrap_err();
    assert_eq!(error.code, "UPDATER_REQUEST_FAILED");
    let request = server.join().unwrap();
    assert!(request.starts_with("CONNECT github.com:443 HTTP/1.1\r\n"));
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::json!({
            "code": "UPDATER_REQUEST_FAILED", "stage": "provenance"
        })
    );
}

// 不能把成功 HTTP 状态下的 HTML 或空对象报告为官方清单可达。
#[test]
#[allow(non_snake_case)]
fn ProxyProbe_RequiresManifest_002() {
    assert!(
        validate_probe_manifest(br#"{"version":"0.18.3","platforms":{"windows-x86_64":{}}}"#)
            .is_ok()
    );
    for body in [
        b"<html>login</html>".as_slice(),
        b"{}",
        b"null",
        br#"{"version":"0.18.3"}"#,
    ] {
        let error = validate_probe_manifest(body).unwrap_err();
        assert_eq!(error.code, "UPDATER_MANIFEST_INVALID");
    }
}

// 数据体超时保留明确超时代码，而不泄露响应地址或原始传输错误。
#[tokio::test]
#[allow(non_snake_case)]
async fn ProxyProbe_BodyTimeout_003() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut connection = loop {
            match listener.accept() {
                Ok((connection, _)) => break connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "body-timeout fixture never received a connection"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("body-timeout fixture accept failed: {error}"),
            }
        };
        read_proxy_request(&mut connection);
        connection
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
            .unwrap();
        std::thread::sleep(Duration::from_millis(150));
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(50))
        .build()
        .unwrap();
    let error = bytes(&client, &address, 16).await.unwrap_err();
    assert_eq!(error.code, "UPDATER_TIMEOUT");
    server.join().unwrap();
}

// 共享更新客户端拒绝跳转到官方之外的站点，即使源返回可读重定向。
#[tokio::test]
#[allow(non_snake_case)]
async fn ProxyProbe_RejectForeignRedirect_004() {
    for response in [
        "HTTP/1.1 302 Found\r\nLocation: https://outside.invalid/private?TOKEN=secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 302 Found\r\nLocation: https://TOKEN:secret@github.com/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 302 Found\r\nLocation: https://github.com:8443/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        "HTTP/1.1 302 Found\r\nLocation: http://github.com/private\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ] {
        let (address, server) = proxy_server(response);
        let proxy = validated_proxy(Some(&address)).unwrap().unwrap();
        let error = http_client(Some(&proxy)).unwrap()
            .get("http://update.invalid/manifest").send().await.unwrap_err();
        // Redirect refusal must happen before another connection, rather than
        // succeeding in this test merely because the fixture proxy has closed.
        assert!(error.is_redirect());
        server.join().unwrap();
    }
}

// 无效配置在网络客户端创建前拒绝，返回值不包含认证信息或 URL。
#[tokio::test]
#[allow(non_snake_case)]
async fn ProxyProbe_InvalidDraft_005() {
    for value in [
        "socks5://user:secret@localhost:1080",
        "http://user:secret@localhost/private?TOKEN=secret",
    ] {
        let error = probe_proxy(Some(value)).await.unwrap_err();
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "code": "UPDATER_PROXY_INVALID", "stage": "proxy"
            })
        );
    }
}
