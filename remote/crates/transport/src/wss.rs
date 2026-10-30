//! Outer WebPKI transport only. Endpoint RPK/AppBind authorization remains separate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectError {
    Origin,
    Limit,
    Deadline,
    Transport,
    Admission,
    Signer,
}
pub async fn connect(
    origin: &str,
    address: std::net::SocketAddr,
    budget: std::time::Duration,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    ConnectError,
> {
    if budget.is_zero() || budget > crate::MAX_IO_BUDGET {
        return Err(ConnectError::Limit);
    }
    if origin.len() > 2048 {
        return Err(ConnectError::Origin);
    }
    let url = url::Url::parse(origin).map_err(|_| ConnectError::Origin)?;
    if url.scheme() != "wss"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ConnectError::Origin);
    }
    let host = match url.host().ok_or(ConnectError::Origin)? {
        url::Host::Domain(s) => s.to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
    let port = url
        .port_or_known_default()
        .filter(|p| *p != 0)
        .ok_or(ConnectError::Origin)?;
    let name = rustls::pki_types::ServerName::try_from(host).map_err(|_| ConnectError::Origin)?;
    if address.port() != port {
        return Err(ConnectError::Origin);
    }
    let deadline = tokio::time::Instant::now()
        .checked_add(budget)
        .ok_or(ConnectError::Limit)?;
    crate::deadline::bounded(deadline, async {
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .map_err(|_| ConnectError::Transport)?;
        upgrade(url.as_str(), name, socket, config()).await
    })
    .await
    .map_err(|_| ConnectError::Deadline)?
}

fn config() -> std::sync::Arc<rustls::ClientConfig> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    with_roots(roots)
}
fn with_roots(roots: rustls::RootCertStore) -> std::sync::Arc<rustls::ClientConfig> {
    let mut c = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .expect("fixed provider")
    .with_root_certificates(roots)
    .with_no_client_auth();
    c.alpn_protocols = vec![b"http/1.1".to_vec()];
    c.resumption = rustls::client::Resumption::disabled();
    c.enable_early_data = false;
    std::sync::Arc::new(c)
}
async fn upgrade(
    origin: &str,
    name: rustls::pki_types::ServerName<'static>,
    socket: tokio::net::TcpStream,
    c: std::sync::Arc<rustls::ClientConfig>,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    ConnectError,
> {
    let tls = tokio_rustls::TlsConnector::from(c)
        .connect(name, socket)
        .await
        .map_err(|_| ConnectError::Transport)?;
    let (ws, _) =
        tokio_tungstenite::client_async_with_config(origin, tls, Some(crate::websocket_config()))
            .await
            .map_err(|_| ConnectError::Transport)?;
    Ok(ws)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::{io::Cursor, sync::Arc};
    fn test_roots() -> rustls::RootCertStore {
        let certs = rustls_pemfile::certs(&mut Cursor::new(include_bytes!(
            "../tests/fixtures/root.pem"
        )))
        .unwrap();
        let mut roots = rustls::RootCertStore::empty();
        for cert in certs {
            roots.add(CertificateDer::from(cert)).unwrap();
        }
        roots
    }
    async fn trial(trust_test_root: bool, host: &str, deny_http: bool) -> Result<(), ConnectError> {
        let certs = rustls_pemfile::certs(&mut Cursor::new(include_bytes!(
            "../tests/fixtures/chain.pem"
        )))
        .unwrap()
        .into_iter()
        .map(CertificateDer::from)
        .collect();
        let key = rustls_pemfile::pkcs8_private_keys(&mut Cursor::new(include_bytes!(
            "../tests/fixtures/end.key"
        )))
        .unwrap()
        .remove(0);
        let mut sc = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .unwrap();
        sc.alpn_protocols = vec![b"http/1.1".to_vec()];
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = l.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, peer) = l.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            if let Ok(mut tls) = tokio_rustls::TlsAcceptor::from(Arc::new(sc))
                .accept(socket)
                .await
            {
                if deny_http {
                    use tokio::io::AsyncWriteExt;
                    let _ = tls
                        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
                        .await;
                } else if let Ok(mut ws) = tokio_tungstenite::accept_async_with_config(
                    tls,
                    Some(crate::websocket_config()),
                )
                .await
                {
                    let _ = ws
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            b"TEST_ONLY".to_vec().into(),
                        ))
                        .await;
                }
            }
        });
        let c = if trust_test_root {
            with_roots(test_roots())
        } else {
            config()
        };
        let socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            let mut ws = upgrade(
                &format!("wss://{host}/"),
                rustls::pki_types::ServerName::try_from(host.to_owned()).unwrap(),
                socket,
                c,
            )
            .await?;
            assert_eq!(
                ws.get_ref().get_ref().1.protocol_version(),
                Some(rustls::ProtocolVersion::TLSv1_3)
            );
            let msg = ws
                .next()
                .await
                .ok_or(ConnectError::Transport)?
                .map_err(|_| ConnectError::Transport)?;
            assert_eq!(msg.into_data().as_ref(), b"TEST_ONLY");
            Ok(())
        })
        .await
        .expect("bounded test");
        server.abort();
        let _ = server.await;
        result
    }
    #[tokio::test]
    async fn test_only_valid_chain_name_and_http_upgrade_really_work() {
        assert_eq!(trial(true, "foobar.com", false).await, Ok(()));
    }
    #[tokio::test]
    async fn production_roots_refuse_public_test_ca() {
        assert_eq!(
            trial(false, "foobar.com", false).await,
            Err(ConnectError::Transport)
        );
    }
    #[tokio::test]
    async fn trusted_test_ca_does_not_bypass_hostname() {
        assert_eq!(
            trial(true, "wrong.invalid", false).await,
            Err(ConnectError::Transport)
        );
    }
    #[tokio::test]
    async fn tls_success_does_not_accept_denied_http_upgrade() {
        assert_eq!(
            trial(true, "foobar.com", true).await,
            Err(ConnectError::Transport)
        );
    }
}

#[path = "wss_admission.rs"]
mod admission;
#[cfg(any(test, feature = "test-only-loopback"))]
pub use admission::test_only_connect_admitted_loopback;
pub use admission::{
    RelayAdmissionIdentity, RelayRole, RelaySigner, SignError, SigningRequest, connect_admitted,
};
