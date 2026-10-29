//! One backend writer resource owner, not endpoint identity or authorization.
use super::*;
use std::sync::{Mutex, OnceLock, Weak};
const MAX_ACTIVE_WRITERS: usize = 12;
const MAX_LIFETIME_SESSIONS: usize = 4096;
pub(super) struct OwnerActivation;
#[derive(Default)]
struct OwnerRegistry {
    admission: Mutex<Option<Weak<OwnerActivation>>>,
}
static OWNER: OnceLock<OwnerRegistry> = OnceLock::new();
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OwnerError {
    Busy,
    Closed,
    Target,
    Retired,
    Capacity,
    Writer(Reject),
}
pub(super) struct SessionStart {
    pub(super) target: Target,
    pub(super) device: [u8; 32],
    pub(super) writer_epoch: u64,
    pub(super) key: DigestKey,
    pub(super) rows: usize,
    pub(super) columns: usize,
}
pub(super) struct StartedWriter {
    pub(super) input: LocalInputHandle,
    pub(super) output: HostOutputHandle,
    pub(super) peer: Option<view_gate::PeerViewHandle>,
}
struct SessionWriter {
    target: Target,
    writer: LocalWriter,
}
enum GuardSpec {
    View([u8; 32], u16),
    Control(view_gate::control::ControlAdmission),
}
/// Sole owner-managed canonical writer factory. One admission per process;
/// existing non-owner static/local qualification paths remain separate. It does
/// not own a PTY Job/reader or supply persistent identity/enrollment/grants.
pub(super) struct WriterHostOwner {
    host: [u8; 32],
    incarnation: u64,
    activation: Arc<OwnerActivation>,
    budget: HostViewBudget,
    active: Vec<SessionWriter>,
    retired: Vec<[u8; 16]>,
}
// Custody is retained by the actual IO object, including a blocked write and
// during unwinding. No detached JoinHandle is accepted as quiescence evidence.
struct HeldIo<W> {
    inner: W,
    _activation: Arc<OwnerActivation>,
}
impl<W: Write> Write for HeldIo<W> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.inner.write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
impl WriterHostOwner {
    pub(super) fn control_status(
        &self,
        t: &Target,
    ) -> Result<view_gate::rebind::PendingStatus, OwnerError> {
        self.find(t)?.control_status().map_err(OwnerError::Writer)
    }
    pub(super) fn rebind(
        &self,
        t: &Target,
        device: [u8; 32],
        expected_fence: u64,
        admission: view_gate::control::ControlAdmission,
    ) -> Result<view_gate::rebind::PendingRebind, OwnerError> {
        self.find(t)?
            .rebind(device, expected_fence, admission)
            .map_err(OwnerError::Writer)
    }
    pub(super) fn start_controlled(
        &mut self,
        journal: Journal,
        stdin: impl Write + Send + 'static,
        start: SessionStart,
        admission: view_gate::control::ControlAdmission,
    ) -> Result<StartedWriter, OwnerError> {
        self.start_inner(journal, stdin, start, Some(GuardSpec::Control(admission)))
    }
    pub(super) fn start_guarded(
        &mut self,
        journal: Journal,
        stdin: impl Write + Send + 'static,
        start: SessionStart,
        binding: [u8; 32],
        minor: u16,
    ) -> Result<StartedWriter, OwnerError> {
        self.start_inner(journal, stdin, start, Some(GuardSpec::View(binding, minor)))
    }
    pub(super) fn new(host: [u8; 32], incarnation: u64) -> Result<Self, OwnerError> {
        Self::from_registry(OWNER.get_or_init(OwnerRegistry::default), host, incarnation)
    }
    fn from_registry(
        registry: &OwnerRegistry,
        host: [u8; 32],
        incarnation: u64,
    ) -> Result<Self, OwnerError> {
        let mut current = registry.admission.lock().map_err(|_| OwnerError::Busy)?;
        if let Some(old) = current.as_ref() {
            return Err(if old.upgrade().is_some() {
                OwnerError::Busy
            } else {
                OwnerError::Closed
            });
        }
        let activation = Arc::new(OwnerActivation);
        *current = Some(Arc::downgrade(&activation));
        Ok(Self {
            host,
            incarnation,
            budget: HostViewBudget::for_owner(activation.clone()),
            activation,
            active: Vec::new(),
            retired: Vec::new(),
        })
    }
    pub(super) fn start(
        &mut self,
        journal: Journal,
        stdin: impl Write + Send + 'static,
        start: SessionStart,
    ) -> Result<StartedWriter, OwnerError> {
        self.start_inner(journal, stdin, start, None)
    }
    fn start_inner(
        &mut self,
        journal: Journal,
        stdin: impl Write + Send + 'static,
        start: SessionStart,
        binding: Option<GuardSpec>,
    ) -> Result<StartedWriter, OwnerError> {
        let t = &start.target;
        if t.host != self.host
            || t.incarnation != self.incarnation
            || t.run.is_empty()
            || t.run.len() > 128
            || t.run.chars().any(char::is_control)
            || self.active.iter().any(|s| s.target.session == t.session)
        {
            return Err(OwnerError::Target);
        }
        if self.retired.contains(&t.session) {
            return Err(OwnerError::Retired);
        }
        if self.active.len() == MAX_ACTIVE_WRITERS
            || self.active.len() + self.retired.len() >= MAX_LIFETIME_SESSIONS
        {
            return Err(OwnerError::Capacity);
        }
        let canonical = CanonicalStart::new(start.rows, start.columns, &self.budget)
            .map_err(OwnerError::Writer)?;
        let stdin = HeldIo {
            inner: stdin,
            _activation: self.activation.clone(),
        };
        let (writer, output, peer) = match binding {
            Some(spec) => {
                let (binding, minor) = match &spec {
                    GuardSpec::View(binding, minor) => (*binding, *minor),
                    GuardSpec::Control(admission) => (admission.binding, admission.minor),
                };
                let view = view_gate::ViewStart {
                    terminal: canonical,
                    binding,
                    minor,
                };
                let (w, o, p) = match spec {
                    GuardSpec::View(..) => LocalWriter::start_view_guarded(
                        journal,
                        stdin,
                        start.target.clone(),
                        start.device,
                        start.writer_epoch,
                        start.key,
                        view,
                    ),
                    GuardSpec::Control(admission) => LocalWriter::start_control_guarded(
                        journal,
                        stdin,
                        start.target.clone(),
                        start.device,
                        start.writer_epoch,
                        start.key,
                        view_gate::control::ControlStart::new(
                            view,
                            admission.clock,
                            admission.grant,
                        ),
                    ),
                }
                .map_err(OwnerError::Writer)?;
                (w, o, Some(p))
            }
            None => {
                let (w, o) = LocalWriter::start_canonical_terminal(
                    journal,
                    stdin,
                    start.target.clone(),
                    start.device,
                    start.writer_epoch,
                    start.key,
                    canonical,
                )
                .map_err(OwnerError::Writer)?;
                (w, o, None)
            }
        };
        let input = writer.handle();
        self.active.push(SessionWriter {
            target: start.target,
            writer,
        });
        Ok(StartedWriter {
            input,
            output,
            peer,
        })
    }
    fn find(&self, t: &Target) -> Result<&LocalWriter, OwnerError> {
        self.active
            .iter()
            .find(|s| &s.target == t)
            .map(|s| &s.writer)
            .ok_or(OwnerError::Target)
    }
    pub(super) fn capture(&self, t: &Target) -> Result<PendingProjection, OwnerError> {
        self.find(t)?.capture().map_err(OwnerError::Writer)
    }
    pub(super) fn seal(&self, t: &Target) -> Result<(), OwnerError> {
        self.find(t)?.seal();
        Ok(())
    }
    /// Writer join only. Never interprets this as whole runtime retirement or
    /// permits a new generation of the same session ID; tombstones stay bounded.
    pub(super) fn try_reap(&mut self, t: &Target) -> Result<bool, OwnerError> {
        let i = self
            .active
            .iter()
            .position(|s| &s.target == t)
            .ok_or(OwnerError::Target)?;
        if !self.active[i]
            .writer
            .try_join()
            .map_err(OwnerError::Writer)?
        {
            return Ok(false);
        }
        let old = self.active.remove(i);
        self.retired.push(old.target.session);
        Ok(true)
    }
}
impl Drop for WriterHostOwner {
    fn drop(&mut self) {
        for session in &self.active {
            session.writer.seal();
        }
    }
}
#[cfg(all(test, windows))]
#[path = "local_writer_owner_tests.rs"]
mod tests;
