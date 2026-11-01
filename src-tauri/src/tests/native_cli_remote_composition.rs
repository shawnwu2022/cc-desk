//! Own-resource composition, not phone/WebView/Job-tree qualification.
use super::*;
use crate::cli::native_runtime::NativeRuntime;
use crate::remote_host::RemoteHostService;
use crate::run_supervisor::{NativeRunPin, NativeShutdown, NativeShutdownObservation};
use crate::tests::remote_channels as channels;
use cc_desk_remote_host::native_host::WriterRetirement;
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
                    Some(NativeShutdownObservation::Unknown {
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
    use crate::platform::owned_job_launch::JobLaunchObservation;
    use cc_desk_remote_client::NativeClient;
    // This guard precedes launch: a failed handoff still belongs to the
    // runtime's original OwnedJobLaunchSlot, not merely a supervisor pin.
    struct RuntimeStop(Arc<NativeRuntime>);
    impl Drop for RuntimeStop {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }
    let transports = Arc::new(TerminalTransports::new());
    let supervisor = Arc::new(NativeRunSupervisor::new(transports.clone()));
    let fixture = Fixture::with_runtime_environment("hold", supervisor, transports, true);
    let host = Arc::new(RemoteHostService::default());
    let runtime = Arc::new(NativeRuntime::with_owned_components_for_test(
        fixture.service.clone(),
        fixture.transports.clone(),
        fixture.supervisor.clone(),
    ));
    let _original_runtime = RuntimeStop(runtime.clone());
    let mut cleanup = Cleanup {
        supervisor: fixture.supervisor.clone(),
        host: host.clone(),
        run: fixture.request_run(),
        pin: None,
        stop: None,
        proofs: Vec::new(),
    };
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
        .expect("fresh fixed managed Node fixture; original runtime guard already installed");
    cleanup.pin = Some(fixture.supervisor.pin_shutdown(&status.run));
    let transferred = runtime.owned_job_slot_for_test().observe() == JobLaunchObservation::Empty;
    let mut original_client = None;
    let mut client = None;
    let mut original_host = None;
    let mut grant = None;
    let mut cancel = None;
    let outcome: Result<(), &'static str> = async {
        let until = Instant::now() + Duration::from_secs(10);
        while fixture.child_reports() == 0 || fixture.events.lock().is_empty() {
            if Instant::now() >= until {
                return Err("own managed Node did not initialize");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let access = fixture
            .service
            .access(&fixture.caller, &status.run)
            .map_err(|_| "original managed access")?;
        let admission = fixture
            .supervisor
            .admit_remote_run(access, &status.run)
            .map_err(|_| "original managed admission")?;
        let scope = admission.scope().map_err(|_| "original managed scope")?;
        let (c, h, gate, stop) = channels::channels(scope).await;
        cleanup.proofs.extend([c.authority(), h.authority()]);
        original_client = Some(c);
        original_host = Some(Arc::new(h));
        grant = Some(gate);
        cancel = Some(stop);
        host.adopt_admitted(admission, original_host.as_ref().unwrap().clone(), &runtime)
            .map_err(|rejected| {
                let (_, original) = *rejected;
                drop(original);
                "same managed Host adoption"
            })?;
        let proof = original_client
            .as_ref()
            .unwrap()
            .client_installed()
            .await
            .map_err(|_| "same managed install proof")?;
        client = Some(
            NativeClient::from_installed(original_client.take().unwrap(), proof)
                .map_err(|_| "same installed managed client")?,
        );
        let snapshot = client
            .as_ref()
            .unwrap()
            .snapshot(
                br#"{"message":"Snapshot","profile":1,"request":"1"}"#,
                Duration::from_secs(1),
            )
            .await
            .map_err(|_| "same managed snapshot")?;
        if snapshot.stamp.target.run != status.run.run_id
            || snapshot.stamp.target.generation != status.run.generation.to_string()
        {
            return Err("managed snapshot owner mismatch");
        }
        Ok(())
    }
    .await;
    let retired = cleanup.close();
    let deadline = Instant::now() + Duration::from_secs(10);
    let facts = loop {
        // The original shutdown entry retains its original pin and Job object;
        // neither a missing fact nor absence of a handle means Job zero.
        let facts = cleanup
            .stop
            .as_mut()
            .and_then(|stop| stop.managed_retirement(&cleanup.run));
        if facts
            .as_ref()
            .is_some_and(|facts| facts.original_resources_retired())
            || Instant::now() >= deadline
        {
            break facts;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
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
    let internal_console_unknown = matches!(
        cleanup
            .stop
            .as_mut()
            .map(|stop| stop.observe_run(&cleanup.run)),
        Some(NativeShutdownObservation::Unknown {
            io_threads_joined: false,
            ..
        })
    );
    eprintln!("TEST_ONLY_MANAGED_REMOTE_RETIREMENT transferred={transferred} host_writer_joined={retired} network_joined={network_joined} facts={facts:?} internal_console_unknown={internal_console_unknown}");
    assert!(
        retired && network_joined,
        "original Host writer/network must join before assertion"
    );
    let facts = facts.expect("original managed retirement facts must remain present");
    assert!(
        facts.original_resources_retired(),
        "same held Job and all six original owners must retire: {facts:?}"
    );
    assert_eq!(facts.job_active_processes, Some(0));
    assert!(
        facts.direct_child_reaped
            && facts.output_reader_joined
            && facts.wait_workers_joined
            && facts.physical_writer_joined
            && facts.original_io_drop_returned
            && facts.stop_job_joined
    );
    assert!(
        transferred && internal_console_unknown,
        "registration transfers original custody; ConPTY internal exit remains unproven"
    );
    assert_eq!(outcome, Ok(()));
}
