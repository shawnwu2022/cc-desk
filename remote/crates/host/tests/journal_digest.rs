//! Public TEST_ONLY keys; never use these bytes as access or production digest keys.
use cc_desk_remote_host::{
    journal::OperationKey,
    journal_digest::{DigestError, DigestKey},
};

fn operation() -> OperationKey {
    OperationKey {
        host: [1; 32],
        session: [2; 16],
        run: "PUBLIC_TEST_ONLY".into(),
        generation: 3,
        incarnation: 4,
        device: [5; 32],
        operation: [6; 16],
        sequence: 7,
    }
}

#[test]
fn digest_binds_every_target_identity_operation_epoch_and_payload_field() {
    let key = DigestKey::from_backend_bytes([0x5a; 32]);
    let base = operation();
    let original = key.input(&base, 8, b"PUBLIC_TEST_ONLY\n").unwrap().0;
    let mut variants = Vec::new();
    let mut next = base.clone();
    next.host[0] ^= 1;
    variants.push(next);
    let mut next = base.clone();
    next.session[0] ^= 1;
    variants.push(next);
    let mut next = base.clone();
    next.run.push('x');
    variants.push(next);
    let mut next = base.clone();
    next.generation += 1;
    variants.push(next);
    let mut next = base.clone();
    next.incarnation += 1;
    variants.push(next);
    let mut next = base.clone();
    next.device[0] ^= 1;
    variants.push(next);
    let mut next = base.clone();
    next.operation[0] ^= 1;
    variants.push(next);
    let mut next = base.clone();
    next.sequence += 1;
    variants.push(next);
    for (field, changed) in variants.iter().enumerate() {
        assert_ne!(
            key.input(changed, 8, b"PUBLIC_TEST_ONLY\n").unwrap().0,
            original,
            "unbound field {field}"
        );
    }
    assert_ne!(
        key.input(&base, 9, b"PUBLIC_TEST_ONLY\n").unwrap().0,
        original
    );
    assert_ne!(
        key.input(&base, 8, b"PUBLIC_TEST_ONLY!\n").unwrap().0,
        original
    );
    assert_ne!(
        DigestKey::from_backend_bytes([0x5b; 32])
            .input(&base, 8, b"PUBLIC_TEST_ONLY\n")
            .unwrap()
            .0,
        original
    );
    assert_eq!(
        key.input(&base, 8, b"PUBLIC_TEST_ONLY\n").unwrap().0,
        original
    );
}

#[test]
fn digest_refuses_unbounded_or_empty_variable_fields() {
    let key = DigestKey::from_backend_bytes([0x5a; 32]);
    let base = operation();
    for bytes in [Vec::new(), vec![0; 16385]] {
        assert!(matches!(
            key.input(&base, 8, &bytes),
            Err(DigestError::Bounds)
        ));
    }
    for run in [String::new(), "x".repeat(129)] {
        let mut next = base.clone();
        next.run = run;
        assert!(matches!(
            key.input(&next, 8, b"x"),
            Err(DigestError::Bounds)
        ));
    }
    let mut edge = base;
    edge.run = "x".repeat(128);
    assert!(key.input(&edge, 8, &vec![0; 16384]).is_ok());
}
