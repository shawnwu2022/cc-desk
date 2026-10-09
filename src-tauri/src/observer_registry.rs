//! Run-scoped observer authentication; no PTY/process-control or filesystem authority.
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use axum::http::HeaderMap;
use parking_lot::Mutex as SyncMutex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(crate) const MAX_OBSERVER_PAYLOAD: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ObserverRun {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ObserverSource {
    ClaudeHook,
    Unknown,
}

#[derive(Clone)]
pub(crate) struct ObserverBinding {
    pub(crate) run: ObserverRun,
    pub(crate) capability: String,
    pub(crate) source: ObserverSource,
}

impl std::fmt::Debug for ObserverBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ObserverBinding(<redacted>)")
    }
}

#[derive(Clone, PartialEq)]
pub(crate) struct ValidatedObserverEvent {
    pub(crate) run: ObserverRun,
    pub(crate) event_id: String,
    pub(crate) source: ObserverSource,
    pub(crate) event: Value,
}

impl std::fmt::Debug for ValidatedObserverEvent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ValidatedObserverEvent(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ObserverAccept {
    Accepted(ValidatedObserverEvent),
    Duplicate,
}

struct ObserverSlot {
    capability: String,
    source: ObserverSource,
    seen: HashSet<String>,
    delivery: ObserverDelivery,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
}

pub(crate) struct ObserverRegistry {
    slots: SyncMutex<HashMap<ObserverRun, ObserverSlot>>,
    capacity: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ObserverDelivery {
    pub(crate) window_label: String,
    pub(crate) legacy_pty: Option<String>,
}
impl ObserverDelivery {
    pub(crate) fn native(window_label: &str) -> Self {
        Self {
            window_label: window_label.into(),
            legacy_pty: None,
        }
    }
    pub(crate) fn legacy(pty_id: &str) -> Self {
        Self {
            window_label: "main".into(),
            legacy_pty: Some(pty_id.into()),
        }
    }
}

/// Owns observation authority only, never a process handle. Dropping a stale lease
/// cannot revoke a newer binding for the same run/generation.
pub(crate) struct ObserverLease {
    registry: Arc<ObserverRegistry>,
    binding: ObserverBinding,
}
impl ObserverLease {
    pub(crate) fn binding(&self) -> &ObserverBinding {
        &self.binding
    }
    pub(crate) fn revoke(&self) {
        self.registry.revoke(&self.binding);
    }
}
impl Drop for ObserverLease {
    fn drop(&mut self) {
        self.revoke();
    }
}
impl std::fmt::Debug for ObserverLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ObserverLease(<redacted>)")
    }
}

impl ObserverRegistry {
    pub(crate) const MAX_SEEN_EVENTS: usize = 1024;

    pub(crate) fn new() -> Self {
        Self::with_capacity(4096)
    }
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: SyncMutex::new(HashMap::new()),
            capacity,
        }
    }

    #[cfg(test)]
    pub(crate) fn mint(
        &self,
        run: ObserverRun,
        source: ObserverSource,
    ) -> Result<ObserverBinding, SafeError> {
        self.attach(run, uuid::Uuid::new_v4().simple().to_string(), source)
    }

    #[cfg(test)]
    pub(crate) fn attach(
        &self,
        run: ObserverRun,
        capability: String,
        source: ObserverSource,
    ) -> Result<ObserverBinding, SafeError> {
        self.insert(
            run,
            capability,
            source,
            ObserverDelivery::native("main"),
            Arc::new(|| true),
        )
    }

    pub(crate) fn lease(
        self: &Arc<Self>,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ObserverLease, SafeError> {
        if !authorize() {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        // Prune revoked bindings without invoking external authorization under a lock.
        let candidates: Vec<_> = self
            .slots
            .lock()
            .iter()
            .map(|(run, slot)| {
                (
                    ObserverBinding {
                        run: run.clone(),
                        capability: slot.capability.clone(),
                        source: slot.source,
                    },
                    slot.authorize.clone(),
                )
            })
            .collect();
        for (binding, active) in candidates {
            if !active() {
                self.revoke(&binding);
            }
        }
        let binding = self.insert(
            run,
            uuid::Uuid::new_v4().simple().to_string(),
            ObserverSource::ClaudeHook,
            delivery,
            authorize,
        )?;
        let lease = ObserverLease {
            registry: self.clone(),
            binding,
        };
        self.check_binding(lease.binding())?;
        Ok(lease)
    }

    fn insert(
        &self,
        run: ObserverRun,
        capability: String,
        source: ObserverSource,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<ObserverBinding, SafeError> {
        validate_observer_run(&run)?;
        validate_capability(&capability)?;
        if source != ObserverSource::ClaudeHook {
            return Err(error("OBSERVER_SOURCE_UNSUPPORTED"));
        }

        let mut slots = self.slots.lock();
        if slots
            .get(&run)
            .is_some_and(|slot| slot.capability == capability && slot.source == source)
        {
            return Ok(ObserverBinding {
                run,
                capability,
                source,
            });
        }
        if !slots.contains_key(&run) && slots.len() >= self.capacity {
            return Err(error("OBSERVER_BINDING_CAPACITY"));
        }
        let previous = slots.insert(
            run.clone(),
            ObserverSlot {
                capability: capability.clone(),
                source,
                seen: HashSet::new(),
                delivery,
                authorize,
            },
        );
        drop(slots);
        drop(previous);
        Ok(ObserverBinding {
            run,
            capability,
            source,
        })
    }

    pub(crate) fn accept_event(
        &self,
        binding: &ObserverBinding,
        event_id: &str,
        payload: &[u8],
    ) -> Result<ObserverAccept, SafeError> {
        validate_observer_run(&binding.run)?;
        if binding.source != ObserverSource::ClaudeHook {
            return Err(error("OBSERVER_SOURCE_UNSUPPORTED"));
        }
        validate_event_id(event_id)?;
        if payload.len() > MAX_OBSERVER_PAYLOAD {
            return Err(error("OBSERVER_PAYLOAD_TOO_LARGE"));
        }

        self.check_binding(binding)?;
        let event: Value =
            serde_json::from_slice(payload).map_err(|_| error("OBSERVER_INVALID_EVENT"))?;
        let event_name = event
            .as_object()
            .and_then(|object| object.get("hook_event_name"))
            .and_then(Value::as_str)
            .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
        if !SUPPORTED_OBSERVER_EVENTS.contains(&event_name) {
            return Err(error("OBSERVER_INVALID_EVENT"));
        }

        let event = sanitize_event(event)?;
        self.check_binding(binding)?;
        let mut slots = self.slots.lock();
        let slot = slots
            .get_mut(&binding.run)
            .ok_or_else(|| error("OBSERVER_FORBIDDEN"))?;
        if !same_capability(&slot.capability, &binding.capability) || slot.source != binding.source
        {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        if slot.seen.contains(event_id) {
            return Ok(ObserverAccept::Duplicate);
        }
        if slot.seen.len() >= Self::MAX_SEEN_EVENTS {
            return Err(error("OBSERVER_EVENT_CAPACITY"));
        }
        slot.seen.insert(event_id.to_string());

        Ok(ObserverAccept::Accepted(ValidatedObserverEvent {
            run: binding.run.clone(),
            event_id: event_id.to_string(),
            source: binding.source,
            event,
        }))
    }

    pub(crate) fn check_binding(&self, binding: &ObserverBinding) -> Result<(), SafeError> {
        let authorize = {
            let slots = self.slots.lock();
            let slot = slots
                .get(&binding.run)
                .ok_or_else(|| error("OBSERVER_FORBIDDEN"))?;
            if !same_capability(&slot.capability, &binding.capability)
                || slot.source != binding.source
            {
                return Err(error("OBSERVER_FORBIDDEN"));
            }
            slot.authorize.clone()
        };
        if !authorize() {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        Ok(())
    }

    pub(crate) fn delivery(
        &self,
        binding: &ObserverBinding,
    ) -> Result<ObserverDelivery, SafeError> {
        self.check_binding(binding)?;
        let slots = self.slots.lock();
        let slot = slots
            .get(&binding.run)
            .ok_or_else(|| error("OBSERVER_FORBIDDEN"))?;
        if !same_capability(&slot.capability, &binding.capability) {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        Ok(slot.delivery.clone())
    }
    fn revoke(&self, binding: &ObserverBinding) {
        let removed = {
            let mut slots = self.slots.lock();
            if slots
                .get(&binding.run)
                .is_some_and(|slot| same_capability(&slot.capability, &binding.capability))
            {
                slots.remove(&binding.run)
            } else {
                None
            }
        };
        drop(removed); // external owner destructors never run under the registry lock
    }
}

pub(crate) fn parse_observer_headers(
    headers: &HeaderMap,
) -> Result<(ObserverBinding, String), SafeError> {
    let run_id = observer_header(headers, "x-cc-desk-run")?.to_string();
    let generation_text = observer_header(headers, "x-cc-desk-generation")?;
    let generation = generation_text
        .parse::<u32>()
        .map_err(|_| SafeError::invalid("generation"))?;
    if generation == 0 || generation.to_string() != generation_text {
        return Err(SafeError::invalid("generation"));
    }
    let capability = observer_header(headers, "x-cc-desk-capability")?.to_string();
    let event_id = observer_header(headers, "x-cc-desk-event")?.to_string();
    let source = match observer_header(headers, "x-cc-desk-observer-source")? {
        "claude-hook" => ObserverSource::ClaudeHook,
        _ => ObserverSource::Unknown,
    };
    if source == ObserverSource::Unknown {
        return Err(error("OBSERVER_SOURCE_UNSUPPORTED"));
    }

    let run = ObserverRun { run_id, generation };
    validate_observer_run(&run)?;
    validate_capability(&capability)?;
    validate_event_id(&event_id)?;
    Ok((
        ObserverBinding {
            run,
            capability,
            source,
        },
        event_id,
    ))
}

fn observer_header<'a>(headers: &'a HeaderMap, name: &'static str) -> Result<&'a str, SafeError> {
    if headers.get_all(name).iter().count() != 1 {
        return Err(SafeError::invalid("observerHeaders"));
    }
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SafeError::invalid("observerHeaders"))
}

const SUPPORTED_OBSERVER_EVENTS: [&str; 13] = [
    "SessionStart",
    "SessionEnd",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
];

fn validate_observer_run(run: &ObserverRun) -> Result<(), SafeError> {
    if run.generation == 0
        || run.run_id.is_empty()
        || run.run_id.len() > 128
        || run.run_id.chars().any(char::is_control)
    {
        return Err(SafeError::invalid("run"));
    }
    Ok(())
}

fn validate_capability(value: &str) -> Result<(), SafeError> {
    if !(32..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(SafeError::invalid("capability"));
    }
    Ok(())
}

fn validate_event_id(value: &str) -> Result<(), SafeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        return Err(SafeError::invalid("eventId"));
    }
    Ok(())
}

/// Keep observation metadata only. Prompt, output, error text, env and unknown fields
/// are deliberately discarded before any frontend/publication object is created.
fn sanitize_event(event: Value) -> Result<Value, SafeError> {
    let object = event
        .as_object()
        .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
    let mut safe = serde_json::Map::new();
    safe.insert("hook_event_name".into(), object["hook_event_name"].clone());
    for name in ["session_id", "cwd", "model", "source", "notification_type"] {
        if let Some(value) = object.get(name) {
            let text = value
                .as_str()
                .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
            let limit = if name == "cwd" { 4096 } else { 256 };
            if text.is_empty() || text.len() > limit || text.chars().any(char::is_control) {
                return Err(error("OBSERVER_INVALID_EVENT"));
            }
            safe.insert(name.into(), value.clone());
        }
    }
    Ok(Value::Object(safe))
}

fn same_capability(left: &str, right: &str) -> bool {
    left.len() == right.len()
        && left
            .bytes()
            .zip(right.bytes())
            .fold(0u8, |different, (a, b)| different | (a ^ b))
            == 0
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Probe {
        registry: std::sync::Weak<ObserverRegistry>,
        unlocked: Arc<AtomicBool>,
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            self.unlocked.store(
                self.registry.upgrade().unwrap().slots.try_lock().is_some(),
                Ordering::SeqCst,
            );
        }
    }
    #[test]
    fn replaced_authorization_destructor_runs_outside_observer_lock() {
        let registry = Arc::new(ObserverRegistry::new());
        let unlocked = Arc::new(AtomicBool::new(false));
        let probe = Probe {
            registry: Arc::downgrade(&registry),
            unlocked: unlocked.clone(),
        };
        let run = ObserverRun {
            run_id: "drop-probe".into(),
            generation: 1,
        };
        let first = registry
            .lease(
                run.clone(),
                ObserverDelivery::native("main"),
                Arc::new(move || {
                    let _ = &probe;
                    true
                }),
            )
            .unwrap();
        let second = registry
            .lease(run, ObserverDelivery::native("main"), Arc::new(|| true))
            .unwrap();
        assert!(unlocked.load(Ordering::SeqCst));
        drop(first);
        drop(second);
    }
}
