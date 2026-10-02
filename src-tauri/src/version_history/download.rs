//! Bounded official asset transport and document-owned two-stage preparation.
//! No installer is executed. CallerIdentity must come from native admission,
//! and the supplied owner check must recheck that exact live registry identity.
use super::catalog::{BoundAsset, CatalogService, SelectionMetadata};
use super::verified_package::{
    DownloadedPayload, PrivatePackageStore, PublisherKey, VerifiedPackage, MAX_PACKAGE_BYTES,
    MAX_SIGNATURE_BYTES,
};
use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::SafeError;
use cap_std::fs::Dir;
use parking_lot::Mutex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub(crate) const MAX_REDIRECTS: usize = 2;
pub(crate) const PREPARATION_TTL: Duration = Duration::from_secs(5 * 60);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_HELD: usize = 64;
const MAX_ACTIVE: usize = 4; // Four bounded payloads, including pending/ready/cancel-unwinding IO.
const MAX_URL_BYTES: usize = 8192;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BeginPrepareRequest {
    pub(crate) selection_token: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PrepareTransactionRequest {
    pub(crate) transaction_id: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparationTicket {
    pub(crate) transaction_id: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparedPackageSummary {
    pub(crate) transaction_id: String,
    pub(crate) version: String,
    pub(crate) verification: &'static str,
    pub(crate) install_ready: bool,
    pub(crate) blocked_reason: &'static str,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CancelPrepareSummary {
    pub(crate) transaction_id: String,
    pub(crate) cancelled: bool,
}

fn secure_url(raw: &str) -> Result<Url, SafeError> {
    if raw.len() > MAX_URL_BYTES
        || raw
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\\')
    {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    let url = Url::parse(raw).map_err(|_| error("HISTORY_REDIRECT_BLOCKED"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_some()
    {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    Ok(url)
}
pub(crate) fn validate_redirect(raw: &str, hop: usize) -> Result<Url, SafeError> {
    if hop == 0 || hop > MAX_REDIRECTS {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    let url = secure_url(raw)?;
    let prefix = match url.host_str() {
        Some("release-assets.githubusercontent.com") => "/github-production-release-asset/",
        Some("objects.githubusercontent.com") => "/github-production-release-asset-2e65be/",
        _ => return Err(error("HISTORY_REDIRECT_BLOCKED")),
    };
    let suffix = url
        .path()
        .strip_prefix(prefix)
        .ok_or_else(|| error("HISTORY_REDIRECT_BLOCKED"))?;
    // Opaque numeric repository id / asset object key; encoded traversal or a
    // different GitHub service path must never become an asset destination.
    if suffix.is_empty()
        || suffix.split('/').any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    Ok(url)
}
fn initial_url(asset: &BoundAsset) -> Result<Url, SafeError> {
    let url = secure_url(asset.download_url())?;
    if url.host_str() != Some("github.com")
        || url.query().is_some()
        || !url
            .path()
            .starts_with("/shawnwu2022/cc-desk/releases/download/v")
        || !url.path().ends_with(&format!("/{}", asset.name()))
    {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    Ok(url)
}

pub(crate) struct DownloadResponse {
    pub(crate) content_length: Option<u64>,
    pub(crate) body: Box<dyn Read + Send>,
}
/// Only the external transport is replaced in deterministic tests. Asset URL,
/// expected size/digest and names come from CatalogService's held selection.
pub(crate) trait AssetSource: Send + Sync {
    fn open(
        &self,
        asset: &BoundAsset,
        check: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<DownloadResponse, SafeError>;
}
struct OfficialAssetSource {
    client: reqwest::blocking::Client,
}
impl OfficialAssetSource {
    fn new() -> Result<Self, SafeError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("CC-Desk-Historical-Preparation")
            .build()
            .map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        Ok(Self { client })
    }
}
impl AssetSource for OfficialAssetSource {
    fn open(
        &self,
        asset: &BoundAsset,
        check: &dyn Fn() -> Result<(), SafeError>,
    ) -> Result<DownloadResponse, SafeError> {
        let mut destination = initial_url(asset)?;
        let started = Instant::now();
        let mut seen = HashSet::new();
        for hop in 0..=MAX_REDIRECTS {
            check()?;
            if !seen.insert(destination.as_str().to_owned()) {
                return Err(error("HISTORY_REDIRECT_BLOCKED"));
            }
            let timeout = REQUEST_TIMEOUT
                .checked_sub(started.elapsed())
                .ok_or_else(|| error("HISTORY_DOWNLOAD_TIMEOUT"))?;
            let response = self
                .client
                .get(destination.clone())
                .header(reqwest::header::ACCEPT_ENCODING, "identity")
                .timeout(timeout)
                .send()
                .map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
            check()?;
            match response.status().as_u16() {
                301 | 302 | 303 | 307 | 308 => {
                    let location = response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok())
                        .ok_or_else(|| error("HISTORY_REDIRECT_BLOCKED"))?;
                    // Only absolute official CDN asset destinations are admitted.
                    // No cookies, authentication headers or custom caller headers exist.
                    destination = validate_redirect(location, hop + 1)?;
                }
                200 => {
                    if response
                        .headers()
                        .get(reqwest::header::CONTENT_ENCODING)
                        .is_some_and(|value| value.as_bytes() != b"identity")
                    {
                        return Err(error("HISTORY_DOWNLOAD_ENCODING"));
                    }
                    return Ok(DownloadResponse {
                        content_length: response.content_length(),
                        body: Box::new(response),
                    });
                }
                403 | 429 => return Err(error("HISTORY_RATE_LIMITED")),
                404 | 410 => return Err(error("HISTORY_RELEASE_UNAVAILABLE")),
                _ => return Err(error("HISTORY_NETWORK_UNAVAILABLE")),
            }
        }
        Err(error("HISTORY_REDIRECT_BLOCKED"))
    }
}

type OwnerCheck = dyn Fn(&CallerIdentity) -> Result<(), SafeError> + Send + Sync;
type Clock = dyn Fn() -> Instant + Send + Sync;
enum Phase {
    Reserved,
    Running,
    Ready(Arc<VerifiedPackage>),
    Handoff { switch_id: String },
    ManagerOwned { switch_id: String },
    Cancelled,
    Failed,
}
struct Transaction {
    owner: CallerIdentity,
    token: String,
    selection: SelectionMetadata,
    expires: Instant,
    cancelled: Arc<AtomicBool>,
    phase: Phase,
    in_flight: bool,
    // Once issued, even a cancelled/failed preparation cannot imply that its
    // private/native switch work has been safely reversed.
    issued_switch: Option<String>,
    verified_abort: bool,
}
impl Transaction {
    fn active(&self) -> bool {
        self.in_flight
            || (self.issued_switch.is_some() && !self.verified_abort)
            || matches!(
                self.phase,
                Phase::Reserved
                    | Phase::Running
                    | Phase::Ready(_)
                    | Phase::Handoff { .. }
                    | Phase::ManagerOwned { .. }
            )
    }
}
pub(crate) struct PrepareService {
    catalog: Arc<CatalogService>,
    source: Arc<dyn AssetSource>,
    store: Arc<PrivatePackageStore>,
    publisher: Arc<PublisherKey>,
    owner_check: Arc<OwnerCheck>,
    clock: Arc<Clock>,
    held: Mutex<HashMap<String, Transaction>>,
}

/// Only the reservation winner receives transfer authority. Repeated source
/// requests can inspect the same switch UUID but cannot copy/launch a second
/// manager. Preparation IDs keep their original 32-hex wire contract.
pub(crate) struct HandoffReservation {
    pub(crate) transaction_id: String,
    pub(crate) transfer: Option<PreparedHandoff>,
}

/// Keeps the exact prepared file alive while bounded private copying happens
/// outside the preparation mutex. This is still source-document authority,
/// not installer admission and not yet durable manager ownership.
pub(crate) struct PreparedHandoff {
    service: Arc<PrepareService>,
    caller: CallerIdentity,
    preparation_id: String,
    switch_id: String,
    package: Arc<VerifiedPackage>,
    observation: Vec<u8>,
    // Rust drops fields in declaration order: release the original package and
    // observation before making this operation's capacity available again.
    _budget: HandoffBudget,
}
struct HandoffBudget {
    service: Arc<PrepareService>,
    caller: CallerIdentity,
    preparation_id: String,
}
impl Drop for HandoffBudget {
    fn drop(&mut self) {
        self.service
            .finish_handoff_budget(&self.caller, &self.preparation_id);
    }
}
impl PreparedHandoff {
    #[cfg(windows)]
    pub(crate) fn record_unstarted(
        &self,
        outcome: &super::windows::source_lifecycle::UnstartedSource,
    ) -> Result<(), SafeError> {
        outcome.verify_document(&self.caller, &self.switch_id)?;
        let mut held = self.service.held.lock();
        let item = transaction_mut(&mut held, &self.caller, &self.preparation_id)?;
        if item.issued_switch.as_deref() != Some(self.switch_id.as_str()) {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        item.verified_abort = true;
        item.phase = Phase::Failed;
        Ok(())
    }
    #[cfg(windows)]
    pub(crate) fn record_verified_abort(
        &self,
        outcome: &super::windows::pre_context_abort::VerifiedPrivateAbort<'_>,
    ) -> Result<(), SafeError> {
        outcome.verify_document(&self.caller, &self.switch_id)?;
        let mut held = self.service.held.lock();
        let item = transaction_mut(&mut held, &self.caller, &self.preparation_id)?;
        if item.issued_switch.as_deref() != Some(self.switch_id.as_str()) {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        item.verified_abort = true;
        item.phase = Phase::Failed;
        Ok(())
    }
    pub(crate) fn transaction_id(&self) -> &str {
        &self.switch_id
    }
    pub(crate) fn preparation_id(&self) -> &str {
        &self.preparation_id
    }
    pub(crate) fn selection(&self) -> &SelectionMetadata {
        self.package.selection()
    }
    pub(crate) fn check(&self) -> Result<(), SafeError> {
        (self.service.owner_check)(&self.caller)?;
        let token = {
            let mut held = self.service.held.lock();
            let item = transaction_mut(&mut held, &self.caller, &self.preparation_id)?;
            state_check(item, (self.service.clock)())?;
            if !matches!(&item.phase, Phase::Handoff { switch_id } if switch_id == &self.switch_id)
            {
                return Err(error("HISTORY_HANDOFF_CHANGED"));
            }
            item.token.clone()
        };
        if self
            .service
            .catalog
            .resolve_selection(&self.caller, &token)?
            != *self.package.selection()
        {
            return Err(error("HISTORY_SELECTION_CHANGED"));
        }
        Ok(())
    }
    /// The material callback has no transaction lock. It may copy to a secured
    /// destination and must call check between bounded writes. Both sides are
    /// checked again here, so cancellation/replacement cannot publish success.
    pub(crate) fn with_material<T>(
        &self,
        copy: impl FnOnce(&[u8], &[u8], &[u8], &str) -> Result<T, SafeError>,
    ) -> Result<T, SafeError> {
        self.package.revalidate(&|| self.check())?;
        let result = copy(
            self.package.bytes(),
            self.package.signature(),
            &self.observation,
            self.package.retained_identity(),
        )?;
        self.package.revalidate(&|| self.check())?;
        Ok(result)
    }
    #[cfg(windows)]
    pub(crate) fn complete(
        &self,
        retained: super::windows::package::RetainedPackage,
    ) -> Result<super::windows::package::RetainedPackage, SafeError> {
        // Full destination readback/signature verification occurs outside the
        // preparation mutex; the final ownership change is short and atomic.
        retained.verify_transfer(self)?;
        let mut held = self.service.held.lock();
        let item = transaction_mut(&mut held, &self.caller, &self.preparation_id)?;
        state_check(item, (self.service.clock)())?;
        (self.service.owner_check)(&self.caller)?;
        if self
            .service
            .catalog
            .resolve_selection(&self.caller, &item.token)?
            != *self.package.selection()
        {
            return Err(error("HISTORY_SELECTION_CHANGED"));
        }
        if !matches!(&item.phase, Phase::Handoff { switch_id } if switch_id == &self.switch_id) {
            return Err(error("HISTORY_HANDOFF_CHANGED"));
        }
        item.phase = Phase::ManagerOwned {
            switch_id: self.switch_id.clone(),
        };
        drop(held);
        Ok(retained)
    }
}
impl PrepareService {
    /// Does not start IO, re-download, allocate a switch UUID, cancel, or replay
    /// a handoff. Issued UUIDs remain observable even after preparation expiry.
    pub(crate) fn inspect_switch(
        &self,
        caller: &CallerIdentity,
        id: &str,
    ) -> Result<super::manager::SwitchReview, SafeError> {
        use super::manager::{
            SwitchContextPolicy, SwitchReview, SwitchReviewAction as Action,
            SwitchReviewBlock as Block, SwitchReviewPhase as ReviewPhase,
        };
        (self.owner_check)(caller)?;
        let mut held = self.held.lock();
        let item = transaction_mut(&mut held, caller, id)?;
        let mut result = SwitchReview {
            preparation_id: id.to_owned(),
            version: item.selection.version().to_owned(),
            phase: ReviewPhase::Unavailable,
            context_policy: SwitchContextPolicy::FreshSettingsPreserveCurrentSharedCli,
            transaction_id: None,
            allowed_actions: vec![Action::Refresh],
            block_reason: None,
        };
        if let Some(switch_id) = &item.issued_switch {
            result.transaction_id = Some(switch_id.clone());
            if item.verified_abort {
                result.phase = ReviewPhase::Aborted;
                result.allowed_actions.push(Action::PrepareAgain);
            } else {
                result.phase = ReviewPhase::HandoffIssued;
                result.block_reason = Some(Block::HandoffIssued);
            }
        } else if matches!(item.phase, Phase::Cancelled) {
            result.phase = ReviewPhase::Cancelled;
        } else if (self.clock)() >= item.expires {
            result.block_reason = Some(Block::PreparationExpired);
            result.allowed_actions.push(Action::CancelPreparation);
        } else {
            match &item.phase {
                Phase::Reserved | Phase::Running => {
                    result.phase = ReviewPhase::Preparing;
                    result.block_reason = Some(Block::PreparationPending);
                    result.allowed_actions.push(Action::CancelPreparation);
                }
                Phase::Ready(_) => {
                    result.phase = ReviewPhase::Verified;
                    result.allowed_actions.push(Action::Review);
                    if item.in_flight {
                        result.block_reason = Some(Block::PreparationBusy);
                    } else {
                        result.block_reason = super::payload_policy::review_block(&item.selection);
                        result.allowed_actions.push(Action::CancelPreparation);
                        if result.block_reason.is_none() {
                            result.allowed_actions.push(Action::BeginSwitch);
                        }
                    }
                }
                Phase::Failed => {
                    result.block_reason = Some(Block::PreparationFailed);
                    result.allowed_actions.push(Action::CancelPreparation);
                }
                Phase::Handoff { .. } | Phase::ManagerOwned { .. } | Phase::Cancelled => {
                    unreachable!("terminal preparation cases handled above")
                }
            }
        }
        (self.owner_check)(caller)?;
        Ok(result)
    }
    /// Use only a pinned private manager directory, admitted CallerIdentity and
    /// exact registry-liveness check. IPC must re-admit after this blocking work
    /// before publishing responses or admitting subsequent manager effects.
    pub(crate) fn production(
        catalog: Arc<CatalogService>,
        private_parent: Dir,
        owner_check: Arc<OwnerCheck>,
    ) -> Result<Self, SafeError> {
        Self::new(
            catalog,
            Arc::new(OfficialAssetSource::new()?),
            private_parent,
            owner_check,
            Arc::new(Instant::now),
            PublisherKey::production()?,
        )
    }
    fn new(
        catalog: Arc<CatalogService>,
        source: Arc<dyn AssetSource>,
        private_parent: Dir,
        owner_check: Arc<OwnerCheck>,
        clock: Arc<Clock>,
        publisher: PublisherKey,
    ) -> Result<Self, SafeError> {
        Ok(Self {
            catalog,
            source,
            store: PrivatePackageStore::new(private_parent)?,
            publisher: Arc::new(publisher),
            owner_check,
            clock,
            held: Mutex::new(HashMap::new()),
        })
    }
    #[cfg(test)]
    pub(crate) fn with_test_boundaries(
        catalog: Arc<CatalogService>,
        source: Arc<dyn AssetSource>,
        private_parent: Dir,
        owner_check: Arc<OwnerCheck>,
        clock: Arc<Clock>,
        encoded_key: &str,
    ) -> Result<Self, SafeError> {
        Self::new(
            catalog,
            source,
            private_parent,
            owner_check,
            clock,
            PublisherKey::fixture(encoded_key)?,
        )
    }
    /// Reserves exactly one operation before any network IO or transaction files.
    /// Returning its opaque ID first makes cancellation possible during blocking IO.
    pub(crate) fn begin_prepare(
        &self,
        caller: &CallerIdentity,
        token: &str,
    ) -> Result<PreparationTicket, SafeError> {
        (self.owner_check)(caller)?;
        let selection = self.catalog.resolve_selection(caller, token)?;
        let now = (self.clock)();
        let mut held = self.held.lock();
        // Only finished expired entries may be discarded. A running operation
        // continues counting against memory/disk capacity until its IO unwinds.
        held.retain(|_, item| {
            now < item.expires
                || item.in_flight
                || item.issued_switch.is_some()
                || matches!(
                    item.phase,
                    Phase::Handoff { .. } | Phase::ManagerOwned { .. }
                )
        });
        if held
            .values()
            .any(|item| &item.owner == caller && item.active())
        {
            return Err(error("HISTORY_PREPARE_BUSY"));
        }
        if held.len() >= MAX_HELD
            || held.values().filter(|item| item.active()).count() >= MAX_ACTIVE
        {
            return Err(error("HISTORY_CAPACITY"));
        }
        (self.owner_check)(caller)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        held.insert(
            id.clone(),
            Transaction {
                owner: caller.clone(),
                token: token.to_owned(),
                selection,
                expires: now + PREPARATION_TTL,
                cancelled: Arc::new(AtomicBool::new(false)),
                phase: Phase::Reserved,
                in_flight: false,
                issued_switch: None,
                verified_abort: false,
            },
        );
        Ok(PreparationTicket { transaction_id: id })
    }
    pub(crate) fn prepare_history(
        &self,
        caller: &CallerIdentity,
        id: &str,
    ) -> Result<PreparedPackageSummary, SafeError> {
        (self.owner_check)(caller)?;
        let (selection, token, cancelled, expires) = {
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            if !matches!(item.phase, Phase::Reserved) {
                return Err(error("HISTORY_PREPARE_ALREADY_STARTED"));
            }
            item.phase = Phase::Running;
            item.in_flight = true;
            (
                item.selection.clone(),
                item.token.clone(),
                item.cancelled.clone(),
                item.expires,
            )
        };
        let started = (self.clock)();
        let check = || -> Result<(), SafeError> {
            if cancelled.load(Ordering::SeqCst) {
                return Err(error("HISTORY_PREPARE_CANCELLED"));
            }
            let now = (self.clock)();
            if now >= expires {
                return Err(error("HISTORY_PREPARE_EXPIRED"));
            }
            if now.saturating_duration_since(started) >= DOWNLOAD_TIMEOUT {
                return Err(error("HISTORY_DOWNLOAD_TIMEOUT"));
            }
            (self.owner_check)(caller)?;
            if self.catalog.resolve_selection(caller, &token)? != selection {
                return Err(error("HISTORY_SELECTION_CHANGED"));
            }
            Ok(())
        };
        let result = (|| {
            check()?;
            if self.catalog.revalidate_selection(caller, &token)? != selection {
                return Err(error("HISTORY_SELECTION_CHANGED"));
            }
            check()?;
            let storage = self.store.transaction(id)?;
            let mut file = storage.create_package()?;
            let signature = read_asset(
                self.source.as_ref(),
                selection.signature(),
                MAX_SIGNATURE_BYTES,
                None,
                &check,
            )?;
            let bytes = read_asset(
                self.source.as_ref(),
                selection.installer(),
                MAX_PACKAGE_BYTES,
                Some(&mut file),
                &check,
            )?;
            let package = Arc::new(VerifiedPackage::finish(
                DownloadedPayload {
                    selection: selection.clone(),
                    bytes,
                    signature,
                },
                self.publisher.clone(),
                storage,
                file,
                &check,
            )?);
            if self.catalog.revalidate_selection(caller, &token)? != selection {
                return Err(error("HISTORY_SELECTION_CHANGED"));
            }
            check()?;
            // Cancel and ready publication share this lock: cancellation cannot
            // win and then be overwritten by a late verified completion.
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            (self.owner_check)(caller)?;
            if !matches!(item.phase, Phase::Running) {
                return Err(error("HISTORY_PREPARE_CANCELLED"));
            }
            item.phase = Phase::Ready(package);
            item.in_flight = false;
            Ok(summary(id, &selection))
        })();
        if result.is_err() {
            if let Some(item) = self
                .held
                .lock()
                .get_mut(id)
                .filter(|item| &item.owner == caller)
            {
                item.in_flight = false;
                if matches!(item.phase, Phase::Running) {
                    item.phase = Phase::Failed;
                }
            }
        }
        result
    }
    pub(crate) fn cancel_prepare(
        &self,
        caller: &CallerIdentity,
        id: &str,
    ) -> Result<CancelPrepareSummary, SafeError> {
        (self.owner_check)(caller)?;
        let mut held = self.held.lock();
        let item = transaction_mut(&mut held, caller, id)?;
        if matches!(item.phase, Phase::ManagerOwned { .. }) {
            return Err(error("HISTORY_RECOVERY_REQUIRED"));
        }
        // Failed/cancelled cancellation is idempotent; a ready package is dropped
        // and its private files removed. In-flight IO cleans up as it unwinds.
        item.cancelled.store(true, Ordering::SeqCst);
        item.phase = Phase::Cancelled;
        Ok(CancelPrepareSummary {
            transaction_id: id.to_owned(),
            cancelled: true,
        })
    }
    pub(crate) fn reserve_handoff(
        self: &Arc<Self>,
        caller: &CallerIdentity,
        id: &str,
    ) -> Result<HandoffReservation, SafeError> {
        self.reserve_handoff_admitted(caller, id, |_| Ok(()))
            .map(|(reservation, _)| reservation)
    }
    /// The winner's additional admission sees the actual still-pinned verified
    /// package before a UUID is reserved. A policy refusal keeps preparation
    /// available; duplicates retain their existing UUID and never re-admit an
    /// installer or return another transfer capability.
    pub(crate) fn reserve_handoff_admitted<T>(
        self: &Arc<Self>,
        caller: &CallerIdentity,
        id: &str,
        admit: impl FnOnce(&VerifiedPackage) -> Result<T, SafeError>,
    ) -> Result<(HandoffReservation, Option<T>), SafeError> {
        self.reserve_handoff_with_source(caller, id, |package, _transaction| admit(package))
    }
    /// The production source-admission callback receives a backend-generated
    /// candidate UUID before publication. A rejected zero-owner freeze leaves
    /// the package ready, and its guard releases an uncommitted freeze if final
    /// package/document checks fail. Duplicates never call it again.
    pub(crate) fn reserve_handoff_with_source<T>(
        self: &Arc<Self>,
        caller: &CallerIdentity,
        id: &str,
        admit: impl FnOnce(&VerifiedPackage, &str) -> Result<T, SafeError>,
    ) -> Result<(HandoffReservation, Option<T>), SafeError> {
        (self.owner_check)(caller)?;
        let (package, token, selection) = {
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            if let Phase::ManagerOwned { switch_id } = &item.phase {
                return Ok((
                    HandoffReservation {
                        transaction_id: switch_id.clone(),
                        transfer: None,
                    },
                    None,
                ));
            }
            state_check(item, (self.clock)())?;
            if let Phase::Handoff { switch_id } = &item.phase {
                return Ok((
                    HandoffReservation {
                        transaction_id: switch_id.clone(),
                        transfer: None,
                    },
                    None,
                ));
            }
            if item.in_flight {
                return Err(error("HISTORY_PREPARE_BUSY"));
            }
            let Phase::Ready(package) = &item.phase else {
                return Err(error("HISTORY_PREPARE_NOT_READY"));
            };
            let package = package.clone();
            item.in_flight = true;
            (package, item.token.clone(), item.selection.clone())
        };
        let mut policy_refused = false;
        let result = (|| {
            let check = || {
                (self.owner_check)(caller)?;
                let mut held = self.held.lock();
                state_check(transaction_mut(&mut held, caller, id)?, (self.clock)())
            };
            let observation = self.catalog.retain_selection_observation(caller, &token)?;
            if self.catalog.resolve_selection(caller, &token)? != selection {
                return Err(error("HISTORY_SELECTION_CHANGED"));
            }
            package.revalidate(&check)?;
            let switch_id = uuid::Uuid::new_v4().to_string();
            let permit = admit(&package, &switch_id).map_err(|failure| {
                policy_refused = true;
                failure
            })?;
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            (self.owner_check)(caller)?;
            let Phase::Ready(current) = &item.phase else {
                return Err(error("HISTORY_PREPARE_NOT_READY"));
            };
            if !Arc::ptr_eq(current, &package) {
                return Err(error("HISTORY_PACKAGE_CHANGED"));
            }
            item.issued_switch = Some(switch_id.clone());
            item.phase = Phase::Handoff {
                switch_id: switch_id.clone(),
            };
            Ok((
                HandoffReservation {
                    transaction_id: switch_id.clone(),
                    transfer: Some(PreparedHandoff {
                        service: self.clone(),
                        caller: caller.clone(),
                        preparation_id: id.to_owned(),
                        switch_id,
                        package: package.clone(),
                        observation,
                        _budget: HandoffBudget {
                            service: self.clone(),
                            caller: caller.clone(),
                            preparation_id: id.to_owned(),
                        },
                    }),
                },
                Some(permit),
            ))
        })();
        drop(package);
        if let Some(item) = self
            .held
            .lock()
            .get_mut(id)
            .filter(|item| &item.owner == caller)
        {
            if result.is_err() {
                item.in_flight = false;
            }
            if result.is_err() && !policy_refused && matches!(item.phase, Phase::Ready(_)) {
                item.phase = Phase::Failed;
            }
        }
        result
    }
    fn finish_handoff_budget(&self, caller: &CallerIdentity, id: &str) {
        if let Some(item) = self
            .held
            .lock()
            .get_mut(id)
            .filter(|item| &item.owner == caller)
        {
            item.in_flight = false;
        }
    }
    /// Rust-only handoff. Revalidates metadata and bytes on the same retained file
    /// object; then cancellation, expiry and the short consumer admission share
    /// the transaction lock. The consumer must not re-enter this service, perform
    /// long blocking work here, or claim install eligibility without its manifest.
    /// No VerifiedPackage reference can escape this borrow or be built from IPC.
    pub(crate) fn with_verified_package<T>(
        &self,
        caller: &CallerIdentity,
        id: &str,
        consume: impl FnOnce(&VerifiedPackage) -> Result<T, SafeError>,
    ) -> Result<T, SafeError> {
        (self.owner_check)(caller)?;
        let (package, token, selection) = {
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            if item.in_flight {
                return Err(error("HISTORY_PREPARE_BUSY"));
            }
            let Phase::Ready(package) = &item.phase else {
                return Err(error("HISTORY_PREPARE_NOT_READY"));
            };
            let package = package.clone();
            item.in_flight = true;
            (package, item.token.clone(), item.selection.clone())
        };
        let mut consumer_admitted = false;
        let result = (|| {
            let check = || {
                (self.owner_check)(caller)?;
                let mut held = self.held.lock();
                state_check(transaction_mut(&mut held, caller, id)?, (self.clock)())
            };
            if self.catalog.revalidate_selection(caller, &token)? != selection {
                return Err(error("HISTORY_SELECTION_CHANGED"));
            }
            package.revalidate(&check)?;
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            (self.owner_check)(caller)?;
            self.catalog.resolve_selection(caller, &token)?;
            let Phase::Ready(current) = &item.phase else {
                return Err(error("HISTORY_PREPARE_NOT_READY"));
            };
            if !Arc::ptr_eq(current, &package) {
                return Err(error("HISTORY_PACKAGE_CHANGED"));
            }
            consumer_admitted = true;
            consume(&package)
        })();
        // Release the retained temporary package before releasing its budget.
        drop(package);
        if let Some(item) = self
            .held
            .lock()
            .get_mut(id)
            .filter(|item| &item.owner == caller)
        {
            item.in_flight = false;
            if result.is_err() && !consumer_admitted && matches!(item.phase, Phase::Ready(_)) {
                item.phase = Phase::Failed;
            }
        }
        result
    }
}
fn summary(id: &str, selection: &SelectionMetadata) -> PreparedPackageSummary {
    PreparedPackageSummary {
        transaction_id: id.to_owned(),
        version: selection.version().to_owned(),
        verification: "publisher-verified",
        install_ready: false,
        blocked_reason: "PACKAGE_IDENTITY_UNVERIFIED",
    }
}
fn transaction_mut<'a>(
    held: &'a mut HashMap<String, Transaction>,
    caller: &CallerIdentity,
    id: &str,
) -> Result<&'a mut Transaction, SafeError> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(error("HISTORY_PREPARE_UNKNOWN"));
    }
    held.get_mut(id)
        .filter(|item| &item.owner == caller)
        .ok_or_else(|| error("HISTORY_PREPARE_UNKNOWN"))
}
fn state_check(item: &mut Transaction, now: Instant) -> Result<(), SafeError> {
    if item.cancelled.load(Ordering::SeqCst) || matches!(item.phase, Phase::Cancelled) {
        return Err(error("HISTORY_PREPARE_CANCELLED"));
    }
    if now >= item.expires {
        if !item.in_flight
            && !matches!(
                item.phase,
                Phase::Handoff { .. } | Phase::ManagerOwned { .. }
            )
        {
            item.phase = Phase::Failed;
        }
        return Err(error("HISTORY_PREPARE_EXPIRED"));
    }
    if matches!(item.phase, Phase::Failed) {
        return Err(error("HISTORY_PREPARE_FAILED"));
    }
    Ok(())
}
fn read_asset(
    source: &dyn AssetSource,
    asset: &BoundAsset,
    limit: u64,
    mut output: Option<&mut std::fs::File>,
    check: &dyn Fn() -> Result<(), SafeError>,
) -> Result<Vec<u8>, SafeError> {
    check()?;
    if asset.size() == 0 || asset.size() > limit {
        return Err(error("HISTORY_DOWNLOAD_TOO_LARGE"));
    }
    let mut response = source.open(asset, check)?;
    check()?;
    if response
        .content_length
        .is_some_and(|size| size != asset.size())
    {
        return Err(error("HISTORY_SIZE_MISMATCH"));
    }
    let mut bytes = Vec::with_capacity(asset.size().min(1024 * 1024) as usize);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        check()?;
        let remaining = asset.size().saturating_sub(bytes.len() as u64);
        let maximum = buffer.len().min(remaining as usize + 1);
        let count = response
            .body
            .read(&mut buffer[..maximum])
            .map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        check()?;
        if count == 0 {
            break;
        }
        if bytes.len() as u64 + count as u64 > asset.size() {
            return Err(error("HISTORY_SIZE_MISMATCH"));
        }
        if let Some(file) = output.as_mut() {
            file.write_all(&buffer[..count])
                .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if bytes.len() as u64 != asset.size() {
        return Err(error("HISTORY_SIZE_MISMATCH"));
    }
    if let Some(file) = output {
        file.sync_all()
            .map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?;
    }
    check()?;
    Ok(bytes)
}
