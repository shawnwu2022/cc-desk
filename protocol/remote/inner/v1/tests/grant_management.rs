use cc_desk_remote_record::{
    appbind::{self, CanonicalOrigin, ClientOffer, Context, HostOffer, RequestedRole},
    grant_management::{self as wire, Message},
};
fn offers() -> (HostOffer, ClientOffer) {
    let context = Context {
        host_endpoint_id: "01".repeat(32),
        client_endpoint_id: "02".repeat(32),
        host_key_epoch: "1".into(),
        client_key_epoch: "2".into(),
        activation: "03".repeat(32),
        incarnation: "4".into(),
        policy_epoch: "5".into(),
        issuer_epoch: "6".into(),
        origin: CanonicalOrigin::trusted("wss://relay.invalid/").unwrap(),
    };
    let caps = vec!["canonical-json".into(), "grant-management-v1".into()];
    (
        HostOffer {
            context: context.clone(),
            host_nonce: "04".repeat(32),
            profile: 1,
            major: 1,
            minors: vec![1],
            minimum_security: "1".into(),
            required_capabilities: caps.clone(),
        },
        ClientOffer {
            context,
            host_nonce: "04".repeat(32),
            client_nonce: "05".repeat(32),
            profile: 1,
            major: 1,
            minors: vec![1],
            minimum_security: "1".into(),
            required_capabilities: caps,
            requested_role: RequestedRole::ManagementOnly,
        },
    )
}
#[test]
fn exact_opt_in_capability_and_minor_are_required_without_legacy_downgrade() {
    let (h, c) = offers();
    assert_eq!(appbind::negotiate_grant(&h, &c).unwrap().minor, 1);
    assert!(appbind::negotiate(&h, &c).is_err());
    let mut old = c.clone();
    old.required_capabilities = vec!["canonical-json".into()];
    assert!(appbind::negotiate_grant(&h, &old).is_err());
    let (mut h, mut c) = (h, c);
    h.minors = vec![0];
    c.minors = vec![0];
    assert!(appbind::negotiate_grant(&h, &c).is_err());
}
#[test]
fn management_records_are_bounded_canonical_exact_shapes() {
    for m in [
        Message::Request {
            nonce: "01".repeat(32),
            reuse: false,
        },
        Message::Accepted {
            nonce: "01".repeat(32),
            binding: "02".repeat(32),
            grant_id: "03".repeat(32),
            digest: "04".repeat(32),
        },
    ] {
        let b = wire::encode(&m).unwrap();
        assert_eq!(wire::decode(&b).unwrap(), m);
        for n in 0..b.len() {
            assert!(wire::decode(&b[..n]).is_err());
        }
        let t = String::from_utf8(b).unwrap();
        assert!(wire::decode(t.replacen('{', "{\"unknown\":0,", 1).as_bytes()).is_err());
        assert!(wire::decode(t.replacen('{', "{\"nonce\":\"bad\",", 1).as_bytes()).is_err());
    }
    assert!(wire::decode(&vec![b' '; 16385]).is_err());
    assert!(wire::encode(&Message::Request {
        nonce: "00".repeat(32),
        reuse: true
    })
    .is_err());
    assert!(wire::decode(&[0xff]).is_err());
}
