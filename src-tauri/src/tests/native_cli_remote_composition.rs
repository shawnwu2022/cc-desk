//! Own-resource composition, not phone/WebView/Job-tree qualification.
use super::*;
use crate::cli::native_runtime::NativeRuntime;
use crate::remote_host::RemoteHostService;
use crate::run_supervisor::{NativeRunPin, NativeShutdown, NativeShutdownObservation};
use crate::tests::remote_channels as channels;
use cc_desk_remote_host::native_host::WriterRetirement;

#[cfg(windows)]
async fn owned_session_exit_joins_original_owners() {
    use crate::cli::{document::DocumentAuthority, run_registry::RunRegistry};
    use crate::platform::owned_job_launch::JobLaunchObservation;
    use crate::remote_client_ipc::RemoteClientService;
    use crate::remote_host::ExitDecision;
    use cc_desk_remote_client::NativeClient;
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let _cleanup = RuntimeStop(runtime.clone()); // Original slot cleanup precedes launch.
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
        .unwrap();
    assert_eq!(
        runtime.owned_job_slot_for_test().observe(),
        JobLaunchObservation::Empty,
        "original Job/reader must transfer to supervisor before response"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let admission = fixture
        .supervisor
        .admit_remote_run(
            fixture
                .service
                .access(&fixture.caller, &status.run)
                .unwrap(),
            &status.run,
        )
        .unwrap();
    let (c, h, _grant, _cancel) = channels::channels(admission.scope().unwrap()).await;
    let c_authority = c.authority();
    let h = Arc::new(h);
    let host = Arc::new(RemoteHostService::default());
    host.adopt_admitted(admission, h.clone(), &runtime)
        .map_err(|rejected| {
            let (error, original) = *rejected;
            eprintln!(
                "TEST_ONLY_ADOPT_FAILURE code={} scope_error={:?} authority={:?} output={:?}",
                error.code,
                original.scope().err().map(|failure| failure.code),
                h.authority().check(),
                fixture
                    .supervisor
                    .snapshot(&status.run)
                    .map(|record| record.output())
            );
            error.code
        })
        .unwrap();
    let proof = c.client_installed().await.unwrap();
    let client = NativeClient::from_installed(c, proof).unwrap();
    let snapshot = client
        .snapshot(
            br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
            Duration::from_secs(1),
        )
        .await
        .unwrap();
    assert_eq!(snapshot.stamp.target.run, status.run.run_id);
    assert_eq!(
        snapshot.stamp.target.generation,
        status.run.generation.to_string()
    );
    // Explicit TEST_ONLY document fixture uses the existing original publication
    // path; it does not bypass the production signer/profile factory.
    let authority = DocumentAuthority::new(
        Arc::new(RunRegistry::new(2)),
        "http://tauri.localhost/index.html".parse().unwrap(),
    )
    .unwrap();
    let caller = authority.test_caller();
    let mut table = tauri::ResourceTable::default();
    let binding = Arc::new(authority.attach(&mut table).unwrap());
    let url = "http://tauri.localhost/index.html".parse().unwrap();
    authority.started(&url);
    authority.finished(&url);
    let client_service = Arc::new(RemoteClientService::default());
    client_service
        .test_only_install_original_client(caller, &binding, client)
        .unwrap();
    // This real late access pins the original PTY after root/worker settlement.
    let late_access = fixture
        .service
        .access(&fixture.caller, &status.run)
        .unwrap();
    assert_eq!(host.request_exit(83, false), ExitDecision::Wait(83));
    let (tx, mut exited) = tokio::sync::oneshot::channel();
    host.start_exit(83, runtime.clone(), client_service.clone(), move |code| {
        let _ = tx.send(code);
    });
    assert_eq!(host.request_exit(84, false), ExitDecision::Prevent);
    let deadline = Instant::now() + Duration::from_secs(10);
    let held = loop {
        let facts = host.test_original_managed_facts(&status.run);
        if facts.as_ref().is_some_and(|facts| {
            facts.direct_child_reaped && facts.wait_workers_joined && facts.physical_writer_joined
        }) {
            break facts.unwrap();
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let prevented = exited.try_recv().is_err();
    drop(late_access);
    let code = tokio::time::timeout(Duration::from_secs(10), exited)
        .await
        .unwrap()
        .unwrap();
    assert!(host.test_join_exit_worker().await);
    let facts = host.test_original_managed_facts(&status.run).unwrap();
    assert!(
        prevented && !held.pinned_conpty_close_returned && !held.stop_job_joined,
        "late access was not retained: {held:?}"
    );
    assert_eq!(held.job_active_processes, Some(0));
    assert!(!held.job_query_is_historical && !held.original_job_handle_closed);
    assert_eq!(facts.job_active_processes, Some(0));
    assert!(
        facts.original_resources_retired()
            && facts.original_job_handle_closed
            && facts.job_query_is_historical
    );
    assert!(
        facts.pinned_conpty_close_returned
            && facts.original_reader_served_close
            && facts.stream_capacity_retired
    );
    assert!(!fixture.supervisor.active_for_test(&status.run));
    assert_eq!(fixture.transports.budgeted_bytes(), 0);
    assert!(fixture
        .service
        .registry()
        .resource(&fixture.caller, &status.run)
        .is_err());
    let record = fixture.supervisor.snapshot(&status.run).unwrap();
    assert_eq!(record.output(), OutputLifecycle::Incomplete);
    assert!(
        record.final_offset().is_none(),
        "ambiguous raw zero is not EOF/parsed ACK"
    );
    assert!(h.authority().check().is_err() && c_authority.check().is_err());
    assert!(h.wait_closed_checked().await.is_ok());
    assert!(client_service.shutdown().await.is_ok());
    assert_eq!(code, 83);
    assert_eq!(host.request_exit(83, false), ExitDecision::Proceed);
    assert_eq!(host.request_exit(84, false), ExitDecision::Prevent);
    let mut next = fixture.request.clone();
    next.request_id = "TEST_ONLY_CLOSED_EXIT_REQUEST".into();
    next.tab_id = "TEST_ONLY_CLOSED_EXIT_TAB".into();
    next.run_id = "TEST_ONLY_CLOSED_EXIT_RUN".into();
    assert!(runtime
        .start_managed_run(&fixture.caller, &next, |_| panic!(
            "closed admission reused retired capacity"
        ))
        .is_err());
    host.start_exit(83, runtime, client_service, |_| {
        panic!("duplicate exit callback")
    });
    eprintln!("TEST_ONLY_OWNED_SESSION_RELEASED held={held:?} final={facts:?} first_exit_code={code} host_client_network_joined=true capacity=0 incomplete=true");
}
struct Cleanup {
    supervisor: Arc<NativeRunSupervisor>,
    host: Arc<RemoteHostService>,
    run: RunKey,
    pin: Option<NativeRunPin>,
    stop: Option<NativeShutdown>,
    proofs: Vec<cc_desk_remote_security::BoundDataAuthority>,
}
impl Cleanup {
    fn close(&mut self) -> bool {
        self.host.test_begin_host_shutdown();
        for proof in &self.proofs {
            proof.request_close();
        }
        if self.stop.is_none() {
            let pin = self
                .pin
                .take()
                .unwrap_or_else(|| self.supervisor.pin_shutdown(&self.run));
            self.stop = self.supervisor.begin_shutdown_owned(vec![pin]).ok();
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            let writer = self.host.test_writer_retirement();
            let native = self.stop.as_mut().map(|s| s.observe_run(&self.run));
            if writer == WriterRetirement::Joined
                && matches!(
                    native,
                    Some(NativeShutdownObservation::OwnedSessionReleased)
                        | Some(NativeShutdownObservation::Unknown {
                            direct_child_reaped: true,
                            output_reader_joined: true,
                            wait_workers_joined: true,
                            original_io_drop_returned: true,
                            stop_job_joined: true,
                            ..
                        })
                )
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ActualAdmissionInstallsOriginalOwnNode_001() {
    use cc_desk_remote_client::NativeClient;
    use cc_desk_remote_record::{control_io as wire, input, projection};
    const BUDGET: Duration = Duration::from_secs(1);
    let check = |ok, error| if ok { Ok(()) } else { Err(error) };
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("input", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    );
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: host.clone(),
        run: fixture.request_run(),
        pin: None,
        stop: None,
        proofs: Vec::new(),
    };
    let status = fixture.start();
    cleanup.pin = Some(fixture.supervisor.pin_shutdown(&status.run));
    let deadline = Instant::now() + Duration::from_secs(10);
    while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
        assert!(
            Instant::now() < deadline,
            "fresh own Node did not initialize"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut original_client = None;
    let mut client = None;
    let mut original_host = None;
    let mut grant = None;
    let mut cancel = None;
    let outcome:Result<(),&'static str>=async{
        let access=fixture.service.access(&fixture.caller,&status.run).map_err(|_|"original access")?;
        let admission=fixture.supervisor.admit_remote_run(access.clone(),&status.run).map_err(|_|"original admission")?;
        let scope=admission.scope().map_err(|_|"original scope")?;
        let mut wrong_scope=scope.clone();wrong_scope.session="a5".repeat(16);
        let(wc,wh,_wrong_gate,_wrong_cancel)=channels::channels(wrong_scope).await;
        cleanup.proofs.extend([wc.authority(),wh.authority()]);
        let wh=Arc::new(wh);
        let wrong=host.adopt_admitted(admission,wh.clone(),&runtime);
        wc.authority().request_close();wh.authority().request_close();
        let _=tokio::join!(wc.wait_closed(),wh.wait_closed());
        let admission=match wrong{
            Err(rejected)=>{let(_,original)=*rejected;original},
            Ok(())=>return Err("wrong scope consumed native admission"),
        };
        check(admission.scope().map_err(|_|"rejected original scope")?==scope,"wrong scope changed original custody")?;
        let(c,h,gate,stop)=channels::channels(scope).await;
        cleanup.proofs.extend([c.authority(),h.authority()]);
        grant=Some(gate);cancel=Some(stop);original_client=Some(c);original_host=Some(Arc::new(h));
        host.adopt_admitted(admission,original_host.as_ref().unwrap().clone(),&runtime).map_err(|rejected|{
            let(_,original)=*rejected;drop(original);"verified native adoption"
        })?;
        let installed=original_client.as_ref().unwrap().client_installed().await.map_err(|_|"retained service install proof")?;
        client=Some(NativeClient::from_installed(original_client.take().unwrap(),installed).map_err(|_|"original installed client")?);
        let client=client.as_ref().unwrap();
        let snapshot=client.snapshot(br#"{"message":"Snapshot","profile":1,"request":"1"}"#,BUDGET).await.map_err(|_|"fresh snapshot delivery")?;
        check(snapshot.stamp.target.run==status.run.run_id && snapshot.stamp.target.generation==status.run.generation.to_string(),"snapshot original target")?;
        let mut grid=String::new();
        for bytes in &snapshot.parts{
            let projection::Content::SnapshotPart(part)=projection::decode(bytes,1).map_err(|_|"snapshot codec")?.content else{return Err("snapshot part");};
            grid.extend(part.cells.iter().map(|cell|cell.scalar));
        }
        check(grid.contains("OWNED_READY"),"canonical prefix before admission missing")?;
        let ack=projection::encode_ack(&projection::Ack{profile:1,stamp:snapshot.stamp.clone()},1).map_err(|_|"ack codec")?;
        let acked=client.exchange(&ack,BUDGET).await.map_err(|_|"exact snapshot ACK")?;
        check(matches!(wire::decode(&acked),Ok(wire::Message::ProjectionAcknowledged{stamp,..}) if stamp==snapshot.stamp),"ACK echo")?;
        let window=client.exchange(br#"{"message":"Window","profile":1,"request":"2"}"#,BUDGET).await.map_err(|_|"input window")?;
        let wire::Message::WindowGranted{window,not_after_host_ms,..}=wire::decode(&window).map_err(|_|"window codec")? else{return Err("window reply");};
        let mut token=[0;32];
        for(i,pair)in window.as_bytes().chunks_exact(2).enumerate(){token[i]=u8::from_str_radix(std::str::from_utf8(pair).map_err(|_|"window hex")?,16).map_err(|_|"window value")?;}
        let frame=input::encode(&input::Input{operation:[0x21;16],sequence:1,window:token,not_after_host_ms:not_after_host_ms.parse().map_err(|_|"window deadline")?,bytes:b"nat".to_vec()}).map_err(|_|"input codec")?;
        let receipt=client.exchange(&frame,BUDGET).await.map_err(|_|"original remote input")?;
        let wire::Message::InputReceipt{operation,sequence,outcome:wire::Outcome::HostWritten,proof:Some(proof),..}=wire::decode(&receipt).map_err(|_|"receipt codec")? else{return Err("exact written receipt");};
        check(operation=="21".repeat(16) && sequence=="1","receipt operation identity")?;
        for request in ["3","4"]{
            let query=wire::encode(&wire::Message::ReceiptQuery{profile:1,request:request.into(),operation:operation.clone(),sequence:sequence.clone()}).map_err(|_|"query codec")?;
            let found=client.exchange(&query,BUDGET).await.map_err(|_|"original receipt query")?;
            check(wire::decode(&found)==Ok(wire::Message::ReceiptStatus{profile:1,request:request.into(),operation:operation.clone(),sequence:sequence.clone(),status:wire::Status::Known,outcome:Some(wire::Outcome::HostWritten),proof:Some(proof.clone())}),"query original proof")?;
        }
        for(request,expected)in [("5",wire::Disposition::Advanced),("6",wire::Disposition::AlreadySettled)]{
            let settle=wire::encode(&wire::Message::ReceiptAck{profile:1,request:request.into(),operation:operation.clone(),sequence:sequence.clone(),outcome:wire::Outcome::HostWritten,proof:proof.clone()}).map_err(|_|"settlement codec")?;
            let settled=client.exchange(&settle,BUDGET).await.map_err(|_|"receipt settlement")?;
            check(matches!(wire::decode(&settled),Ok(wire::Message::ReceiptSettled{disposition,..}) if disposition==expected),"exact settlement/no replay")?;
        }
        let next=client.snapshot(br#"{"message":"Snapshot","profile":1,"request":"7"}"#,BUDGET).await.map_err(|_|"fresh second snapshot")?;
        check(next.stamp!=snapshot.stamp,"fresh delivery lineage")?;
        // A new complete snapshot was delivered but has NOT been acknowledged.
        // Replaying the old original input must not reach physical IO.
        check(client.exchange(&frame,BUDGET).await.is_err(),"input before new exact ACK was accepted")?;
        check(original_host.as_ref().unwrap().authority().check().is_err(),"failed peer remains live")?;
        let local=access.write_frame(b"ive").map_err(|_|"phone loss stopped original local input")?;
        check(local.state==crate::terminal_input::InputWriteState::HostWritten && local.confirmed_bytes==3,"exact local continuation")?;
        drop(access);
        fixture.wait_lifecycle(|state|state.process()==ProcessLifecycle::Exited);
        let reports:Vec<Value>=fs::read_dir(fixture.root.path().join("work")).map_err(|_|"own report directory")?.filter_map(Result::ok).filter(|e|e.path().extension().is_some_and(|e|e=="json")).map(|e|serde_json::from_slice(&fs::read(e.path()).unwrap()).unwrap()).collect();
        check(reports.iter().any(|r|r["input"]==json!(b"native".to_vec())),"own Node did not consume exactly3 remote+3 local bytes")?;
        Ok(())
    }.await;
    let retired = cleanup.close();
    let network_joined = host.test_join_network().await;
    if let Some(client) = &client {
        let _ = client.close().await;
    }
    if let Some(channel) = &original_client {
        let _ = channel.close().await;
    }
    if let Some(channel) = &original_host {
        let _ = channel.wait_closed().await;
    }
    drop(grant);
    drop(cancel);
    assert!(
        retired && network_joined,
        "original own root/reader/writer/IO/network must retire before assertion"
    );
    assert_eq!(outcome, Ok(()));
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(non_snake_case)]
async fn NativeRemote_ManagedOriginalJobAndRemoteOwnersRetireTogether_002() {
    // One permanent Host admission per process: the complete approved exit
    // scenario extends this original case instead of creating a second Host.
    owned_session_exit_joins_original_owners().await;
}
