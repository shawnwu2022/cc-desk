//! Backend-only capabilities. Public SourceRef values are references, not permissions.
use super::scoped_fs::{ReadResult, Root};
use super::wire::{
    ProjectionResult, ReadRequest, ResourceItem, ResourceKind, ScopeTarget, SourceBasis, SourceRef,
};
use crate::cli::types::{CliKind, SafeError, WireU64};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Owner {
    pub instance: String,
    pub window: String,
    pub epoch: WireU64,
}
pub(crate) struct Grant {
    pub owner: Owner,
    pub cli: CliKind,
    pub profile_id: String,
    pub profile_revision: WireU64,
    pub target: ScopeTarget,
    pub basis: SourceBasis,
    pub root: Root,
    pub project: Option<Root>,
    pub project_paths: Vec<PathBuf>,
    pub user_config: Option<Root>,
    pub check: Arc<dyn Fn() -> ReadResult<()> + Send + Sync>,
}

struct Scope {
    grant: Grant,
    source: SourceRef,
    history_generation: std::sync::atomic::AtomicU64,
}
// Only consecutive History pages retain an observation. This is never a
// cross-request freshness cache or additional filesystem authority.
struct HistorySnapshot {
    source: SourceRef,
    request_epoch: WireU64,
    observed_at: WireU64,
    created: std::time::Instant,
    generation: u64,
    retained_bytes: usize,
    items: Vec<ResourceItem>,
    metadata_incomplete: Option<bool>,
    read_failures: Vec<&'static str>,
}
const HISTORY_SNAPSHOT_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const HISTORY_SNAPSHOT_BYTES: usize = 32 * 1024 * 1024;
const HISTORY_SNAPSHOT_TTL: std::time::Duration = std::time::Duration::from_secs(5);

struct State {
    next_epoch: u64,
    scopes: std::collections::BTreeMap<String, Arc<Scope>>,
}
pub(crate) struct ScopeRegistry {
    state: std::sync::Mutex<State>,
    capacity: usize,
    reading: std::sync::atomic::AtomicUsize,
    history_pages: std::sync::Mutex<Vec<Arc<HistorySnapshot>>>,
}
impl Grant {
    fn current(&self) -> ReadResult<()> {
        (self.check)()?;
        self.root.current()?;
        if let Some(root) = &self.project {
            root.current()?;
        }
        if let Some(root) = &self.user_config {
            root.current()?;
        }
        Ok(())
    }
    fn same(&self, other: &Self) -> bool {
        self.owner == other.owner
            && self.cli == other.cli
            && self.target == other.target
            && self.profile_id == other.profile_id
            && self.profile_revision == other.profile_revision
            && self.basis == other.basis
            && self.root.key() == other.root.key()
            && self.project.as_ref().map(Root::key) == other.project.as_ref().map(Root::key)
            && self.user_config.as_ref().map(Root::key) == other.user_config.as_ref().map(Root::key)
            && self.project_paths == other.project_paths
    }
}
impl ScopeRegistry {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            state: std::sync::Mutex::new(State {
                next_epoch: 0,
                scopes: Default::default(),
            }),
            capacity: capacity.clamp(1, 256),
            reading: Default::default(),
            history_pages: Default::default(),
        }
    }
    pub(crate) fn register(&self, grant: Grant) -> Result<SourceRef, SafeError> {
        grant.target.validate()?;
        grant.current().map_err(safe)?;
        // Filesystem/authorization work and handle destruction must not hold the global map lock.
        let candidates: Vec<_> = self
            .state
            .lock()
            .map_err(|_| safe("SCOPE_UNAVAILABLE"))?
            .scopes
            .values()
            .cloned()
            .collect();
        let invalid: Vec<_> = candidates
            .iter()
            .filter(|s| s.grant.current().is_err())
            .cloned()
            .collect();
        let (selected, retired) = {
            let mut state = self.state.lock().map_err(|_| safe("SCOPE_UNAVAILABLE"))?;
            let mut retired = vec![];
            for candidate in invalid {
                if state
                    .scopes
                    .get(&candidate.source.scope_id)
                    .is_some_and(|s| Arc::ptr_eq(s, &candidate))
                {
                    retired.extend(state.scopes.remove(&candidate.source.scope_id));
                }
            }
            if let Some(scope) = state.scopes.values().find(|s| s.grant.same(&grant)) {
                (Ok(scope.clone()), retired)
            } else if state.scopes.len() >= self.capacity {
                (Err(safe("SCOPE_CAPACITY")), retired)
            } else {
                let epoch = state
                    .next_epoch
                    .checked_add(1)
                    .ok_or_else(|| safe("SCOPE_EPOCH_EXHAUSTED"))?;
                let source = SourceRef {
                    scope_id: format!("scope-{epoch}"),
                    instance_id: grant.owner.instance.clone(),
                    cli: grant.cli,
                    source_root_key: grant.root.key().into(),
                    identity_epoch: WireU64::parse(&epoch.to_string())?,
                    profile_id: grant.profile_id.clone(),
                    profile_revision: grant.profile_revision,
                    target: grant.target.clone(),
                    basis: grant.basis,
                };
                // Use the same strict wire checks as readers; a backend bug must not mint unusable authority.
                ReadRequest {
                    source: source.clone(),
                    resource_kind: super::wire::ResourceKind::History,
                    request_epoch: WireU64::parse("0")?,
                    query: None,
                    session_id: None,
                    limit: 1,
                    offset: 0,
                }
                .validate()?;
                let scope = Arc::new(Scope {
                    grant,
                    source,
                    history_generation: Default::default(),
                });
                state.next_epoch = epoch;
                state
                    .scopes
                    .insert(scope.source.scope_id.clone(), scope.clone());
                (Ok(scope), retired)
            }
        };
        drop(retired);
        let selected = selected?;
        selected.grant.current().map_err(safe)?;
        Ok(selected.source.clone())
    }
    pub(crate) fn read(
        &self,
        owner: &Owner,
        request: &ReadRequest,
    ) -> Result<ProjectionResult, SafeError> {
        use super::wire::ProjectionState;
        use std::sync::atomic::Ordering;
        request.validate()?;
        let scope = self
            .state
            .lock()
            .map_err(|_| safe("SCOPE_UNAVAILABLE"))?
            .scopes
            .get(&request.source.scope_id)
            .cloned()
            .ok_or_else(|| safe("SCOPE_UNKNOWN"))?;
        if &scope.grant.owner != owner {
            return Err(safe("FORBIDDEN"));
        }
        if scope.source != request.source {
            return Err(safe("SCOPE_STALE"));
        }
        // Two scans per runtime: a blocked native filesystem cannot allocate an unbounded queue of scans.
        self.reading
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                if n < 2 {
                    Some(n + 1)
                } else {
                    None
                }
            })
            .map_err(|_| safe("SOURCE_BUSY"))?;
        struct Permit<'a>(&'a std::sync::atomic::AtomicUsize);
        impl Drop for Permit<'_> {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _permit = Permit(&self.reading);
        let g = &scope.grant;
        let history_generation = if request.resource_kind == ResourceKind::History {
            Some(if request.offset == 0 {
                scope
                    .history_generation
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                        value.checked_add(1)
                    })
                    .map_err(|_| safe("SCOPE_EPOCH_EXHAUSTED"))?
                    + 1
            } else {
                scope.history_generation.load(Ordering::SeqCst)
            })
        } else {
            None
        };
        // Claim the generation before any possibly blocking authority check.
        (scope.grant.check)().map_err(safe)?;
        let continuation = if request.resource_kind == ResourceKind::History && request.offset != 0
        {
            let pages = self
                .history_pages
                .lock()
                .map_err(|_| safe("SCOPE_UNAVAILABLE"))?;
            pages
                .iter()
                .find(|page| {
                    page.source == request.source
                        && page.request_epoch == request.request_epoch
                        && page.created.elapsed() <= HISTORY_SNAPSHOT_TTL
                        && Some(page.generation) == history_generation
                })
                .cloned()
        } else {
            None
        };
        let mut fresh_snapshot = None;
        let mut response_generation = history_generation;
        let mut response = if let Some(snapshot) = continuation {
            // The grant and each held root are rechecked even for cached bytes.
            // Its original observation time is retained, never made fresh here.
            response_generation = Some(snapshot.generation);
            match g.current() {
                Ok(()) => history_page(&snapshot, request),
                Err("FORBIDDEN" | "SCOPE_REVOKED") => return Err(safe("SCOPE_REVOKED")),
                Err(code) => unavailable(&scope.source, request, code)?,
            }
        } else if request.resource_kind == ResourceKind::History && request.offset != 0 {
            unavailable(&scope.source, request, "SOURCE_SNAPSHOT_EXPIRED")?
        } else {
            if request.resource_kind == ResourceKind::History {
                // An explicit offset-zero load always scans anew, including reuse
                // of the same epoch. Retire any older receipt for this scope.
                self.retire_history_pages(&scope, history_generation.unwrap())?;
            }
            let catalog = super::catalog::Catalog {
                cli: g.cli,
                root: &g.root,
                project: g.project.as_ref(),
                project_paths: &g.project_paths,
                user_config: g.user_config.as_ref(),
                check: g.check.as_ref(),
            };
            let mut budget = super::scoped_fs::Budget::new(super::scoped_fs::Limits::default());
            let result = super::catalog::read(
                &catalog,
                &super::catalog::Options {
                    kind: request.resource_kind,
                    query: request.query.as_deref(),
                    session_id: request.session_id.as_deref(),
                },
                &mut budget,
            );
            // Authority errors never become successfully authorized empty sources.
            (g.check)().map_err(safe)?;
            let result = result.and_then(|items| {
                g.current()?;
                Ok(items)
            });
            match result {
                Ok(items) => {
                    let mut snapshot = HistorySnapshot {
                        source: scope.source.clone(),
                        request_epoch: request.request_epoch,
                        observed_at: observed_at()?,
                        created: std::time::Instant::now(),
                        generation: history_generation.unwrap_or(0),
                        retained_bytes: 0,
                        items,
                        metadata_incomplete: (request.resource_kind == ResourceKind::History
                            && budget.history_metadata_incomplete())
                        .then_some(true),
                        read_failures: if request.resource_kind == ResourceKind::History {
                            budget.history_read_failures()
                        } else {
                            vec![]
                        },
                    };
                    snapshot.retained_bytes =
                        history_snapshot_bytes(&snapshot.items, snapshot.items.capacity())
                            .saturating_add(std::mem::size_of::<HistorySnapshot>())
                            .saturating_add(source_heap_bytes(&snapshot.source))
                            .saturating_add(
                                snapshot.read_failures.capacity() * std::mem::size_of::<&str>(),
                            );
                    let snapshot = Arc::new(snapshot);
                    let page = history_page(&snapshot, request);
                    if request.resource_kind == ResourceKind::History && page.has_more {
                        // Account retained heap allocations, not only one IPC page.
                        // The original read/entry caps remain independently enforced.
                        if snapshot.retained_bytes > HISTORY_SNAPSHOT_BYTES {
                            unavailable(&scope.source, request, "SOURCE_RESPONSE_TOO_LARGE")?
                        } else {
                            fresh_snapshot = Some(snapshot);
                            page
                        }
                    } else {
                        page
                    }
                }
                Err("FORBIDDEN" | "SCOPE_REVOKED") => return Err(safe("SCOPE_REVOKED")),
                Err(code) => unavailable(&scope.source, request, code)?,
            }
        };
        // Bound encoded IPC, not just input file bytes; JSON escaping can multiply size.
        let encoded = serde_json::to_vec(&response).map_err(|_| safe("SOURCE_INVALID"))?;
        if encoded.len() > 2 * 1024 * 1024 {
            response.state = ProjectionState::Unavailable;
            response.reason = Some("SOURCE_RESPONSE_TOO_LARGE".into());
            response.items.clear();
            response.has_more = false;
            response.history_metadata_incomplete = None;
            response.history_read_failures.clear();
        }
        (g.check)().map_err(safe)?;
        if response.state == ProjectionState::Ready {
            if let Err(code) = g.current() {
                return Err(safe(code));
            }
        }
        if response_generation
            .is_some_and(|generation| generation != scope.history_generation.load(Ordering::SeqCst))
        {
            // A late older scan cannot publish or replace the newer load's receipt.
            return unavailable(&scope.source, request, "SOURCE_CHANGED");
        }
        if response.state == ProjectionState::Ready && response.has_more {
            if let Some(snapshot) = fresh_snapshot {
                let mut pages = self
                    .history_pages
                    .lock()
                    .map_err(|_| safe("SCOPE_UNAVAILABLE"))?;
                if snapshot.generation != scope.history_generation.load(Ordering::SeqCst) {
                    return unavailable(&scope.source, request, "SOURCE_CHANGED");
                }
                pages.retain(|page| {
                    page.created.elapsed() <= HISTORY_SNAPSHOT_TTL && page.source != snapshot.source
                });
                let mut retained = pages.iter().fold(0usize, |size, page| {
                    size.saturating_add(page.retained_bytes)
                });
                while pages.len() >= self.capacity
                    || retained.saturating_add(snapshot.retained_bytes)
                        > HISTORY_SNAPSHOT_TOTAL_BYTES
                {
                    let evicted = pages.remove(0);
                    retained = retained.saturating_sub(evicted.retained_bytes);
                }
                pages.push(snapshot);
            }
        } else if request.resource_kind == ResourceKind::History {
            self.history_pages
                .lock()
                .map_err(|_| safe("SCOPE_UNAVAILABLE"))?
                .retain(|page| {
                    page.source != request.source
                        || page.request_epoch != request.request_epoch
                        || response_generation.is_none_or(|generation| page.generation > generation)
                });
        }
        Ok(response)
    }
    fn retire_history_pages(&self, scope: &Scope, generation: u64) -> Result<(), SafeError> {
        let mut pages = self
            .history_pages
            .lock()
            .map_err(|_| safe("SCOPE_UNAVAILABLE"))?;
        if generation
            != scope
                .history_generation
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Err(safe("SOURCE_CHANGED"));
        }
        pages.retain(|page| page.source != scope.source || page.generation > generation);
        Ok(())
    }
}
fn source_heap_bytes(source: &SourceRef) -> usize {
    let base = [
        source.scope_id.capacity(),
        source.instance_id.capacity(),
        source.source_root_key.capacity(),
        source.profile_id.capacity(),
    ]
    .into_iter()
    .fold(0usize, usize::saturating_add);
    match &source.target {
        ScopeTarget::Profile {
            profile_id,
            project_id,
            ..
        } => base
            .saturating_add(profile_id.capacity())
            .saturating_add(project_id.as_ref().map_or(0, String::capacity)),
        ScopeTarget::Run { run_id, .. } => base.saturating_add(run_id.capacity()),
    }
}
fn observed_at() -> Result<WireU64, SafeError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| safe("CLOCK_UNAVAILABLE"))?
        .as_millis();
    WireU64::parse(&now.to_string())
}
fn unavailable(
    source: &SourceRef,
    request: &ReadRequest,
    code: &str,
) -> Result<ProjectionResult, SafeError> {
    Ok(ProjectionResult {
        source: source.clone(),
        resource_kind: request.resource_kind,
        request_epoch: request.request_epoch,
        observed_at: observed_at()?,
        state: super::wire::ProjectionState::Unavailable,
        reason: Some(code.into()),
        items: vec![],
        has_more: false,
        history_metadata_incomplete: None,
        history_read_failures: vec![],
    })
}
fn history_page(snapshot: &HistorySnapshot, request: &ReadRequest) -> ProjectionResult {
    let end = (request.offset as usize).saturating_add(request.limit as usize);
    ProjectionResult {
        source: snapshot.source.clone(),
        resource_kind: request.resource_kind,
        request_epoch: snapshot.request_epoch,
        observed_at: snapshot.observed_at,
        state: super::wire::ProjectionState::Ready,
        reason: None,
        items: snapshot
            .items
            .iter()
            .skip(request.offset as usize)
            .take(request.limit as usize)
            .cloned()
            .collect(),
        has_more: snapshot.items.len() > end,
        history_metadata_incomplete: snapshot.metadata_incomplete,
        history_read_failures: snapshot.read_failures.clone(),
    }
}
fn history_snapshot_bytes(items: &[ResourceItem], capacity: usize) -> usize {
    items.iter().fold(
        capacity.saturating_mul(std::mem::size_of::<ResourceItem>()),
        |size, item| {
            let ResourceItem::Session {
                session_key,
                native_session_id,
                title,
                cwd,
                updated_at,
                ..
            } = item
            else {
                return usize::MAX;
            };
            [
                session_key.capacity(),
                native_session_id.capacity(),
                title.capacity(),
                cwd.as_ref().map_or(0, String::capacity),
                updated_at.as_ref().map_or(0, String::capacity),
            ]
            .into_iter()
            .fold(size, usize::saturating_add)
        },
    )
}

fn safe(code: &str) -> SafeError {
    SafeError {
        code: code.into(),
        field: None,
        index: None,
        retryable: false,
    }
}
#[cfg(test)]
#[path = "../../tests/native_cli_projection_registry.rs"]
mod tests;
