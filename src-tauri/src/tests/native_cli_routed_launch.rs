use crate::cli::environment::EnvMap;
use crate::cli::launch::LaunchCoordinator;
use crate::cli::profiles::{error, Override, Profile};
use crate::cli::routed_launch::RoutedResource;
use crate::cli::run_registry::LaunchPhase;
use crate::cli::snapshot::{freeze_launch, CallerIdentity, FreezeContext, LaunchSnapshot};
use crate::cli::types::{CliKind, LaunchAction, LaunchRequest, SafeError};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

pub(super) struct Lease {
    pub(super) drops: Arc<AtomicUsize>,
    pub(super) check: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
        if let Some(check) = self.check.take() {
            check();
        }
    }
}

pub(super) struct Fixture<P> {
    directory: tempfile::TempDir,
    pub(super) profile: Profile,
    pub(super) request: LaunchRequest,
    pub(super) driver: LaunchCoordinator<RoutedResource<P, Lease>>,
    pub(super) caller: CallerIdentity,
    pub(super) drops: Arc<AtomicUsize>,
}

impl<P> Fixture<P> {
    pub(super) fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut profile = Profile::new("owned-profile", CliKind::Codex);
        profile.program_path =
            Override::Set(std::env::current_exe().unwrap().to_str().unwrap().into());
        let request = LaunchRequest {
            request_id: "owned-request".into(),
            tab_id: "owned-tab".into(),
            run_id: "owned-run".into(),
            generation: 1,
            profile_id: profile.id.clone(),
            expected_profile_revision: profile.revision,
            cli: CliKind::Codex,
            launch_cwd: directory.path().to_str().unwrap().into(),
            action: LaunchAction::Raw {
                argv: vec!["synthetic-private-argument".into()],
            },
            extra_args: vec![],
            cols: 80,
            rows: 24,
        };
        let driver = LaunchCoordinator::new(128);
        let caller = driver.registry().activate_window("main").unwrap();
        Self {
            directory,
            profile,
            request,
            driver,
            caller,
            drops: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(super) fn root(&self) -> &Path {
        self.directory.path()
    }

    pub(super) fn freeze(&self) -> Result<LaunchSnapshot, SafeError> {
        let mut inherited: EnvMap = std::env::vars_os().collect();
        inherited.insert(
            "CC_DESK_TEST_ROOT".into(),
            self.root().as_os_str().to_owned(),
        );
        let empty = EnvMap::new();
        freeze_launch(
            &self.request,
            &self.profile,
            &self.caller,
            &FreezeContext {
                inherited: &inherited,
                terminal: &empty,
                legacy: None,
                observer: None,
            },
        )
    }

    pub(super) fn lease(&self) -> Lease {
        Lease {
            drops: self.drops.clone(),
            check: None,
        }
    }
}

#[test]
fn D11_Route_LeaseLivesThroughSpawnAndExit_01() {
    let f = Fixture::<usize>::new();
    let status = f
        .driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| Ok(f.lease()),
            |_| {
                assert_eq!(f.drops.load(Ordering::SeqCst), 0, "route died before spawn");
                Ok(42)
            },
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    f.driver.registry().mark_exited(&status.run).unwrap();
    assert_eq!(
        f.drops.load(Ordering::SeqCst),
        0,
        "root exit is not route retirement"
    );
    f.driver.registry().retire(&status.run).unwrap();
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Route_FailedSpawnReleasesAfterStatusOutsideLock_02() {
    let f = Fixture::<usize>::new();
    let registry = f.driver.registry().clone();
    let caller = f.caller.clone();
    let lease = Lease {
        drops: f.drops.clone(),
        check: Some(Box::new(move || {
            assert_eq!(
                registry.status(&caller, "owned-request").unwrap().phase,
                LaunchPhase::Failed,
            );
        })),
    };
    let status = f
        .driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| Ok(lease),
            |_| Err(error("synthetic-failure")),
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Failed);
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Route_RevokeBeforeSpawnDropsLease_03() {
    let f = Fixture::<usize>::new();
    let status = f
        .driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| {
                f.driver.registry().revoke_window(&f.caller)?;
                Ok(f.lease())
            },
            |_| panic!("revoked attempt must not spawn"),
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Cancelled);
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Route_SpawnPanicDropsLeaseWithoutReplay_04() {
    let f = Fixture::<usize>::new();
    let alive_at_spawn = AtomicBool::new(false);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = f.driver.start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| Ok(f.lease()),
            |_| {
                alive_at_spawn.store(f.drops.load(Ordering::SeqCst) == 0, Ordering::SeqCst);
                panic!("synthetic pre-child panic");
            },
        );
    }))
    .is_err());
    let replay = f
        .driver
        .start_routed(
            &f.caller,
            &f.request,
            || panic!("replay must not prepare"),
            |_| panic!("replay must not connect"),
            |_| panic!("replay must not spawn"),
        )
        .unwrap();
    assert_eq!(replay.phase, LaunchPhase::Indeterminate);
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
    assert!(alive_at_spawn.load(Ordering::SeqCst));
}

#[test]
fn D11_Route_ExternalResourceLeaseSurvivesRetirement_05() {
    let f = Fixture::<usize>::new();
    let status = f
        .driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| Ok(f.lease()),
            |_| Ok(42),
        )
        .unwrap();
    let held = f
        .driver
        .registry()
        .resource(&f.caller, &status.run)
        .unwrap();
    f.driver.registry().mark_exited(&status.run).unwrap();
    f.driver.registry().retire(&status.run).unwrap();
    assert_eq!(held.process, 42);
    assert_eq!(f.drops.load(Ordering::SeqCst), 0);
    drop(held);
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
}

#[test]
fn D11_Route_ConnectFailureNeverSpawns_06() {
    let f = Fixture::<usize>::new();
    let status = f
        .driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| {
                let _partial_route = f.lease();
                Err(error("synthetic-route-failure"))
            },
            |_| panic!("failed route must not spawn"),
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Failed);
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
}
