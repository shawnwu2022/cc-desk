use crate::updater_http::{bytes, http_client};
use crate::updater_policy::validated_proxy;
use std::io::{Read, Write};
use std::net::TcpListener;
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
        connection.write_all(response.as_bytes()).unwrap();
        String::from_utf8(request).unwrap()
    });
    (address, thread)
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
