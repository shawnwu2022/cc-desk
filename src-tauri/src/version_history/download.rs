//! Bounded official asset transport and document-owned two-stage preparation.
//! No installer is executed. CallerIdentity must come from native admission,
//! and the supplied owner check must recheck that exact live registry identity.
use super::catalog::{BoundAsset, CatalogService, SelectionMetadata};
use super::verified_package::{DownloadedPayload, PrivatePackageStore, PublisherKey, VerifiedPackage, MAX_PACKAGE_BYTES, MAX_SIGNATURE_BYTES};
use crate::cli::profiles::error;
use crate::cli::snapshot::CallerIdentity;
use crate::cli::types::SafeError;
use cap_std::fs::Dir;
use parking_lot::Mutex;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
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
pub(crate) struct BeginPrepareRequest { pub(crate) selection_token: String }
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PrepareTransactionRequest { pub(crate) transaction_id: String }
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreparationTicket { pub(crate) transaction_id: String }
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
    if raw.len() > MAX_URL_BYTES || raw.bytes().any(|byte| byte.is_ascii_control() || byte == b'\\') {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    let url = Url::parse(raw).map_err(|_| error("HISTORY_REDIRECT_BLOCKED"))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some()
        || url.fragment().is_some() || url.port().is_some() {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    Ok(url)
}
pub(crate) fn validate_redirect(raw: &str, hop: usize) -> Result<Url, SafeError> {
    if hop == 0 || hop > MAX_REDIRECTS { return Err(error("HISTORY_REDIRECT_BLOCKED")); }
    let url = secure_url(raw)?;
    let prefix = match url.host_str() {
        Some("release-assets.githubusercontent.com") => "/github-production-release-asset/",
        Some("objects.githubusercontent.com") => "/github-production-release-asset-2e65be/",
        _ => return Err(error("HISTORY_REDIRECT_BLOCKED")),
    };
    let suffix = url.path().strip_prefix(prefix).ok_or_else(|| error("HISTORY_REDIRECT_BLOCKED"))?;
    // Opaque numeric repository id / asset object key; encoded traversal or a
    // different GitHub service path must never become an asset destination.
    if suffix.is_empty() || suffix.split('/').any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')) {
        return Err(error("HISTORY_REDIRECT_BLOCKED"));
    }
    Ok(url)
}
fn initial_url(asset: &BoundAsset) -> Result<Url, SafeError> {
    let url = secure_url(asset.download_url())?;
    if url.host_str() != Some("github.com") || url.query().is_some()
        || !url.path().starts_with("/shawnwu2022/cc-desk/releases/download/v")
        || !url.path().ends_with(&format!("/{}", asset.name())) {
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
    fn open(&self, asset: &BoundAsset, check: &dyn Fn() -> Result<(), SafeError>) -> Result<DownloadResponse, SafeError>;
}
struct OfficialAssetSource { client: reqwest::blocking::Client }
impl OfficialAssetSource {
    fn new() -> Result<Self, SafeError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5)).timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none()).user_agent("CC-Desk-Historical-Preparation")
            .build().map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        Ok(Self { client })
    }
}
impl AssetSource for OfficialAssetSource {
    fn open(&self, asset: &BoundAsset, check: &dyn Fn() -> Result<(), SafeError>) -> Result<DownloadResponse, SafeError> {
        let mut destination = initial_url(asset)?;
        let started = Instant::now();
        let mut seen = HashSet::new();
        for hop in 0..=MAX_REDIRECTS {
            check()?;
            if !seen.insert(destination.as_str().to_owned()) { return Err(error("HISTORY_REDIRECT_BLOCKED")); }
            let timeout = REQUEST_TIMEOUT.checked_sub(started.elapsed()).ok_or_else(|| error("HISTORY_DOWNLOAD_TIMEOUT"))?;
            let response = self.client.get(destination.clone()).header(reqwest::header::ACCEPT_ENCODING, "identity")
                .timeout(timeout).send().map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
            check()?;
            match response.status().as_u16() {
                301 | 302 | 303 | 307 | 308 => {
                    let location = response.headers().get(reqwest::header::LOCATION)
                        .and_then(|value| value.to_str().ok()).ok_or_else(|| error("HISTORY_REDIRECT_BLOCKED"))?;
                    // Only absolute official CDN asset destinations are admitted.
                    // No cookies, authentication headers or custom caller headers exist.
                    destination = validate_redirect(location, hop + 1)?;
                }
                200 => {
                    if response.headers().get(reqwest::header::CONTENT_ENCODING).is_some_and(|value| value.as_bytes() != b"identity") {
                        return Err(error("HISTORY_DOWNLOAD_ENCODING"));
                    }
                    return Ok(DownloadResponse { content_length: response.content_length(), body: Box::new(response) });
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
enum Phase { Reserved, Running, Ready(Arc<VerifiedPackage>), Cancelled, Failed }
struct Transaction {
    owner: CallerIdentity,
    token: String,
    selection: SelectionMetadata,
    expires: Instant,
    cancelled: Arc<AtomicBool>,
    phase: Phase,
    in_flight: bool,
}
impl Transaction {
    fn active(&self) -> bool { self.in_flight || matches!(self.phase, Phase::Reserved | Phase::Running | Phase::Ready(_)) }
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
impl PrepareService {
    /// Use only a pinned private manager directory, admitted CallerIdentity and
    /// exact registry-liveness check. IPC must re-admit after this blocking work
    /// before publishing responses or admitting subsequent manager effects.
    pub(crate) fn production(catalog: Arc<CatalogService>, private_parent: Dir, owner_check: Arc<OwnerCheck>) -> Result<Self, SafeError> {
        Self::new(catalog, Arc::new(OfficialAssetSource::new()?), private_parent, owner_check, Arc::new(Instant::now), PublisherKey::production()?)
    }
    fn new(catalog: Arc<CatalogService>, source: Arc<dyn AssetSource>, private_parent: Dir, owner_check: Arc<OwnerCheck>, clock: Arc<Clock>, publisher: PublisherKey) -> Result<Self, SafeError> {
        Ok(Self { catalog, source, store: PrivatePackageStore::new(private_parent)?, publisher: Arc::new(publisher), owner_check, clock, held: Mutex::new(HashMap::new()) })
    }
    #[cfg(test)]
    pub(crate) fn with_test_boundaries(catalog: Arc<CatalogService>, source: Arc<dyn AssetSource>, private_parent: Dir, owner_check: Arc<OwnerCheck>, clock: Arc<Clock>, encoded_key: &str) -> Result<Self, SafeError> {
        Self::new(catalog, source, private_parent, owner_check, clock, PublisherKey::fixture(encoded_key)?)
    }
    /// Reserves exactly one operation before any network IO or transaction files.
    /// Returning its opaque ID first makes cancellation possible during blocking IO.
    pub(crate) fn begin_prepare(&self, caller: &CallerIdentity, token: &str) -> Result<PreparationTicket, SafeError> {
        (self.owner_check)(caller)?;
        let selection = self.catalog.resolve_selection(caller, token)?;
        let now = (self.clock)();
        let mut held = self.held.lock();
        // Only finished expired entries may be discarded. A running operation
        // continues counting against memory/disk capacity until its IO unwinds.
        held.retain(|_, item| now < item.expires || item.in_flight);
        if held.values().any(|item| &item.owner == caller && item.active()) { return Err(error("HISTORY_PREPARE_BUSY")); }
        if held.len() >= MAX_HELD || held.values().filter(|item| item.active()).count() >= MAX_ACTIVE { return Err(error("HISTORY_CAPACITY")); }
        (self.owner_check)(caller)?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        held.insert(id.clone(), Transaction { owner: caller.clone(), token: token.to_owned(), selection, expires: now + PREPARATION_TTL, cancelled: Arc::new(AtomicBool::new(false)), phase: Phase::Reserved, in_flight: false });
        Ok(PreparationTicket { transaction_id: id })
    }
    pub(crate) fn prepare_history(&self, caller: &CallerIdentity, id: &str) -> Result<PreparedPackageSummary, SafeError> {
        (self.owner_check)(caller)?;
        let (selection, token, cancelled, expires) = {
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            if !matches!(item.phase, Phase::Reserved) { return Err(error("HISTORY_PREPARE_ALREADY_STARTED")); }
            item.phase = Phase::Running;
            item.in_flight = true;
            (item.selection.clone(), item.token.clone(), item.cancelled.clone(), item.expires)
        };
        let started = (self.clock)();
        let check = || -> Result<(), SafeError> {
            if cancelled.load(Ordering::SeqCst) { return Err(error("HISTORY_PREPARE_CANCELLED")); }
            let now = (self.clock)();
            if now >= expires { return Err(error("HISTORY_PREPARE_EXPIRED")); }
            if now.saturating_duration_since(started) >= DOWNLOAD_TIMEOUT { return Err(error("HISTORY_DOWNLOAD_TIMEOUT")); }
            (self.owner_check)(caller)?;
            if self.catalog.resolve_selection(caller, &token)? != selection { return Err(error("HISTORY_SELECTION_CHANGED")); }
            Ok(())
        };
        let result = (|| {
            check()?;
            if self.catalog.revalidate_selection(caller, &token)? != selection { return Err(error("HISTORY_SELECTION_CHANGED")); }
            check()?;
            let storage = self.store.transaction(id)?;
            let mut file = storage.create_package()?;
            let signature = read_asset(self.source.as_ref(), selection.signature(), MAX_SIGNATURE_BYTES, None, &check)?;
            let bytes = read_asset(self.source.as_ref(), selection.installer(), MAX_PACKAGE_BYTES, Some(&mut file), &check)?;
            let package = Arc::new(VerifiedPackage::finish(DownloadedPayload { selection: selection.clone(), bytes, signature }, self.publisher.clone(), storage, file, &check)?);
            if self.catalog.revalidate_selection(caller, &token)? != selection { return Err(error("HISTORY_SELECTION_CHANGED")); }
            check()?;
            // Cancel and ready publication share this lock: cancellation cannot
            // win and then be overwritten by a late verified completion.
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            (self.owner_check)(caller)?;
            if !matches!(item.phase, Phase::Running) { return Err(error("HISTORY_PREPARE_CANCELLED")); }
            item.phase = Phase::Ready(package);
            item.in_flight = false;
            Ok(summary(id, &selection))
        })();
        if result.is_err() {
            if let Some(item) = self.held.lock().get_mut(id).filter(|item| &item.owner == caller) {
                item.in_flight = false;
                if matches!(item.phase, Phase::Running) { item.phase = Phase::Failed; }
            }
        }
        result
    }
    pub(crate) fn cancel_prepare(&self, caller: &CallerIdentity, id: &str) -> Result<CancelPrepareSummary, SafeError> {
        (self.owner_check)(caller)?;
        let mut held = self.held.lock();
        let item = transaction_mut(&mut held, caller, id)?;
        // Failed/cancelled cancellation is idempotent; a ready package is dropped
        // and its private files removed. In-flight IO cleans up as it unwinds.
        item.cancelled.store(true, Ordering::SeqCst);
        item.phase = Phase::Cancelled;
        Ok(CancelPrepareSummary { transaction_id: id.to_owned(), cancelled: true })
    }
    /// Rust-only handoff. Revalidates metadata and bytes on the same retained file
    /// object; then cancellation, expiry and the short consumer admission share
    /// the transaction lock. The consumer must not re-enter this service, perform
    /// long blocking work here, or claim install eligibility without its manifest.
    /// No VerifiedPackage reference can escape this borrow or be built from IPC.
    pub(crate) fn with_verified_package<T>(&self, caller: &CallerIdentity, id: &str, consume: impl FnOnce(&VerifiedPackage) -> Result<T, SafeError>) -> Result<T, SafeError> {
        (self.owner_check)(caller)?;
        let (package, token, selection) = {
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            if item.in_flight { return Err(error("HISTORY_PREPARE_BUSY")); }
            let Phase::Ready(package) = &item.phase else { return Err(error("HISTORY_PREPARE_NOT_READY")); };
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
            if self.catalog.revalidate_selection(caller, &token)? != selection { return Err(error("HISTORY_SELECTION_CHANGED")); }
            package.revalidate(&check)?;
            let mut held = self.held.lock();
            let item = transaction_mut(&mut held, caller, id)?;
            state_check(item, (self.clock)())?;
            (self.owner_check)(caller)?;
            self.catalog.resolve_selection(caller, &token)?;
            let Phase::Ready(current) = &item.phase else { return Err(error("HISTORY_PREPARE_NOT_READY")); };
            if !Arc::ptr_eq(current, &package) { return Err(error("HISTORY_PACKAGE_CHANGED")); }
            consumer_admitted = true;
            consume(&package)
        })();
        // Release the retained temporary package before releasing its budget.
        drop(package);
        if let Some(item) = self.held.lock().get_mut(id).filter(|item| &item.owner == caller) {
            item.in_flight = false;
            if result.is_err() && !consumer_admitted && matches!(item.phase, Phase::Ready(_)) { item.phase = Phase::Failed; }
        }
        result
    }
}
fn summary(id: &str, selection: &SelectionMetadata) -> PreparedPackageSummary {
    PreparedPackageSummary { transaction_id: id.to_owned(), version: selection.version().to_owned(), verification: "publisher-verified", install_ready: false, blocked_reason: "PACKAGE_IDENTITY_UNVERIFIED" }
}
fn transaction_mut<'a>(held: &'a mut HashMap<String, Transaction>, caller: &CallerIdentity, id: &str) -> Result<&'a mut Transaction, SafeError> {
    if id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) { return Err(error("HISTORY_PREPARE_UNKNOWN")); }
    held.get_mut(id).filter(|item| &item.owner == caller).ok_or_else(|| error("HISTORY_PREPARE_UNKNOWN"))
}
fn state_check(item: &mut Transaction, now: Instant) -> Result<(), SafeError> {
    if item.cancelled.load(Ordering::SeqCst) || matches!(item.phase, Phase::Cancelled) { return Err(error("HISTORY_PREPARE_CANCELLED")); }
    if now >= item.expires {
        if !item.in_flight { item.phase = Phase::Failed; }
        return Err(error("HISTORY_PREPARE_EXPIRED"));
    }
    if matches!(item.phase, Phase::Failed) { return Err(error("HISTORY_PREPARE_FAILED")); }
    Ok(())
}
fn read_asset(source: &dyn AssetSource, asset: &BoundAsset, limit: u64, mut output: Option<&mut std::fs::File>, check: &dyn Fn() -> Result<(), SafeError>) -> Result<Vec<u8>, SafeError> {
    check()?;
    if asset.size() == 0 || asset.size() > limit { return Err(error("HISTORY_DOWNLOAD_TOO_LARGE")); }
    let mut response = source.open(asset, check)?;
    check()?;
    if response.content_length.is_some_and(|size| size != asset.size()) { return Err(error("HISTORY_SIZE_MISMATCH")); }
    let mut bytes = Vec::with_capacity(asset.size().min(1024 * 1024) as usize);
    let mut buffer = [0u8; 64 * 1024];
    loop {
        check()?;
        let remaining = asset.size().saturating_sub(bytes.len() as u64);
        let maximum = buffer.len().min(remaining as usize + 1);
        let count = response.body.read(&mut buffer[..maximum]).map_err(|_| error("HISTORY_NETWORK_UNAVAILABLE"))?;
        check()?;
        if count == 0 { break; }
        if bytes.len() as u64 + count as u64 > asset.size() { return Err(error("HISTORY_SIZE_MISMATCH")); }
        if let Some(file) = output.as_mut() { file.write_all(&buffer[..count]).map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?; }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if bytes.len() as u64 != asset.size() { return Err(error("HISTORY_SIZE_MISMATCH")); }
    if let Some(file) = output { file.sync_all().map_err(|_| error("HISTORY_STORAGE_UNAVAILABLE"))?; }
    check()?;
    Ok(bytes)
}
