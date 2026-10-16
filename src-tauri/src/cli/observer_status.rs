//! 只投影冻结安装结果，不泄露 observer capability 或改变进程所有权。
use super::profiles::error;
use super::run_registry::RunKey;
use super::types::{CliKind, LaunchAction, SafeError};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ObserverStatusRequest {
    pub(crate) request_id: String,
    pub(crate) run_id: String,
    pub(crate) generation: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ObserverStatus {
    pub(crate) request_id: String,
    pub(crate) run: RunKey,
    pub(crate) state: &'static str,
    pub(crate) reason_code: &'static str,
}

pub(crate) fn installation_reason(
    snapshot: &super::snapshot::LaunchSnapshot,
    installed: bool,
    failure: Option<&'static str>,
) -> &'static str {
    if snapshot.request().cli != CliKind::Claude
        || matches!(snapshot.request().action, LaunchAction::Raw { .. })
    {
        "OBSERVER_UNSUPPORTED"
    } else if !snapshot.observer_requested() {
        "OBSERVER_DISABLED"
    } else if installed {
        "OBSERVER_CONNECTING"
    } else {
        failure.unwrap_or("OBSERVER_SERVICE_UNAVAILABLE")
    }
}

impl super::launch_service::LaunchService {
    pub(crate) fn observer_status(
        &self,
        caller: &super::snapshot::CallerIdentity,
        query: &ObserverStatusRequest,
    ) -> Result<ObserverStatus, SafeError> {
        let run = RunKey {
            run_id: query.run_id.clone(),
            generation: query.generation,
        };
        self.registry().admit_run(caller, &run, &query.request_id)?;
        let resource = self.registry().resource(caller, &run)?;
        let frozen = &resource.process;
        if frozen.snapshot.request().request_id != query.request_id {
            return Err(error("REQUEST_CONFLICT"));
        }
        let reason = installation_reason(
            &frozen.snapshot,
            frozen.observer.is_some(),
            frozen.observer_failure,
        );
        Ok(ObserverStatus {
            request_id: query.request_id.clone(),
            run,
            state: match reason {
                "OBSERVER_DISABLED" | "OBSERVER_UNSUPPORTED" => "off",
                "OBSERVER_CONNECTING" => "connecting",
                _ => "unavailable",
            },
            reason_code: reason,
        })
    }
}
