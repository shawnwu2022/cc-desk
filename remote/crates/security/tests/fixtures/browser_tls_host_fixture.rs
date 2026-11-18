#[allow(dead_code)]
#[path = "../support/identities.rs"]
mod identities;
use cc_desk_remote_security::{
    LocalIdentity, P256Spki, PeerPin, Role, Unbound, accept, cancellation,
};
use std::{io::Write, sync::Arc, time::Duration};
fn client_spki() -> Vec<u8> {
    let hex = "3059301306072a8648ce3d020106082a8648ce3d030107034200046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}
fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "normal".into());
    assert!(["normal", "silent", "eof", "alpn", "cipher"].contains(&mode.as_str()));
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (certified, public) = identities::identity(false,false);
        println!("{}",serde_json::json!({"kind":"ready","port":listener.local_addr().unwrap().port(),"hostSpki":public.iter().map(|v|format!("{v:02x}")).collect::<String>()}));std::io::stdout().flush().unwrap();
        let (stream,_) = tokio::time::timeout(Duration::from_secs(3),listener.accept()).await.unwrap().unwrap();drop(listener);
        if mode=="eof" { drop(stream);println!("{{\"kind\":\"closed\"}}");return; }
        if mode=="silent" {tokio::time::sleep(Duration::from_millis(1500)).await;drop(stream);println!("{{\"kind\":\"closed\"}}");return;}
        let client = client_spki();
        if mode=="normal" {
            let (_handle,token)=cancellation();
            let result=accept(Unbound::new(stream),LocalIdentity::new(Role::Host,P256Spki::new(&public).unwrap(),certified.key.clone()).unwrap(),PeerPin::new(Role::Client,P256Spki::new(&client).unwrap()),Duration::from_secs(2),token).await;
            match result {Ok(pending)=>{let e=pending.evidence();println!("{}",serde_json::json!({"kind":"accepted","protocol":u16::from(e.protocol),"cipher":u16::from(e.cipher),"group":u16::from(e.group),"full":e.full_handshake,"rpk":e.raw_public_key,"alpn":e.alpn_matches}));std::io::stdout().flush().unwrap();tokio::time::sleep(Duration::from_millis(700)).await;drop(pending);},Err(error)=>println!("{}",serde_json::json!({"kind":"rejected","error":format!("{error:?}")}))}
        } else {
            // 仅负例互操作端配置畸形ALPN/不相交cipher，不更改产品config。
            let mut provider=rustls::crypto::ring::default_provider();provider.kx_groups=vec![rustls::crypto::ring::kx_group::X25519];
            if mode=="cipher" {provider.cipher_suites=vec![rustls::crypto::ring::cipher_suite::TLS13_AES_256_GCM_SHA384];}
            let mut config=rustls::ServerConfig::builder_with_provider(Arc::new(provider)).with_protocol_versions(&[&rustls::version::TLS13]).unwrap().with_client_cert_verifier(Arc::new(identities::Pin{expected:client,counts:Arc::default()})).with_cert_resolver(Arc::new(rustls::server::AlwaysResolvesServerRawPublicKeys::new(certified)));
            config.alpn_protocols=vec![if mode=="alpn" {b"TEST_ONLY-wrong".to_vec()}else{b"cc-desk-e2e/1".to_vec()}];
            let result=tokio_rustls::TlsAcceptor::from(Arc::new(config)).accept(stream).await;println!("{}",serde_json::json!({"kind":"mutantDone","rejected":result.is_err()}));
        }
        println!("{{\"kind\":\"closed\"}}");
    });
}
