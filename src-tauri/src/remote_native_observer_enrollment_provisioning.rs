use super::super::{hex, ApprovalMachine, DIALOG_ACTIVE};
use super::*;
use crate::{
    cli::{document::DocumentBinding, launch_service::NativeRun, snapshot::CallerIdentity},
    native_crypto::provisioning::PendingPair,
    remote_native_pairing_store::{Backend, CurrentUserDpapi, Sealer, StoreError, Transaction},
    version_history::windows::pairing_store::WindowsPairingBackend,
};
use std::path::Path;
use std::sync::atomic::Ordering;

fn setup_error() -> SafeError {
    error("REMOTE_PAIRING_SETUP_FAILED")
}
pub(super) fn read_profile(dir: &Path) -> Result<Option<Vec<u8>>, StoreError> {
    let backend = match WindowsPairingBackend::open_observer_setup(dir, false) {
        Ok(backend) => backend,
        Err(StoreError::Missing) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut tx = backend.begin()?;
    let Some(sealed) = tx.read()? else {
        return Ok(None);
    };
    let bytes = CurrentUserDpapi.unseal(&sealed)?;
    if bytes.len() > 4096 {
        return Err(StoreError::Invalid);
    }
    Ok(Some(bytes))
}
fn absent_legacy(dir: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(dir.join("native-observer-relay-v1.json")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(StoreError::Conflict),
        Err(_) => Err(StoreError::Io),
    }
}
pub(super) async fn provision(
    app: &tauri::AppHandle,
    window: &tauri::Window,
    document: Arc<DocumentBinding<NativeRun>>,
    caller: CallerIdentity,
    dir: PathBuf,
    endpoint: &str,
) -> Result<Reply, SafeError> {
    let origin = CanonicalOrigin::trusted(endpoint).map_err(|_| setup_error())?;
    if origin.as_str().len() > 512 {
        return Err(setup_error());
    }
    document.check_admitted(&caller)?;
    DIALOG_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| setup_error())?;
    let prompt=format!("Create the local connection setup for {}? This does not start sharing. The relay administrator must authorize this computer separately.",origin.as_str());
    let machine = ApprovalMachine::show(
        app,
        window,
        origin,
        prompt,
        "Create local connection setup",
        Instant::now() + Duration::from_secs(30),
    );
    let permit = loop {
        document.check_admitted(&caller)?;
        if let Some(permit) = machine.take() {
            break permit;
        }
        if !machine.waiting() {
            return Ok(Reply::Declined);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    tokio::task::spawn_blocking(move || -> Result<Reply, SafeError> {
        permit
            .consume(|origin, end| {
                let authority = || {
                    if Instant::now() >= end
                        || machine.lease.is_revoked()
                        || document.check_admitted(&caller).is_err()
                    {
                        return Err(StoreError::Unauthorized);
                    }
                    Ok(())
                };
                let profile = save(&dir, origin, authority).map_err(|_| setup_error())?;
                authority().map_err(|_| setup_error())?;
                Ok(Reply::PublicRegistration {
                    metadata: super::super::setup::PublicRegistrationMetadata::new_public(
                        profile.relay_id,
                        &decode::<91>(&profile.relay_spki).map_err(|_| setup_error())?,
                    ),
                })
            })
            .ok_or_else(setup_error)?
    })
    .await
    .map_err(|_| setup_error())?
}
fn vacant(tx: &mut impl Transaction) -> Result<(), StoreError> {
    if tx.read()?.is_some() {
        Err(StoreError::Conflict)
    } else {
        Ok(())
    }
}
fn save(
    dir: &Path,
    origin: CanonicalOrigin,
    mut authorize: impl FnMut() -> Result<(), StoreError>,
) -> Result<FileProfile, StoreError> {
    authorize()?;
    absent_legacy(dir)?;
    fs::create_dir_all(dir).map_err(|_| StoreError::Io)?;
    let open = |create| WindowsPairingBackend::open_observer_setup(dir, create);
    let backend = match open(false) {
        Ok(b) => b,
        Err(StoreError::Missing) => open(true)?,
        Err(e) => return Err(e),
    };
    let mut tx = backend.begin()?;
    vacant(&mut tx)?;
    authorize()?;
    absent_legacy(dir)?;
    let profile_id = uuid::Uuid::new_v4().simple().to_string();
    let endpoint_name = format!("ccdesk-native-v1-{profile_id}-endpoint");
    let relay_name = format!("ccdesk-native-v1-{profile_id}-relay");
    let (mut custody, public) =
        PendingPair::create([&endpoint_name, &relay_name]).map_err(|_| StoreError::Unauthorized)?;
    let profile = FileProfile {
        version: 1,
        profile_id,
        profile_revision: 1,
        endpoint_name,
        endpoint_spki: hex(&public.endpoint),
        relay_name,
        relay_spki: hex(&public.relay),
        relay_id: uuid::Uuid::new_v4().simple().to_string(),
        relay_origin: origin,
        socket_address: None,
        host_key_epoch: 1,
        client_key_epoch: 1,
        policy_epoch: 1,
        issuer_epoch: 1,
    };
    let bytes = serde_json::to_vec(&profile).map_err(|_| StoreError::Invalid)?;
    if bytes.len() > 4096 {
        return Err(StoreError::Invalid);
    }
    let sealed = CurrentUserDpapi.seal(&bytes)?;
    let result = tx.commit(&sealed, false, &mut || {
        authorize()?;
        absent_legacy(dir)
    });
    // Unknown may be post-publication: retaining new unused names is safer
    // than breaking an already published profile; existing keys never change.
    if result.is_ok() || result == Err(StoreError::Unknown) {
        custody.retain();
    }
    result?;
    Ok(profile)
}
pub(super) async fn resolve(origin: &CanonicalOrigin) -> Result<SocketAddr, SafeError> {
    let parsed = url::Url::parse(origin.as_str()).map_err(|_| setup_error())?;
    let hostname = parsed.host_str().ok_or_else(setup_error)?;
    let port = parsed.port_or_known_default().ok_or_else(setup_error)?;
    let address = if let Ok(ip) = hostname
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
    {
        SocketAddr::new(ip, port)
    } else {
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((hostname, port)),
        )
        .await
        .map_err(|_| setup_error())?
        .map_err(|_| setup_error())?
        .find(|a| !a.ip().is_unspecified())
        .ok_or_else(setup_error)?
    };
    if address.ip().is_unspecified() {
        return Err(setup_error());
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(non_snake_case)]
    fn NativeSetup_DenialCannotWriteAndPublicRegistrationIsMinimal_002() {
        let root = tempfile::tempdir().unwrap();
        let unused = root.path().join("unused");
        let origin = CanonicalOrigin::trusted("wss://example.invalid/enrollment").unwrap();
        let now = Instant::now();
        let pending = ApprovalMachine::pending(origin.clone(), now, now + Duration::from_secs(30));
        assert!(pending.take().is_none());
        pending.complete(false, now);
        assert!(pending.take().is_none());
        assert!(matches!(
            save(&unused, origin, || Err(StoreError::Unauthorized)),
            Err(StoreError::Unauthorized)
        ));
        assert!(!unused.exists());
        let public = super::super::super::setup::PublicRegistrationMetadata::new_public(
            "11".repeat(16),
            &[2; 91],
        );
        let value = serde_json::to_value(public).unwrap();
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            ["relayId", "relayPublicKeySec1", "role", "version"]
                .into_iter()
                .collect()
        );
    }

    #[test]
    #[allow(non_snake_case)]
    fn NativeSetup_CreateOnceProtectedWindowsStoreDoesNotReplace_003() {
        let dir = tempfile::tempdir().unwrap();
        let backend = WindowsPairingBackend::open_observer_setup(dir.path(), true).unwrap();
        {
            let mut tx = backend.begin().unwrap();
            vacant(&mut tx).unwrap();
            tx.commit(b"test-only-first", false, &mut || Ok(()))
                .unwrap();
        }
        {
            let mut tx = backend.begin().unwrap();
            assert_eq!(vacant(&mut tx), Err(StoreError::Conflict));
            assert_eq!(
                tx.commit(b"test-only-forbidden", false, &mut || Ok(())),
                Err(StoreError::Invalid)
            );
        }
        let mut tx = backend.begin().unwrap();
        assert_eq!(tx.read().unwrap().unwrap(), b"test-only-first");
    }
}
