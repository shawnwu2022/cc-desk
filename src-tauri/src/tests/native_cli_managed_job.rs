//! Fresh own Node only; no existing process, account, Job limits or remote grant.
use super::*;
use crate::cli::native_runtime::NativeRuntime;
use crate::platform::owned_job_launch::JobLaunchObservation;
struct Cleanup(Arc<NativeRuntime>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

#[test]
#[allow(non_snake_case)]
fn ManagedJob_RuntimeTransfersOriginalResourcesAndStopsTree_001() {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let mut fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    // The root exits only after its fresh inherited-Job child reports ready.
    // The child remains alive until THIS original Job is stopped, never by PID.
    fixture.request.action=LaunchAction::Raw{argv:vec!["-e".into(),r#"
const fs=require('node:fs'),path=require('node:path');
const root=process.env.CC_DESK_TEST_ROOT;
// Node 24.18.0/libuv detached omits its own kill-on-parent-exit Job and
// explicitly DOES NOT set CREATE_BREAKAWAY_FROM_JOB. Our original Job remains.
const child=require('node:child_process').spawn(process.execPath,['-e',"if(process.send)process.send('ready');setInterval(()=>{},1000)"],{cwd:process.cwd(),detached:true,windowsHide:true,stdio:['ignore','inherit','inherit','ipc']});
child.once('message',()=>{
  fs.writeFileSync(path.join(root,'managed-child.json'),'{}',{flag:'wx'});
  const gate=setInterval(()=>{
    if(fs.existsSync(path.join(root,'cleanup-child'))){clearInterval(gate);child.once('close',()=>{fs.writeFileSync(path.join(root,'child-reaped'),'yes');process.exit(24)});child.kill();}
    else if(fs.existsSync(path.join(root,'allow-root-exit'))){clearInterval(gate);child.disconnect();child.unref();process.stdout.write('OWNED_TREE_READY\n',()=>process.exit(23));}
  },10);
});
"#.into()]};
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let _cleanup = Cleanup(runtime.clone());
    let events = fixture.events.clone();
    let status = runtime
        .start_managed_run(&fixture.caller, &fixture.request, |_| {
            fixture.routes.bind(1, Box::new(|| Ok(())), || {
                Ok(Channel::new(move |body| {
                    if let InvokeResponseBody::Json(text) = body {
                        events.lock().push(serde_json::from_str(&text).unwrap());
                    }
                    Ok(())
                }))
            })
        })
        .expect("managed own start");
    let resource = fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run)
        .unwrap();
    let job = resource
        .process
        .managed_job
        .as_ref()
        .expect("actual Job custody")
        .clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    let membership = loop {
        if fixture
            .root
            .path()
            .join("work/managed-child.json")
            .is_file()
            && job.active_processes() == Ok(2)
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    fs::write(
        fixture.root.path().join(if membership {
            "work/allow-root-exit"
        } else {
            "work/cleanup-child"
        }),
        b"own-fixture",
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut observed_active = None;
    let root_reaped_with_child = loop {
        if matches!(resource.process.pty.observe_exact_root(), Ok(true)) {
            observed_active = job.active_processes().ok();
            if observed_active == Some(1) {
                break true;
            }
            if observed_active == Some(0) {
                break false;
            }
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let transferred = runtime.owned_job_slot_for_test().observe() == JobLaunchObservation::Empty;
    let child_ready = fixture
        .root
        .path()
        .join("work/managed-child.json")
        .is_file();
    let root_code = resource
        .process
        .pty
        .try_wait()
        .ok()
        .flatten()
        .map(|status| status.exit_code());
    let mut stop = fixture
        .supervisor
        .begin_shutdown_owned(vec![fixture.supervisor.pin_shutdown(&status.run)])
        .unwrap();
    drop(resource);
    let deadline = Instant::now() + Duration::from_secs(10);
    let facts = loop {
        let facts = stop
            .managed_retirement(&status.run)
            .expect("managed owner remains retained");
        if facts.original_resources_retired() || Instant::now() >= deadline {
            break facts;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // Observe and finish cleanup BEFORE assertions; never abandon a live Job.
    assert!(
        facts.original_resources_retired(),
        "original owned resources not retired: {facts:?}"
    );
    assert!(
        membership && root_reaped_with_child,
        "root reap was incorrectly treated as entire tree exit: child_ready={child_ready} active={observed_active:?} root_code={root_code:?}"
    );
    assert!(
        transferred,
        "slot cleared without actual supervisor registration"
    );
    assert_eq!(job.active_processes(), Ok(0));
    assert!(
        matches!(
            stop.observe_run(&status.run),
            crate::run_supervisor::NativeShutdownObservation::Unknown {
                io_threads_joined: false,
                ..
            }
        ),
        "destructor return is not internal console exit"
    );
}

#[test]
#[allow(non_snake_case)]
fn ManagedJob_CloseAfterClaimCannotCompeteForOriginalReader_002() {
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let _cleanup = Cleanup(runtime.clone());
    let original = runtime.clone();
    let saw_no_drain = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = saw_no_drain.clone();
    runtime
        .owned_job_slot_for_test()
        .set_claim_hook_for_test(Box::new(move || {
            let slot = original.owned_job_slot_for_test();
            let no_competing_reader = !slot.begin_drain_for_test();
            let pending = slot.cleanup_step() == JobLaunchObservation::Pending;
            observed.store(no_competing_reader && pending, Ordering::SeqCst);
        }));
    let result = runtime.start_managed_run(&fixture.caller, &fixture.request, |_| {
        fixture
            .routes
            .bind(1, Box::new(|| Ok(())), || Ok(Channel::new(|_| Ok(()))))
    });
    let run = fixture.request_run();
    let mut stop = fixture
        .supervisor
        .begin_shutdown_owned(vec![fixture.supervisor.pin_shutdown(&run)])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let retired = loop {
        if stop
            .managed_retirement(&run)
            .is_some_and(|facts| facts.original_resources_retired())
        {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        retired,
        "cancelled claimed resources must retain one real supervisor cleanup owner"
    );
    assert!(
        result.is_err(),
        "closed claim must not publish a successful launch response"
    );
    assert!(
        saw_no_drain.load(Ordering::SeqCst),
        "claim-to-adopt cancellation stole the original reader"
    );
    assert_eq!(
        runtime.owned_job_slot_for_test().observe(),
        JobLaunchObservation::Empty
    );
}
