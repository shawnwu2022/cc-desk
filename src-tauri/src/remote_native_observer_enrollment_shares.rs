use super::{store::RelayPair, *};
use cc_desk_remote_transport::wss::ShareSelectionKey;

const CATALOG: &[u8] = b"cc-desk.native-pairing.shares.v1\0";
const READY: &[u8] = b"cc-desk.native-pairing.share-ready.v1\0";
const TTL: Duration = Duration::from_secs(55);

pub(crate) struct Target {
    pub(crate) run: NativeRunAdmission,
    pub(crate) label: String,
}
struct Offer {
    target: Target,
    scope: SessionScope,
    id: [u8; 16],
}
struct Frozen {
    origin: CanonicalOrigin,
    pair: RelayPair,
    host: [u8; 91],
    offers: Vec<Offer>,
}
pub(crate) struct Approval(Arc<ApprovalMachine<Frozen>>);
pub(crate) struct Live {
    frozen: Arc<Frozen>,
    lease: Arc<ApprovalLease>,
    end: Instant,
    pub(crate) wire: String,
    pub(crate) digest: [u8; 32],
    expires: u64,
    pub(crate) authority_end: Instant,
}
impl Frozen {
    fn check(&self) -> Result<()> {
        for offer in &self.offers {
            if offer.target.run.scope().map_err(|_| Error::Authority)? != offer.scope {
                return Err(Error::Authority);
            }
        }
        Ok(())
    }
}
impl Approval {
    pub(crate) fn show<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        window: &tauri::Window<R>,
        origin: CanonicalOrigin,
        pair: RelayPair,
        host: [u8; 91],
        targets: Vec<Target>,
    ) -> Result<Self> {
        if targets.is_empty() || targets.len() > 4 || !url_shape(&origin) {
            return Err(Error::Invalid);
        }
        P256Spki::new(&host).map_err(|_| Error::Authority)?;
        let mut offers: Vec<Offer> = Vec::new();
        for target in targets {
            let scope = target.run.scope().map_err(|_| Error::Authority)?;
            encode_scope(&scope).map_err(|_| Error::Authority)?;
            let label = &target.label;
            if label.is_empty()
                || label.trim() != label
                || label.len() > 48
                || !label
                    .chars()
                    .all(|c| c.is_alphanumeric() || " -_.#()".contains(c))
                || offers.iter().any(|old| old.scope == scope)
            {
                return Err(Error::Invalid);
            }
            offers.push(Offer {
                target,
                scope,
                id: *uuid::Uuid::new_v4().as_bytes(),
            });
        }
        let text = format!(
            "Share these sessions with this phone for viewing only?\n\nPhone: {}\n\n{}",
            hex(&pair.phone_id),
            offers
                .iter()
                .map(|o| format!("{} · {}", o.target.label, o.scope.run))
                .collect::<Vec<_>>()
                .join("\n")
        );
        DIALOG_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::Busy)?;
        Ok(Self(ApprovalMachine::show(
            app,
            window,
            Frozen {
                origin,
                pair,
                host,
                offers,
            },
            text,
            "Share selected sessions",
            Instant::now() + Duration::from_secs(300),
        )))
    }
    pub(crate) fn pending(&self) -> bool {
        self.0.waiting()
    }
    pub(crate) fn approved(&self) -> bool {
        let state = self.0.state.lock();
        !state.closed
            && state.description.is_some()
            && state.approved_end.is_some_and(|end| Instant::now() < end)
    }
    pub(crate) async fn sign(&self, key: &KeyRequestHandle) -> Result<Option<Live>> {
        let Some(permit) = self.0.take() else {
            return Ok(None);
        };
        let (frozen, end) = permit
            .consume(|frozen, end| (frozen, end))
            .ok_or(Error::Authority)?;
        make_live(Arc::new(frozen), self.0.lease.clone(), end, key)
            .await
            .map(Some)
    }
}
async fn make_live(
    frozen: Arc<Frozen>,
    lease: Arc<ApprovalLease>,
    authority_end: Instant,
    key: &KeyRequestHandle,
) -> Result<Live> {
    frozen.check()?;
    let end = authority_end.min(Instant::now() + TTL);
    let expires = unix_ms()?
        .checked_add(end.saturating_duration_since(Instant::now()).as_millis() as u64)
        .ok_or(Error::Expired)?;
    let offers=frozen.offers.iter().map(|o|serde_json::json!({
            "offerId":hex(&o.id),"session":o.scope.session,"run":o.scope.run,"generation":o.scope.generation,"label":o.target.label
        })).collect::<Vec<_>>();
    let payload=serde_json::to_vec(&serde_json::json!({
            "version":1,"relayOrigin":frozen.origin.as_str(),"tunnel":hex(&frozen.pair.tunnel),
            "endpointBinding":hex(&frozen.pair.endpoint_binding),"catalogId":hex(uuid::Uuid::new_v4().as_bytes()),"offers":offers,"expiresUnixMs":expires
        })).map_err(|_| Error::Invalid)?;
    let wire = sign(key, &lease, &frozen.host, CATALOG, &payload, end).await?;
    frozen.check()?;
    let digest = Sha256::digest(wire.as_bytes()).into();
    Ok(Live {
        frozen,
        lease,
        end,
        wire,
        digest,
        expires,
        authority_end,
    })
}

impl Live {
    pub(crate) fn deadline(&self) -> Instant {
        self.end
    }
    pub(crate) fn tunnel(&self) -> [u8; 16] {
        self.frozen.pair.tunnel
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.lease.is_revoked() || Instant::now() >= self.end || unix_ms()? >= self.expires {
            return Err(Error::Expired);
        }
        self.frozen.check()
    }
    pub(crate) fn target(&self, selected: &ShareSelectionKey) -> Result<&NativeRunAdmission> {
        self.check()?;
        if selected.tunnel != self.tunnel()
            || selected.catalog_digest != self.digest
            || selected.selection_id == [0; 16]
        {
            return Err(Error::Authority);
        }
        self.frozen
            .offers
            .iter()
            .find(|o| o.id == selected.scope_offer_id)
            .map(|o| &o.target.run)
            .ok_or(Error::Authority)
    }
    pub(crate) async fn rotate(&self, key: &KeyRequestHandle) -> Result<Live> {
        make_live(
            self.frozen.clone(),
            self.lease.clone(),
            self.authority_end,
            key,
        )
        .await
    }
    pub(crate) async fn ready(
        &self,
        key: &KeyRequestHandle,
        selected: &ShareSelectionKey,
    ) -> Result<String> {
        let scope = self
            .target(selected)?
            .scope()
            .map_err(|_| Error::Authority)?;
        let payload=serde_json::to_vec(&serde_json::json!({
            "version":1,"relayOrigin":self.frozen.origin.as_str(),"tunnel":hex(&selected.tunnel),"endpointBinding":hex(&self.frozen.pair.endpoint_binding),
            "catalogDigest":hex(&self.digest),"offerId":hex(&selected.scope_offer_id),"selectionId":hex(&selected.selection_id),
            "session":scope.session,"run":scope.run,"generation":scope.generation,"expiresUnixMs":self.expires
        })).map_err(|_|Error::Invalid)?;
        let wire = sign(
            key,
            &self.lease,
            &self.frozen.host,
            READY,
            &payload,
            self.end,
        )
        .await?;
        self.target(selected)?;
        Ok(wire)
    }
}
async fn sign(
    key: &KeyRequestHandle,
    lease: &ApprovalLease,
    public: &[u8; 91],
    domain: &[u8],
    payload: &[u8],
    end: Instant,
) -> Result<String> {
    if key.binding() != (SigningPurpose::Endpoint, public.as_slice()) || lease.is_revoked() {
        return Err(Error::Authority);
    }
    let fence = SigningFence::new_revocable(end, lease.native.clone());
    let attempt = fence
        .advance(
            std::slice::from_ref(key),
            end.min(Instant::now() + Duration::from_secs(5)),
        )
        .map_err(|_| Error::Signing)?;
    let msg = transcript(domain, payload);
    let sig = key
        .begin_attempt(msg.clone(), SignatureEncoding::Fixed, attempt)
        .map_err(|_| Error::Signing)?
        .await
        .map_err(|_| Error::Signing)?;
    if lease.is_revoked() || Instant::now() >= end {
        return Err(Error::Authority);
    }
    verify_p256(public, &msg, &sig)?;
    packed(payload, &sig)
}
