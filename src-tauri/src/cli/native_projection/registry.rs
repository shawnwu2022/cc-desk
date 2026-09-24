//! Backend-only capabilities. Public SourceRef values are references, not permissions.
use super::scoped_fs::{ReadResult, Root};
use super::wire::{ProjectionResult, ReadRequest, ScopeTarget, SourceBasis, SourceRef};
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
}
struct State {
    next_epoch: u64,
    scopes: std::collections::BTreeMap<String, Arc<Scope>>,
}
pub(crate) struct ScopeRegistry {
    state: std::sync::Mutex<State>,
    capacity: usize,
    reading: std::sync::atomic::AtomicUsize,
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
                let scope = Arc::new(Scope { grant, source });
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
        (scope.grant.check)().map_err(safe)?;
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
        // Authority errors are never converted into an apparent successfully authorized empty source.
        (g.check)().map_err(safe)?;
        let result = result.and_then(|items| {
            g.current()?;
            Ok(items)
        });
        let (state, reason, items, has_more) = match result {
            Ok(items) => {
                let end = (request.offset as usize).saturating_add(request.limit as usize);
                let has_more = items.len() > end;
                let page = items
                    .into_iter()
                    .skip(request.offset as usize)
                    .take(request.limit as usize)
                    .collect();
                (ProjectionState::Ready, None, page, has_more)
            }
            Err("FORBIDDEN" | "SCOPE_REVOKED") => return Err(safe("SCOPE_REVOKED")),
            Err(code) => (
                ProjectionState::Unavailable,
                Some(code.to_string()),
                vec![],
                false,
            ),
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| safe("CLOCK_UNAVAILABLE"))?
            .as_millis();
        let mut response = ProjectionResult {
            source: scope.source.clone(),
            resource_kind: request.resource_kind,
            request_epoch: request.request_epoch,
            observed_at: WireU64::parse(&now.to_string())?,
            state,
            reason,
            items,
            has_more,
        };
        // Bound encoded IPC, not just input file bytes; JSON escaping can multiply size.
        let encoded = serde_json::to_vec(&response).map_err(|_| safe("SOURCE_INVALID"))?;
        if encoded.len() > 2 * 1024 * 1024 {
            response.state = ProjectionState::Unavailable;
            response.reason = Some("SOURCE_RESPONSE_TOO_LARGE".into());
            response.items.clear();
            response.has_more = false;
        }
        (g.check)().map_err(safe)?;
        if response.state == ProjectionState::Ready {
            if let Err(code) = g.current() {
                return Err(safe(code));
            }
        }
        Ok(response)
    }
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
