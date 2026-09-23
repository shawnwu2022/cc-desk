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
                assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
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
fn malformed_files_return_no_partial_success_and_no_raw_error_text() {
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
    assert_eq!(r.state, ProjectionState::Unavailable);
    assert!(r.items.is_empty());
    assert!(!serde_json::to_string(&r).unwrap().contains("SECRET"));
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
