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
    CodexHook,
    OpencodeHook,
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
    codex_replay: crate::codex_observer::ReplayWindow,
    delivery: ObserverDelivery,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    identity_diagnostics: CodexIdentityDiagnostics,
}

/// Receipt diagnostics only, never a second frontend adoption state machine.
#[derive(Default)]
struct CodexIdentityDiagnostics {
    emitted: u8,
    last: Option<(String, uuid::Uuid, bool)>,
    reported: Option<uuid::Uuid>,
}
impl CodexIdentityDiagnostics {
    fn receipt(&mut self, run: &ObserverRun, event: &Value, duplicate: bool) -> Option<String> {
        let kind = event.get("hook_event_name")?.as_str()?;
        if !matches!(kind, "SessionStart" | "UserPromptSubmit" | "Stop") {
            return None;
        }
        let id = uuid::Uuid::parse_str(event.get("session_id")?.as_str()?).ok()?;
        let before = self.reported;
        if !duplicate {
            self.reported = Some(id);
        }
        let signature = (kind.to_owned(), id, duplicate);
        if self.emitted >= 16 || self.last.as_ref() == Some(&signature) {
            return None;
        }
        self.last = Some(signature);
        self.emitted += 1;
        // Only normal Desk-generated routing IDs are printable. Never echo an
        // arbitrary caller string that happened to pass routing validation.
        let safe_run = run
            .run_id
            .strip_prefix("run-")
            .filter(|s| {
                s.len() == 32
                    && s.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .map(|_| run.run_id.as_str())
            .unwrap_or("redacted");
        Some(serde_json::json!({"layer":"backend-receipt","run":safe_run,"generation":run.generation,
            "event":kind,"outcome":if duplicate {"duplicate"} else {"backend-authenticated"},
            "reason":if duplicate {"replay"} else {"auth-shape-replay-passed"},
            "beforeReported":before.map(|v|v.to_string()),"afterReported":self.reported.map(|v|v.to_string()),
            "frontendAdoption":"unknown"}).to_string())
    }
}

pub(crate) struct ObserverRegistry {
    slots: SyncMutex<HashMap<ObserverRun, ObserverSlot>>,
    capacity: usize,
    diagnostic_emitted: std::sync::atomic::AtomicUsize,
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
    resource: Option<tempfile::TempDir>,
}
impl ObserverLease {
    pub(crate) fn keep_resource(&mut self, resource: tempfile::TempDir) {
        self.resource = Some(resource);
    }
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
            diagnostic_emitted: std::sync::atomic::AtomicUsize::new(0),
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
        self.lease_source(run, delivery, authorize, ObserverSource::ClaudeHook)
    }
    pub(crate) fn lease_source(
        self: &Arc<Self>,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
        source: ObserverSource,
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
            source,
            delivery,
            authorize,
        )?;
        let lease = ObserverLease {
            registry: self.clone(),
            binding,
            resource: None,
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
        if !matches!(
            source,
            ObserverSource::ClaudeHook | ObserverSource::CodexHook | ObserverSource::OpencodeHook
        ) {
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
                codex_replay: Default::default(),
                delivery,
                authorize,
                identity_diagnostics: Default::default(),
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
        if !matches!(
            binding.source,
            ObserverSource::ClaudeHook | ObserverSource::CodexHook | ObserverSource::OpencodeHook
        ) {
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

        if binding.source == ObserverSource::OpencodeHook {
            let id = event
                .get("session_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !id.starts_with("ses_")
                || id.len() <= 4
                || id.len() > 256
                || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || !matches!(event_name, "SessionStart" | "Notification")
                || event_id.len() != 17
                || !event_id.starts_with('o')
                || !event_id[1..].bytes().all(|b| b.is_ascii_digit())
            {
                return Err(error("OBSERVER_INVALID_EVENT"));
            }
        }
        let event = if binding.source == ObserverSource::OpencodeHook {
            let mut safe =
                serde_json::json!({"hook_event_name":event_name,"session_id":event["session_id"]});
            if event_name == "Notification" {
                let status = event["notification_type"]
                    .as_str()
                    .filter(|value| {
                        matches!(
                            *value,
                            "opencode_idle"
                                | "opencode_busy"
                                | "opencode_retry"
                                | "opencode_permission"
                                | "opencode_question"
                                | "opencode_error"
                                | "opencode_unknown"
                        )
                    })
                    .unwrap_or("opencode_unknown");
                safe["notification_type"] = status.into();
            }
            safe
        } else if binding.source == ObserverSource::CodexHook {
            crate::codex_observer::metadata(&event)?
        } else {
            sanitize_event(event)?
        };
        self.check_binding(binding)?;
        let mut slots = self.slots.lock();
        let slot = slots
            .get_mut(&binding.run)
            .ok_or_else(|| error("OBSERVER_FORBIDDEN"))?;
        if !same_capability(&slot.capability, &binding.capability) || slot.source != binding.source
        {
            return Err(error("OBSERVER_FORBIDDEN"));
        }
        if binding.source == ObserverSource::OpencodeHook
            || binding.source == ObserverSource::CodexHook
                && crate::codex_observer::ordered_event_id(event_id)
        {
            if !slot.codex_replay.accept(event_id, Self::MAX_SEEN_EVENTS) {
                let diagnostic = (binding.source == ObserverSource::CodexHook)
                    .then(|| {
                        slot.identity_diagnostics
                            .receipt(&binding.run, &event, true)
                    })
                    .flatten();
                drop(slots);
                self.log_identity_diagnostic(diagnostic);
                return Ok(ObserverAccept::Duplicate);
            }
        } else {
            if slot.seen.contains(event_id) {
                let diagnostic = (binding.source == ObserverSource::CodexHook)
                    .then(|| {
                        slot.identity_diagnostics
                            .receipt(&binding.run, &event, true)
                    })
                    .flatten();
                drop(slots);
                self.log_identity_diagnostic(diagnostic);
                return Ok(ObserverAccept::Duplicate);
            }
            if slot.seen.len() >= Self::MAX_SEEN_EVENTS {
                return Err(error("OBSERVER_EVENT_CAPACITY"));
            }
            slot.seen.insert(event_id.to_string());
        }

        let diagnostic = (binding.source == ObserverSource::CodexHook)
            .then(|| {
                slot.identity_diagnostics
                    .receipt(&binding.run, &event, false)
            })
            .flatten();
        drop(slots);
        self.log_identity_diagnostic(diagnostic);
        Ok(ObserverAccept::Accepted(ValidatedObserverEvent {
            run: binding.run.clone(),
            event_id: event_id.to_string(),
            source: binding.source,
            event,
        }))
    }

    fn log_identity_diagnostic(&self, record: Option<String>) {
        if let Some(record) = record {
            if self
                .diagnostic_emitted
                .fetch_update(
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                    |count| (count < 128).then_some(count + 1),
                )
                .is_ok()
            {
                log::info!("CC_DESK_CODEX_IDENTITY {record}");
            }
        }
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
        "codex-hook" => ObserverSource::CodexHook,
        "opencode-hook" => ObserverSource::OpencodeHook,
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

#[cfg(test)]
mod identity_diagnostic_tests {
    use super::*;
    const A: &str = "01a18afd-4f1f-7df1-98b7-93534352dc84";
    const B: &str = "01a18aff-66df-79f1-ba6b-cba3ff004b65";
    fn input(event: &str, id: &str) -> Value {
        serde_json::json!({"hook_event_name":event,"session_id":id,"prompt":"PRIVATE_TOKEN", "cwd":"PRIVATE_PATH", "argv":["PRIVATE_ARG"]})
    }
    #[test]
    fn codex_diagnostic_receipt_is_not_adoption_and_redacts_input_001() {
        let run = ObserverRun {
            run_id: "PRIVATE_PATH_TOKEN".into(),
            generation: 1,
        };
        let mut trace = CodexIdentityDiagnostics::default();
        let first = trace
            .receipt(&run, &input("SessionStart", A), false)
            .unwrap();
        assert!(!first.contains("PRIVATE"));
        let first: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(first["run"], "redacted");
        assert_eq!(first["afterReported"], A);
        let next: Value =
            serde_json::from_str(&trace.receipt(&run, &input("Stop", B), false).unwrap()).unwrap();
        assert_eq!(next["beforeReported"], A);
        assert_eq!(next["afterReported"], B);
        assert_eq!(next["outcome"], "backend-authenticated");
        assert_eq!(next["frontendAdoption"], "unknown");
        assert!(trace.receipt(&run, &input("Stop", B), false).is_none());
        assert!(trace
            .receipt(&run, &input("PRIVATE_EVENT", A), false)
            .is_none());
        assert!(trace
            .receipt(&run, &input("Stop", "PRIVATE_ID"), false)
            .is_none());
        for i in 0..100 {
            let _ = trace.receipt(
                &run,
                &input("SessionStart", if i % 2 == 0 { A } else { B }),
                false,
            );
        }
        assert_eq!(trace.emitted, 16);
    }
    #[test]
    fn codex_diagnostic_auth_generation_replay_and_retirement_remain_authoritative_002() {
        let registry = ObserverRegistry::new();
        let run = ObserverRun {
            run_id: format!("run-{}", "a".repeat(32)),
            generation: 1,
        };
        let binding = registry
            .mint(run.clone(), ObserverSource::CodexHook)
            .unwrap();
        let bytes = serde_json::to_vec(&input("SessionStart", A)).unwrap();
        let mut wrong = binding.clone();
        wrong.capability = "b".repeat(32);
        assert!(registry.accept_event(&wrong, "wrong-auth", &bytes).is_err());
        wrong = binding.clone();
        wrong.run.generation = 2;
        assert!(registry.accept_event(&wrong, "wrong-gen", &bytes).is_err());
        assert_eq!(registry.slots.lock()[&run].identity_diagnostics.emitted, 0);
        assert!(matches!(
            registry.accept_event(&binding, "first", &bytes).unwrap(),
            ObserverAccept::Accepted(_)
        ));
        assert_eq!(
            registry.accept_event(&binding, "first", &bytes).unwrap(),
            ObserverAccept::Duplicate
        );
        let stop = serde_json::to_vec(&input("Stop", B)).unwrap();
        // The registry must not copy the frontend's mismatched-ID rejection.
        assert!(matches!(
            registry.accept_event(&binding, "stop", &stop).unwrap(),
            ObserverAccept::Accepted(_)
        ));
        assert_eq!(
            registry.slots.lock()[&run]
                .identity_diagnostics
                .reported
                .unwrap()
                .to_string(),
            B
        );
        let claude = registry
            .mint(
                ObserverRun {
                    run_id: "claude-private".into(),
                    generation: 1,
                },
                ObserverSource::ClaudeHook,
            )
            .unwrap();
        registry.accept_event(&claude, "claude", &bytes).unwrap();
        assert_eq!(
            registry.slots.lock()[&claude.run]
                .identity_diagnostics
                .emitted,
            0
        );
        registry.revoke(&binding);
        let count = registry
            .diagnostic_emitted
            .load(std::sync::atomic::Ordering::Relaxed);
        assert!(registry.accept_event(&binding, "retired", &bytes).is_err());
        assert_eq!(
            registry
                .diagnostic_emitted
                .load(std::sync::atomic::Ordering::Relaxed),
            count
        );
        for _ in 0..200 {
            registry.log_identity_diagnostic(Some("fixed-test-record".into()));
        }
        assert_eq!(
            registry
                .diagnostic_emitted
                .load(std::sync::atomic::Ordering::Relaxed),
            128
        );
    }
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
    // 官方 StopFailure.error 是固定分类；详情、渲染错误文本和未知值仍不发布。
    // https://code.claude.com/docs/en/hooks#stopfailure-input
    if object["hook_event_name"] == "StopFailure" {
        if let Some(reason) = object
            .get("error")
            .and_then(Value::as_str)
            .filter(|reason| {
                matches!(
                    *reason,
                    "rate_limit"
                        | "overloaded"
                        | "authentication_failed"
                        | "oauth_org_not_allowed"
                        | "account_on_hold"
                        | "billing_error"
                        | "invalid_request"
                        | "model_not_found"
                        | "server_error"
                        | "max_output_tokens"
                        | "cloud_credential_error"
                        | "unknown"
                )
            })
        {
            safe.insert("error".into(), Value::String(reason.into()));
        }
    }
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
    // A provider-generated subagent ID is useful causal identity, never a path
    // or a global sequence. Retain it only for these lifecycle occurrences.
    if matches!(
        object["hook_event_name"].as_str(),
        Some("SubagentStart" | "SubagentStop")
    ) {
        if let Some(value) = object.get("agent_id") {
            let id = value
                .as_str()
                .ok_or_else(|| error("OBSERVER_INVALID_EVENT"))?;
            if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
                return Err(error("OBSERVER_INVALID_EVENT"));
            }
            safe.insert("agent_id".into(), value.clone());
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
