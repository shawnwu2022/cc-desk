//! Optional observer preparation. Owns observation authority, not a CLI process.
use crate::cli::environment::{EnvMap, ObserverEnv};
use crate::cli::profiles::error;
use crate::cli::types::SafeError;
use crate::observer_registry::{ObserverDelivery, ObserverLease, ObserverRegistry, ObserverRun};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) struct ObserverHost {
    registry: Arc<ObserverRegistry>,
    plugin_dir: PathBuf,
    port: Arc<dyn Fn() -> Option<u16> + Send + Sync>,
}
pub(crate) struct PreparedObservation {
    pub(crate) lease: ObserverLease,
    pub(crate) environment: ObserverEnv,
    pub(crate) plugin_dir: PathBuf,
    pub(crate) codex_args: Option<Vec<std::ffi::OsString>>,
}
impl ObserverHost {
    pub(crate) fn new(
        registry: Arc<ObserverRegistry>,
        plugin_dir: PathBuf,
        port: Arc<dyn Fn() -> Option<u16> + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            plugin_dir,
            port,
        }
    }
    pub(crate) fn prepare(
        &self,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PreparedObservation, SafeError> {
        let port = (self.port)()
            .filter(|port| *port != 0)
            .ok_or_else(|| error("OBSERVER_SERVICE_UNAVAILABLE"))?;
        if !crate::hook_config::plugin_ready(&self.plugin_dir) {
            return Err(error("OBSERVER_PLUGIN_UNAVAILABLE"));
        }
        let lease = self.registry.lease(run, delivery, authorize)?;
        let binding = lease.binding();
        let environment = ObserverEnv {
            values: EnvMap::from([
                ("CC_BOX_HOOK_PORT".into(), port.to_string().into()),
                (
                    "CC_DESK_OBSERVER_RUN".into(),
                    binding.run.run_id.clone().into(),
                ),
                (
                    "CC_DESK_OBSERVER_GENERATION".into(),
                    binding.run.generation.to_string().into(),
                ),
                (
                    "CC_DESK_OBSERVER_CAPABILITY".into(),
                    binding.capability.clone().into(),
                ),
            ]),
        };
        Ok(PreparedObservation {
            lease,
            environment,
            plugin_dir: self.plugin_dir.clone(),
            codex_args: None,
        })
    }
    pub(crate) fn prepare_codex(
        &self,
        run: ObserverRun,
        delivery: ObserverDelivery,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<PreparedObservation, SafeError> {
        let port = (self.port)()
            .filter(|port| *port != 0)
            .ok_or_else(|| error("OBSERVER_SERVICE_UNAVAILABLE"))?;
        let args = crate::codex_observer::hook_args(
            &std::env::current_exe().map_err(|_| error("OBSERVER_PREPARE_FAILED"))?,
        )?;
        let lease = self.registry.lease_source(
            run,
            delivery,
            authorize,
            crate::observer_registry::ObserverSource::CodexHook,
        )?;
        let binding = lease.binding();
        let names = crate::codex_observer::ENV_NAMES;
        let values = EnvMap::from([
            (names[0].into(), port.to_string().into()),
            (names[1].into(), binding.run.run_id.clone().into()),
            (names[2].into(), binding.run.generation.to_string().into()),
            (names[3].into(), binding.capability.clone().into()),
        ]);
        Ok(PreparedObservation {
            lease,
            environment: ObserverEnv { values },
            plugin_dir: PathBuf::new(),
            codex_args: Some(args),
        })
    }
}
