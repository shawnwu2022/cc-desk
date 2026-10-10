use super::*;
use crate::cli::native_projection::wire::{ProjectionState, ResourceItem, ResourceKind};
use std::fs;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
fn n(x: u64) -> WireU64 {
    WireU64::parse(&x.to_string()).unwrap()
}
fn owner() -> Owner {
    Owner {
        instance: "instance".into(),
        window: "main".into(),
        epoch: n(1),
    }
}
fn grant(path: &std::path::Path, title: &str, live: Arc<AtomicBool>) -> Grant {
    fs::create_dir_all(path.join("projects/p")).unwrap();
    fs::write(
        path.join("projects/p/same.jsonl"),
        format!("{{\"type\":\"custom-title\",\"customTitle\":\"{title}\"}}\n"),
    )
    .unwrap();
    Grant {
        owner: owner(),
        cli: CliKind::Claude,
        profile_id: "p".into(),
        profile_revision: n(1),
        target: ScopeTarget::Profile {
            profile_id: "p".into(),
            expected_profile_revision: n(1),
            project_id: None,
        },
        basis: SourceBasis::ConfiguredProfile,
        root: Root::open(path).unwrap(),
        project: None,
        project_paths: vec![],
        user_config: None,
        check: Arc::new(move || {
            if live.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("SCOPE_REVOKED")
            }
        }),
    }
}
fn request(source: SourceRef) -> ReadRequest {
    ReadRequest {
        source,
        resource_kind: ResourceKind::History,
        request_epoch: n(9),
        query: None,
        session_id: None,
        limit: 100,
        offset: 0,
    }
}
#[test]
fn incomplete_history_survives_filtering_and_empty_pages() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let mut g = grant(t.path(), "main", Arc::new(AtomicBool::new(true)));
    fs::write(
        t.path().join("projects/p/same.jsonl"),
        format!(
            "{{\"type\":\"custom-title\",\"customTitle\":\"main\"}}\n{}",
            "x".repeat(80 * 1024)
        ),
    )
    .unwrap();
    g.project = Some(Root::open(t.path()).unwrap());
    g.project_paths = vec![t.path().to_owned()];
    let source = registry.register(g).unwrap();
    let response = registry.read(&owner(), &request(source)).unwrap();
    assert_eq!(response.state, ProjectionState::Ready);
    assert!(response.items.is_empty());
    assert_eq!(response.history_metadata_incomplete, Some(true));
}
#[test]
fn incomplete_history_response_budget_remains_safe() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let g = grant(t.path(), "main", Arc::new(AtomicBool::new(true)));
    let body = format!(
        "{}\n{}\n",
        serde_json::json!({"type":"user","cwd":format!("/{}", "x".repeat(24_000)),"message":{"content":"task"}}),
        serde_json::json!({"type":"assistant","message":{"content":"x".repeat(80 * 1024)}})
    );
    for index in 0..100 {
        fs::write(t.path().join(format!("projects/p/{index:03}.jsonl")), &body).unwrap();
    }
    let source = registry.register(g).unwrap();
    let response = registry.read(&owner(), &request(source)).unwrap();
    assert_eq!(response.state, ProjectionState::Unavailable);
    assert_eq!(
        response.reason.as_deref(),
        Some("SOURCE_RESPONSE_TOO_LARGE")
    );
    assert!(response.items.is_empty());
    assert_eq!(response.history_metadata_incomplete, None);
}
#[test]
fn two_roots_with_same_native_id_have_separate_references_and_real_reads() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let live = Arc::new(AtomicBool::new(true));
    let a = registry
        .register(grant(&t.path().join("a"), "left", live.clone()))
        .unwrap();
    let b = registry
        .register(grant(&t.path().join("b"), "right", live))
        .unwrap();
    assert_ne!(a.scope_id, b.scope_id);
    assert_ne!(a.source_root_key, b.source_root_key);
    for (source, title) in [(a, "left"), (b, "right")] {
        let r = registry.read(&owner(), &request(source.clone())).unwrap();
        assert_eq!(r.source, source);
        assert_eq!(r.request_epoch, n(9));
        assert_eq!(r.state, ProjectionState::Ready);
        assert!(matches!(&r.items[0],ResourceItem::Session{title:t,..} if t==title));
    }
}
#[test]
fn forged_owner_and_every_reference_dimension_fail_before_reader() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let live = Arc::new(AtomicBool::new(true));
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let mut g = grant(t.path(), "name", live);
    g.check = Arc::new(move || {
        c.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let source = registry.register(g).unwrap();
    let before = count.load(Ordering::SeqCst);
    for o in [
        Owner {
            epoch: n(2),
            ..owner()
        },
        Owner {
            instance: "peer".into(),
            ..owner()
        },
        Owner {
            window: "peer".into(),
            ..owner()
        },
    ] {
        assert_eq!(
            registry
                .read(&o, &request(source.clone()))
                .unwrap_err()
                .code,
            "FORBIDDEN"
        );
    }
    let mut refs = vec![];
    let mut r = source.clone();
    r.scope_id = "other".into();
    refs.push(r);
    let mut r = source.clone();
    r.instance_id = "other".into();
    refs.push(r);
    let mut r = source.clone();
    r.cli = CliKind::Codex;
    refs.push(r);
    let mut r = source.clone();
    r.source_root_key = "forged".into();
    refs.push(r);
    let mut r = source.clone();
    r.identity_epoch = n(2);
    refs.push(r);
    let mut r = source.clone();
    r.profile_revision = n(2);
    refs.push(r);
    let mut r = source.clone();
    r.basis = SourceBasis::LaunchEnvironment;
    refs.push(r);
    let mut r = source;
    r.target = ScopeTarget::Run {
        run_id: "run".into(),
        generation: 1,
    };
    refs.push(r);
    for r in refs {
        assert!(registry.read(&owner(), &request(r)).is_err());
    }
    assert_eq!(count.load(Ordering::SeqCst), before);
}
#[test]
fn revocation_before_or_during_read_never_returns_stale_items() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let live = Arc::new(AtomicBool::new(true));
    let source = registry
        .register(grant(t.path(), "old", live.clone()))
        .unwrap();
    live.store(false, Ordering::SeqCst);
    assert_eq!(
        registry.read(&owner(), &request(source)).unwrap_err().code,
        "SCOPE_REVOKED"
    );
    let checks = Arc::new(AtomicUsize::new(0));
    let c = checks.clone();
    let mut g = grant(t.path(), "old", Arc::new(AtomicBool::new(true)));
    g.check = Arc::new(move || {
        if c.fetch_add(1, Ordering::SeqCst) < 7 {
            Ok(())
        } else {
            Err("SCOPE_REVOKED")
        }
    });
    let source = registry.register(g).unwrap();
    assert_eq!(
        registry.read(&owner(), &request(source)).unwrap_err().code,
        "SCOPE_REVOKED"
    );
}
#[test]
fn replaced_root_is_unavailable_not_an_empty_ready_projection() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let p = t.path().join("active");
    let source = registry
        .register(grant(&p, "old", Arc::new(AtomicBool::new(true))))
        .unwrap();
    match fs::rename(&p, t.path().join("retired")) {
        Ok(()) => {
            fs::create_dir(&p).unwrap();
            let r = registry.read(&owner(), &request(source)).unwrap();
            assert_eq!(r.state, ProjectionState::Unavailable);
            assert_eq!(r.reason.as_deref(), Some("SOURCE_CHANGED"));
            assert!(r.items.is_empty());
            assert!(!r.has_more);
        }
        Err(e) => {
            #[cfg(not(windows))]
            panic!("unexpected rename denial: {e}");
            #[cfg(windows)]
            {
                // Delete-sharing denial can surface as ERROR_SHARING_VIOLATION
                // rather than Rust's PermissionDenied classification.
                assert!(
                    matches!(e.raw_os_error(), Some(5 | 32)),
                    "unexpected rename error: {e:?}"
                );
                let r = registry.read(&owner(), &request(source)).unwrap();
                assert_eq!(r.state, ProjectionState::Ready);
                assert!(matches!(&r.items[0],ResourceItem::Session{title,..} if title=="old"));
            }
        }
    }
}
#[test]
fn identical_grant_reuses_scope_and_capacity_cannot_grow_without_bound() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(1);
    let live = Arc::new(AtomicBool::new(true));
    let a = registry
        .register(grant(&t.path().join("a"), "a", live.clone()))
        .unwrap();
    assert_eq!(
        a,
        registry
            .register(grant(&t.path().join("a"), "a", live.clone()))
            .unwrap()
    );
    assert_eq!(
        registry
            .register(grant(
                &t.path().join("b"),
                "b",
                Arc::new(AtomicBool::new(true))
            ))
            .unwrap_err()
            .code,
        "SCOPE_CAPACITY"
    );
    live.store(false, Ordering::SeqCst);
    let b = registry
        .register(grant(
            &t.path().join("b"),
            "b",
            Arc::new(AtomicBool::new(true)),
        ))
        .unwrap();
    assert!(b.identity_epoch.get() > a.identity_epoch.get());
    assert!(registry.read(&owner(), &request(a)).is_err());
}
#[test]
fn malformed_history_keeps_positive_sessions() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = registry
        .register(grant(t.path(), "good", Arc::new(AtomicBool::new(true))))
        .unwrap();
    fs::write(
        t.path().join("projects/p/other.jsonl"),
        "SECRET malformed body",
    )
    .unwrap();
    let r = registry.read(&owner(), &request(source)).unwrap();
    assert_eq!(r.state, ProjectionState::Ready);
    assert_eq!(r.items.len(), 1);
    let encoded = serde_json::to_value(&r).unwrap();
    assert_eq!(encoded["historyMetadataIncomplete"], true);
    assert_eq!(
        encoded["historyReadFailures"],
        serde_json::json!(["SOURCE_INVALID"])
    );
    assert!(!encoded.to_string().contains("SECRET"));
}
// 只有无法识别的条目时仍明确不完整，不能伪装为缺失会话证明。
#[test]
fn unsupported_history_is_explicitly_partial() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = registry
        .register(grant(t.path(), "good", Arc::new(AtomicBool::new(true))))
        .unwrap();
    fs::write(t.path().join("projects/p/same.jsonl"), "").unwrap();
    fs::create_dir_all(t.path().join("projects/p/same/unknown/nested")).unwrap();
    let r = registry.read(&owner(), &request(source)).unwrap();
    assert_eq!(r.state, ProjectionState::Ready);
    assert!(r.items.is_empty());
    assert_eq!(r.history_metadata_incomplete, Some(true));
    assert_eq!(r.history_read_failures, vec!["SOURCE_UNSUPPORTED"]);
}
// 同一来源中的重复原生ID仍拒绝整个结果，不把歧义当作可略过内容错误。
#[test]
fn duplicate_ids_remain_unavailable() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = registry
        .register(grant(t.path(), "good", Arc::new(AtomicBool::new(true))))
        .unwrap();
    fs::create_dir_all(t.path().join("projects/q")).unwrap();
    fs::copy(
        t.path().join("projects/p/same.jsonl"),
        t.path().join("projects/q/same.jsonl"),
    )
    .unwrap();
    let r = registry.read(&owner(), &request(source)).unwrap();
    assert_eq!(r.state, ProjectionState::Unavailable);
    assert_eq!(r.reason.as_deref(), Some("SOURCE_AMBIGUOUS"));
    assert!(r.items.is_empty());
    assert!(r.history_read_failures.is_empty());
}
#[test]
fn pagination_is_explicit_and_invalid_query_never_enters_reader() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = registry
        .register(grant(t.path(), "one", Arc::new(AtomicBool::new(true))))
        .unwrap();
    fs::write(
        t.path().join("projects/p/two.jsonl"),
        "{\"type\":\"custom-title\",\"customTitle\":\"two\"}\n",
    )
    .unwrap();
    let mut req = request(source);
    req.limit = 1;
    let r = registry.read(&owner(), &req).unwrap();
    assert_eq!(r.items.len(), 1);
    assert!(r.has_more);
    req.offset = 1;
    assert!(!registry.read(&owner(), &req).unwrap().has_more);
    req.query = Some("not-for-history".into());
    assert_eq!(
        registry.read(&owner(), &req).unwrap_err().code,
        "INVALID_REQUEST"
    );
}

// Consecutive History pages belong to one bounded observation; pagination must
// not enumerate and parse the same root again for each 200-row IPC page.
#[test]
fn history_pages_reuse_one_authenticated_observation() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let checks = Arc::new(AtomicUsize::new(0));
    let mut g = grant(t.path(), "main", Arc::new(AtomicBool::new(true)));
    for index in 0..200 {
        fs::write(
            t.path().join(format!("projects/p/{index:03}.jsonl")),
            "{\"type\":\"user\",\"cwd\":\"/repo\",\"message\":{\"content\":\"task\"}}\n",
        )
        .unwrap();
    }
    let observed = checks.clone();
    g.check = Arc::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let source = registry.register(g).unwrap();
    let mut page = request(source);
    checks.store(0, Ordering::SeqCst);
    let first = registry.read(&owner(), &page).unwrap();
    assert_eq!(first.state, ProjectionState::Ready);
    assert!(first.has_more);
    assert!(checks.load(Ordering::SeqCst) > 200);
    checks.store(0, Ordering::SeqCst);
    page.offset = 100;
    let second = registry.read(&owner(), &page).unwrap();
    assert_eq!(second.state, ProjectionState::Ready);
    assert_eq!(second.observed_at, first.observed_at);
    assert!(
        checks.load(Ordering::SeqCst) < 20,
        "a continuation re-scanned the source"
    );
    assert_eq!(second.items.len(), 100);
}

fn paged_source(
    path: &std::path::Path,
    registry: &ScopeRegistry,
    live: Arc<AtomicBool>,
) -> SourceRef {
    let g = grant(path, "original", live);
    fs::write(
        path.join("projects/p/two.jsonl"),
        "{\"type\":\"custom-title\",\"customTitle\":\"two\"}\n",
    )
    .unwrap();
    registry.register(g).unwrap()
}
#[test]
fn history_snapshot_fresh_load_never_reuses_prior_items() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = paged_source(t.path(), &registry, Arc::new(AtomicBool::new(true)));
    let mut req = request(source);
    req.limit = 1;
    assert!(registry.read(&owner(), &req).unwrap().has_more);
    fs::write(
        t.path().join("projects/p/same.jsonl"),
        "{\"type\":\"custom-title\",\"customTitle\":\"fresh\"}\n",
    )
    .unwrap();
    req.request_epoch = n(10);
    let fresh = registry.read(&owner(), &req).unwrap();
    assert!(matches!(&fresh.items[0], ResourceItem::Session { title, .. } if title == "fresh"));
    req.request_epoch = n(9);
    req.offset = 1;
    let stale = registry.read(&owner(), &req).unwrap();
    assert_eq!(stale.reason.as_deref(), Some("SOURCE_SNAPSHOT_EXPIRED"));
    assert!(stale.items.is_empty());
}
#[test]
fn history_snapshot_rechecks_revoked_profile_or_project_authority() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let live = Arc::new(AtomicBool::new(true));
    let source = paged_source(t.path(), &registry, live.clone());
    let mut req = request(source);
    req.limit = 1;
    assert!(registry.read(&owner(), &req).unwrap().has_more);
    live.store(false, Ordering::SeqCst);
    req.offset = 1;
    assert_eq!(
        registry.read(&owner(), &req).unwrap_err().code,
        "SCOPE_REVOKED"
    );
}
#[test]
fn history_snapshot_old_scan_cannot_replace_a_newer_load() {
    let t = tempfile::tempdir().unwrap();
    let registry = Arc::new(ScopeRegistry::new(4));
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let old_checks = Arc::new(AtomicUsize::new(0));
    let mut g = grant(t.path(), "original", Arc::new(AtomicBool::new(true)));
    fs::write(
        t.path().join("projects/p/two.jsonl"),
        "{\"type\":\"custom-title\",\"customTitle\":\"two\"}\n",
    )
    .unwrap();
    let (e, r, c) = (entered.clone(), release.clone(), old_checks.clone());
    g.check = Arc::new(move || {
        if std::thread::current().name() == Some("old-history-scan")
            && c.fetch_add(1, Ordering::SeqCst) == 2
        {
            e.wait();
            r.wait();
        }
        Ok(())
    });
    let source = registry.register(g).unwrap();
    let mut req = request(source);
    req.limit = 1;
    let (old_registry, old_req) = (registry.clone(), req.clone());
    let old = std::thread::Builder::new()
        .name("old-history-scan".into())
        .spawn(move || old_registry.read(&owner(), &old_req))
        .unwrap();
    entered.wait();
    req.request_epoch = n(10);
    assert!(registry.read(&owner(), &req).unwrap().has_more);
    release.wait();
    let retired = old.join().unwrap().unwrap();
    assert_eq!(retired.reason.as_deref(), Some("SOURCE_CHANGED"));
    req.offset = 1;
    assert_eq!(
        registry.read(&owner(), &req).unwrap().state,
        ProjectionState::Ready
    );
}

#[test]
fn history_snapshot_capacity_ttl_and_final_page_retirement_are_bounded() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let mut requests = Vec::new();
    for index in 0..3 {
        let source = paged_source(
            &t.path().join(index.to_string()),
            &registry,
            Arc::new(AtomicBool::new(true)),
        );
        let mut req = request(source);
        req.limit = 1;
        assert!(registry.read(&owner(), &req).unwrap().has_more);
        requests.push(req);
    }
    assert_eq!(registry.history_pages.lock().unwrap().len(), 3);
    requests[0].offset = 1;
    assert_eq!(
        registry.read(&owner(), &requests[0]).unwrap().state,
        ProjectionState::Ready
    );
    {
        let mut pages = registry.history_pages.lock().unwrap();
        let page = Arc::get_mut(&mut pages[0]).unwrap();
        page.created =
            std::time::Instant::now() - HISTORY_SNAPSHOT_TTL - std::time::Duration::from_millis(1);
    }
    requests[1].offset = 1;
    assert_eq!(
        registry
            .read(&owner(), &requests[1])
            .unwrap()
            .reason
            .as_deref(),
        Some("SOURCE_SNAPSHOT_EXPIRED")
    );
    requests[2].offset = 1;
    let last = registry.read(&owner(), &requests[2]).unwrap();
    assert_eq!(last.state, ProjectionState::Ready);
    assert!(!last.has_more);
    assert!(registry.history_pages.lock().unwrap().is_empty());
}
#[test]
fn history_snapshot_accounts_allocated_capacity_and_retains_partial_flags() {
    let mut title = String::with_capacity(HISTORY_SNAPSHOT_BYTES);
    title.push('x');
    let items = vec![ResourceItem::Session {
        session_key: "key".into(),
        native_session_id: "id".into(),
        title,
        title_unknown: None,
        title_source: None,
        metadata_incomplete: None,
        cwd: None,
        updated_at: None,
        truncated: true,
    }];
    assert!(history_snapshot_bytes(&items, items.capacity()) > HISTORY_SNAPSHOT_BYTES);
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = paged_source(t.path(), &registry, Arc::new(AtomicBool::new(true)));
    fs::write(
        t.path().join("projects/p/bad.jsonl"),
        "{\"type\":\"unknown\"}\n",
    )
    .unwrap();
    let mut req = request(source);
    req.limit = 1;
    let first = registry.read(&owner(), &req).unwrap();
    req.offset = 1;
    let second = registry.read(&owner(), &req).unwrap();
    assert_eq!(second.observed_at, first.observed_at);
    assert_eq!(second.history_metadata_incomplete, Some(true));
    assert_eq!(second.history_read_failures, vec!["SOURCE_UNSUPPORTED"]);
}
#[test]
fn history_snapshot_rechecks_root_and_registered_project_handles() {
    for replace_project in [false, true] {
        let t = tempfile::tempdir().unwrap();
        let registry = ScopeRegistry::new(4);
        let root_path = t.path().join("source");
        let project_path = t.path().join("registered");
        fs::create_dir(&project_path).unwrap();
        let mut g = grant(&root_path, "original", Arc::new(AtomicBool::new(true)));
        for name in ["same", "two"] {
            fs::write(root_path.join(format!("projects/p/{name}.jsonl")), format!("{}\n", serde_json::json!({"type":"user", "cwd":project_path.to_str().unwrap(),"message":{"content":name}}))).unwrap();
        }
        g.project = Some(Root::open(&project_path).unwrap());
        g.project_paths = vec![project_path.clone()];
        let source = registry.register(g).unwrap();
        let mut req = request(source);
        req.limit = 1;
        assert!(registry.read(&owner(), &req).unwrap().has_more);
        let replaced = if replace_project {
            &project_path
        } else {
            &root_path
        };
        match fs::rename(replaced, t.path().join("retired")) {
            Ok(()) => {
                fs::create_dir(replaced).unwrap();
                req.offset = 1;
                let unavailable = registry.read(&owner(), &req).unwrap();
                assert_eq!(unavailable.reason.as_deref(), Some("SOURCE_CHANGED"));
                assert!(unavailable.items.is_empty());
            }
            Err(error) => {
                #[cfg(not(windows))]
                panic!("unexpected rename denial: {error}");
                #[cfg(windows)]
                {
                    assert!(
                        matches!(error.raw_os_error(), Some(5 | 32)),
                        "unexpected rename error: {error:?}"
                    );
                    req.offset = 1;
                    assert_eq!(
                        registry.read(&owner(), &req).unwrap().state,
                        ProjectionState::Ready
                    );
                }
            }
        }
    }
}

#[test]
fn review_probe_old_token_miss_cannot_retire_new_same_epoch_snapshot() {
    let t = tempfile::tempdir().unwrap();
    let registry = Arc::new(ScopeRegistry::new(4));
    let entered = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let checks = Arc::new(AtomicUsize::new(0));
    let mut g = grant(t.path(), "original", Arc::new(AtomicBool::new(true)));
    fs::write(
        t.path().join("projects/p/two.jsonl"),
        "{\"type\":\"custom-title\",\"customTitle\":\"two\"}\n",
    )
    .unwrap();
    let (e, r, c) = (entered.clone(), release.clone(), checks.clone());
    g.check = Arc::new(move || {
        if std::thread::current().name() == Some("old-missing-history-page")
            && c.fetch_add(1, Ordering::SeqCst) == 1
        {
            e.wait();
            r.wait();
        }
        Ok(())
    });
    let source = registry.register(g).unwrap();
    let mut old_req = request(source.clone());
    old_req.limit = 1;
    old_req.offset = 1;
    let old_registry = registry.clone();
    let old = std::thread::Builder::new()
        .name("old-missing-history-page".into())
        .spawn(move || old_registry.read(&owner(), &old_req))
        .unwrap();
    entered.wait();
    let mut new_req = request(source);
    new_req.limit = 1;
    assert!(registry.read(&owner(), &new_req).unwrap().has_more);
    release.wait();
    assert_eq!(
        old.join().unwrap().unwrap().reason.as_deref(),
        Some("SOURCE_CHANGED")
    );
    new_req.offset = 1;
    assert_eq!(
        registry.read(&owner(), &new_req).unwrap().state,
        ProjectionState::Ready,
        "late token miss removed a newer same-epoch snapshot"
    );
}

#[test]
fn old_first_page_retirement_cannot_remove_newer_generation() {
    let t = tempfile::tempdir().unwrap();
    let registry = ScopeRegistry::new(4);
    let source = paged_source(t.path(), &registry, Arc::new(AtomicBool::new(true)));
    let mut req = request(source.clone());
    req.limit = 1;
    registry.read(&owner(), &req).unwrap();
    let scope = registry.state.lock().unwrap().scopes[&source.scope_id].clone();
    let older = scope.history_generation.load(Ordering::SeqCst);
    req.request_epoch = n(10);
    registry.read(&owner(), &req).unwrap();
    assert_eq!(
        registry
            .retire_history_pages(&scope, older)
            .unwrap_err()
            .code,
        "SOURCE_CHANGED"
    );
    req.offset = 1;
    assert_eq!(
        registry.read(&owner(), &req).unwrap().state,
        ProjectionState::Ready
    );
}
