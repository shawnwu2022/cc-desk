//! Backend run reservation contract. Behavioral implementation follows observed RED.
#![allow(dead_code)]

use super::profiles::error;
use super::snapshot::CallerIdentity;
use super::types::{SafeError, WireU64};
use serde::Serialize;
use std::marker::PhantomData;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunKey {
    pub(crate) run_id: String,
    pub(crate) generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LaunchPhase {
    Reserved,
    Starting,
    Running,
    Failed,
    Cancelled,
    Indeterminate,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum LaunchFailure {
    RouteUnavailable,
    ProcessStartFailed,
    Aborted,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LaunchStatus {
    pub(crate) request_id: String,
    pub(crate) run: RunKey,
    pub(crate) phase: LaunchPhase,
    pub(crate) failure: Option<LaunchFailure>,
}

pub(crate) struct RunRegistry<R> {
    marker: PhantomData<R>,
}

impl<R> std::fmt::Debug for RunRegistry<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunRegistry(<redacted>)")
    }
}

impl<R> RunRegistry<R> {
    pub(crate) fn new(_capacity: usize) -> Self {
        Self { marker: PhantomData }
    }

    pub(crate) fn activate_window(&self, label: &str) -> Result<CallerIdentity, SafeError> {
        Ok(CallerIdentity {
            instance_id: "scaffold-instance".into(),
            window_label: label.into(),
            webview_epoch: WireU64::parse("1")?,
        })
    }

    pub(crate) fn revoke_window(&self, _caller: &CallerIdentity) -> Result<(), SafeError> {
        Ok(())
    }

    pub(crate) fn status(
        &self,
        _caller: &CallerIdentity,
        _request_id: &str,
    ) -> Result<LaunchStatus, SafeError> {
        Err(error("REGISTRY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn resource(
        &self,
        _caller: &CallerIdentity,
        _run: &RunKey,
    ) -> Result<Arc<R>, SafeError> {
        Err(error("REGISTRY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn mark_exited(&self, _run: &RunKey) -> Result<(), SafeError> {
        Err(error("REGISTRY_NOT_IMPLEMENTED"))
    }

    pub(crate) fn retire(&self, _run: &RunKey) -> Result<(), SafeError> {
        Err(error("REGISTRY_NOT_IMPLEMENTED"))
    }
}
