use super::*;

fn hex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap())
        .collect()
}
fn vector() -> serde_json::Value {
    serde_json::from_slice(include_bytes!(
        "../../remote/crates/security/tests/vectors/signatures/rfc6979-p256.json"
    ))
    .unwrap()
}
fn import_public_rfc6979_fixture(point: &[u8]) -> Result<NativeOwnedSoftwareKey, CryptoError> {
    if point.len() != 65 || point[0] != 4 {
        return Err(CryptoError::Key);
    }
    let mut approved_spki = [0; 91];
    approved_spki[..SPKI_HEAD.len()].copy_from_slice(SPKI_HEAD);
    approved_spki[SPKI_HEAD.len()..].copy_from_slice(&point[1..]);
    let mut owned = NativeOwnedSoftwareKey {
        provider: NCRYPT_PROV_HANDLE::default(),
        key: NCRYPT_KEY_HANDLE::default(),
        approved_purpose: SigningPurpose::Endpoint,
        approved_spki,
        worker: std::thread::current().id(),
        unknown: false,
        _not_send_or_sync: PhantomData,
        free_effect: None,
    };
    let imported = (|| {
        unsafe { NCryptOpenStorageProvider(&mut owned.provider, MS_KEY_STORAGE_PROVIDER, 0) }
            .map_err(|_| CryptoError::Native)?;
        eprintln!("TEST_ONLY_CNG_FIXED_SOFTWARE_PROVIDER_OPEN_OK");
        // Existing openly published RFC6979 A.2.5 scalar from identities.rs.
        // TEST_ONLY; no production import path, generated secret, name or store.
        let scalar = hex("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721");
        if point.len() != 65 || point[0] != 4 {
            return Err(CryptoError::Key);
        }
        let mut blob = Vec::with_capacity(104);
        blob.extend(BCRYPT_ECDSA_PRIVATE_P256_MAGIC.to_le_bytes());
        blob.extend(32u32.to_le_bytes());
        blob.extend(&point[1..]);
        blob.extend(&scalar);
        // Nonopaque blob, NULL import key/parameters/keyname. SILENT is the only
        // flag: no Machine/VBS/legacy/overwrite/no-validation/finalize flags.
        unsafe {
            NCryptImportKey(
                owned.provider,
                None,
                BCRYPT_ECCPRIVATE_BLOB,
                None,
                &mut owned.key,
                &blob,
                NCRYPT_SILENT_FLAG,
            )
        }
        .map_err(|_| CryptoError::Native)
    })();
    if let Err(error) = imported {
        if owned.try_retire().is_err() {
            owned.retain_unknown_for_test();
        }
        return Err(error);
    }
    eprintln!("TEST_ONLY_CNG_PUBLIC_RFC6979_UNNAMED_IMPORT_OK");
    Ok(owned)
}
#[test]
#[allow(non_snake_case)]
fn CngP256_DerMatchesExistingPublicVector_001() {
    let v = vector();
    let fixed = hex(v["signature_fixed"].as_str().unwrap());
    assert_eq!(
        fixed_to_der(&fixed).unwrap(),
        hex(v["signature_der"].as_str().unwrap())
    );
    let second = &v["second"];
    assert_eq!(
        fixed_to_der(&hex(second["signature_fixed"].as_str().unwrap())).unwrap(),
        hex(second["signature_der"].as_str().unwrap())
    );
    for invalid in [vec![], vec![0; 63], vec![0; 65]] {
        assert!(fixed_to_der(&invalid).is_err());
    }
}

#[test]
#[allow(non_snake_case)]
fn CngP256_PublicFixtureSignsAndExplicitlyRetiresOriginalHandles_002() {
    let worker = std::thread::spawn(|| -> Result<(), CryptoError> {
        let v = vector();
        let point = hex(v["public_key_sec1"].as_str().unwrap());
        let mut expected = hex("3059301306072a8648ce3d020106082a8648ce3d030107034200");
        expected.extend(&point);
        let mut owned = import_public_rfc6979_fixture(&point)?;
        let outcome = (|| -> Result<(), CryptoError> {
            let mut wrong = expected.clone();
            wrong[90] ^= 1;
            if CngP256Signer::admit(&mut owned, SigningPurpose::Endpoint, &wrong).is_ok() {
                return Err(CryptoError::Key);
            }
            let mut signer = CngP256Signer::admit(&mut owned, SigningPurpose::Endpoint, &expected)?;
            if signer.public_spki() != expected {
                return Err(CryptoError::Key);
            }
            if signer.sign_fixed(SigningPurpose::Issuer, b"sample") != Err(CryptoError::Purpose) {
                return Err(CryptoError::Purpose);
            }
            if signer.sign_fixed(SigningPurpose::Relay, b"sample") != Err(CryptoError::Purpose)
                || signer.sign_fixed(SigningPurpose::Endpoint, &vec![0; MAX_SIGN_BYTES + 1])
                    != Err(CryptoError::Bounds)
            {
                return Err(CryptoError::Purpose);
            }
            let fixed = signer.sign_fixed(SigningPurpose::Endpoint, b"sample")?;
            ring::signature::UnparsedPublicKey::new(
                &ring::signature::ECDSA_P256_SHA256_FIXED,
                &point,
            )
            .verify(b"sample", &fixed)
            .map_err(|_| CryptoError::Native)?;
            let der = signer.sign_der(SigningPurpose::Endpoint, b"sample")?;
            ring::signature::UnparsedPublicKey::new(
                &ring::signature::ECDSA_P256_SHA256_ASN1,
                &point,
            )
            .verify(b"sample", &der)
            .map_err(|_| CryptoError::Native)?;
            use coset::{iana, CoseSign1Builder, HeaderBuilder, TaggedCborSerializable};
            let signed = CoseSign1Builder::new()
                .protected(
                    HeaderBuilder::new()
                        .algorithm(iana::Algorithm::ES256)
                        .build(),
                )
                .payload(b"PUBLIC_TEST_ONLY_CNG_BOUNDARY".to_vec())
                .try_create_signature(b"PUBLIC_TEST_AAD", |message| {
                    signer.sign_fixed(SigningPurpose::Endpoint, message)
                })?
                .build();
            let encoded = signed.to_tagged_vec().map_err(|_| CryptoError::Native)?;
            let decoded =
                coset::CoseSign1::from_tagged_slice(&encoded).map_err(|_| CryptoError::Native)?;
            decoded.verify_signature(b"PUBLIC_TEST_AAD", |signature, message| {
                ring::signature::UnparsedPublicKey::new(
                    &ring::signature::ECDSA_P256_SHA256_FIXED,
                    &point,
                )
                .verify(message, signature)
                .map_err(|_| CryptoError::Native)
            })?;
            if decoded
                .verify_signature(b"WRONG_AAD", |signature, message| {
                    ring::signature::UnparsedPublicKey::new(
                        &ring::signature::ECDSA_P256_SHA256_FIXED,
                        &point,
                    )
                    .verify(message, signature)
                })
                .is_ok()
            {
                return Err(CryptoError::Native);
            }
            if CngP256Signer::admit(&mut owned, SigningPurpose::Issuer, &expected).is_ok() {
                return Err(CryptoError::Purpose);
            }
            Ok(())
        })();
        if owned.try_retire().is_err() {
            owned.retain_unknown_for_test();
        }
        if !owned.explicitly_retired() {
            return Err(CryptoError::Unknown);
        }
        eprintln!("TEST_ONLY_CNG_ORIGINAL_KEY_AND_PROVIDER_EXPLICIT_FREE_OK");
        if CngP256Signer::admit(&mut owned, SigningPurpose::Endpoint, &expected).is_ok() {
            return Err(CryptoError::Closed);
        }
        outcome
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut reported = false;
    while !worker.is_finished() {
        if !reported && std::time::Instant::now() >= deadline {
            eprintln!("TEST_ONLY_CNG_PENDING_UNKNOWN: original worker/handles retained; no native cancellation claimed");
            reported = true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let result = worker.join().expect("original native worker must join");
    eprintln!("TEST_ONLY_CNG_ORIGINAL_WORKER_JOINED");
    assert!(
        !reported,
        "native worker exceeded observation budget; no hard deadline is claimed"
    );
    assert_eq!(result, Ok(()));
}

#[test]
#[allow(non_snake_case)]
fn CngP256_PublicBlobRejectsWrongCurveWidthAndTail_003() {
    let v = vector();
    let point = hex(v["public_key_sec1"].as_str().unwrap());
    let mut blob = Vec::new();
    blob.extend(BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend(32u32.to_le_bytes());
    blob.extend(&point[1..]);
    let spki = spki_from_blob(&blob).unwrap();
    assert_eq!(&spki[26..], point);
    let mut bad = blob.clone();
    bad[0..4].copy_from_slice(&BCRYPT_ECDH_PUBLIC_P256_MAGIC.to_le_bytes());
    assert!(spki_from_blob(&bad).is_err());
    let mut bad = blob.clone();
    bad[4..8].copy_from_slice(&31u32.to_le_bytes());
    assert!(spki_from_blob(&bad).is_err());
    blob.push(0);
    assert!(spki_from_blob(&blob).is_err());
    assert!(spki_from_blob(&[]).is_err());
}

#[test]
#[allow(non_snake_case)]
fn CngP256_FreeFailureRetainsOriginalFactsAndNeverRetries_004() {
    let calls = Rc::new(std::cell::Cell::new(0));
    let effect_calls = calls.clone();
    // Synthetic values only: this fixture owns NO actual CNG objects. The
    // permanent injected effect intercepts every call, including any regression.
    let mut owned = NativeOwnedSoftwareKey {
        provider: NCRYPT_PROV_HANDLE(2),
        key: NCRYPT_KEY_HANDLE(1),
        approved_purpose: SigningPurpose::Endpoint,
        approved_spki: [0; 91],
        worker: std::thread::current().id(),
        unknown: false,
        _not_send_or_sync: PhantomData,
        free_effect: Some(Box::new(move |_| {
            effect_calls.set(effect_calls.get() + 1);
            Err(CryptoError::Unknown)
        })),
    };
    assert_eq!(owned.try_retire(), Err(CryptoError::Unknown));
    assert_eq!(owned.try_retire(), Err(CryptoError::Unknown));
    assert!(!owned.explicitly_retired());
    assert_eq!((owned.key.0, owned.provider.0), (1, 2));
    assert_eq!(calls.get(), 1);
    assert_eq!(format!("{owned:?}"), "NativeOwnedSoftwareKey(<redacted>)");
    // Drop cannot count as positive retirement and cannot reissue an Unknown.
    drop(owned);
    assert_eq!(calls.get(), 1);
}
