use cc_desk_remote_record::grant::{DataRole, SessionScope};
use cc_desk_remote_security::{BoundDataAuthority, Role};
use cc_desk_remote_terminal::{MAX_TERMINAL_FEED_BYTES, SemanticSnapshot, TerminalCore};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserverOutputError {
    Authority,
    Projection,
}

/// Output projection only; this is not a source authorization boundary.
/// A trusted native owner must independently bind the original output source
/// to this grant's exact run and feed it from offset zero. Scope arguments
/// alone do not prove where bytes came from. This type has no terminal writer,
/// channel dispatch, or access to control/delegation capabilities.
/// A returned snapshot is owned data, not a revocable handle. Its caller
/// must independently revalidate the original grant and source before use,
/// especially after every await and before emitting a frame.
pub struct ObserverOutput {
    authority: BoundDataAuthority,
    scope: SessionScope,
    terminal: TerminalCore,
    next: u64,
    failed: bool,
}

impl ObserverOutput {
    pub fn new(
        authority: BoundDataAuthority,
        scope: &SessionScope,
        rows: usize,
        columns: usize,
    ) -> Result<Self, ObserverOutputError> {
        if authority.local_role() != Role::Host
            || authority.data_role() != DataRole::Observer
            || authority.scope() != scope
            || authority.control_lease_renew_enabled()
            || authority.data_keepalive_negotiated()
            || authority.check().is_err()
        {
            return Err(ObserverOutputError::Authority);
        }
        let terminal =
            TerminalCore::new(rows, columns).map_err(|_| ObserverOutputError::Projection)?;
        Ok(Self {
            authority,
            scope: scope.clone(),
            terminal,
            next: 0,
            failed: false,
        })
    }

    fn check(&self, scope: &SessionScope) -> Result<(), ObserverOutputError> {
        if self.failed || scope != &self.scope || self.authority.check().is_err() {
            return Err(ObserverOutputError::Authority);
        }
        Ok(())
    }

    pub fn append(
        &mut self,
        scope: &SessionScope,
        offset: u64,
        bytes: &[u8],
    ) -> Result<(), ObserverOutputError> {
        self.check(scope)?;
        // A discontinuous/partially applied stream cannot yield a new snapshot.
        let next = offset.checked_add(bytes.len() as u64);
        if offset != self.next
            || bytes.is_empty()
            || bytes.len() > MAX_TERMINAL_FEED_BYTES
            || next.is_none()
            || self.terminal.feed(bytes).is_err()
        {
            self.failed = true;
            return Err(ObserverOutputError::Projection);
        }
        self.next = next.expect("checked offset");
        self.check(scope)
    }

    pub fn snapshot(
        &mut self,
        scope: &SessionScope,
    ) -> Result<SemanticSnapshot, ObserverOutputError> {
        self.check(scope)?;
        let view = self
            .terminal
            .semantic_snapshot()
            .map_err(|_| ObserverOutputError::Projection)?;
        self.check(scope)?;
        Ok(view)
    }
}
