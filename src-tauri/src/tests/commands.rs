use crate::commands::{dispatch_indexed_store, spawn_blocking_store, validate_display_name_inner};
use crate::session_name_index::{
    IndexedResult, PendingIndexFlush, RawIndexSnapshot, ResolutionStats, SessionNameIndexDelta,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn run_async<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

// spawn_blocking 内 store 业务错误向前端透传（不吞错）。
#[test]
fn SpawnBlockingStore_PropagatesError_001() {
    let error = run_async(spawn_blocking_store::<_, ()>("fixture", || {
        anyhow::bail!("store failed")
    }))
    .unwrap_err();
    assert!(error.contains("store failed"));
}

// spawn_blocking 任务 panic 转 JoinError，错误串含 command 标签与 blocking 提示。
#[test]
fn SpawnBlockingStore_LabelsJoinError_002() {
    let error = run_async(spawn_blocking_store::<_, ()>("fixture", || {
        panic!("worker panic")
    }))
    .unwrap_err();
    assert!(error.contains("fixture"));
    assert!(error.contains("blocking task failed"));
}

fn pending_flush_fixture() -> PendingIndexFlush {
    PendingIndexFlush {
        base_raw: RawIndexSnapshot::Missing,
        delta: SessionNameIndexDelta {
            request_compaction: true,
            ..SessionNameIndexDelta::default()
        },
    }
}

// command 必须在后台 flush 完成前返回业务值。
#[test]
fn IndexDispatch_ReturnsBeforeFlush_010() {
    run_async(async {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let future = dispatch_indexed_store(
            "fixture",
            || {
                Ok(IndexedResult {
                    value: 42,
                    pending_flush: Some(pending_flush_fixture()),
                    stats: ResolutionStats::default(),
                })
            },
            move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok(())
            },
        );

        let value = tokio::time::timeout(Duration::from_millis(200), future)
            .await
            .expect("dispatch must not wait for flush")
            .unwrap();
        assert_eq!(value, 42);
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        release_tx.send(()).unwrap();
    });
}

// 无 delta 时不得创建后台 flush job。
#[test]
fn IndexDispatch_NoDelta_NoSpawn_011() {
    let calls = Arc::new(AtomicU64::new(0));
    let captured = Arc::clone(&calls);

    let value = run_async(dispatch_indexed_store(
        "fixture",
        || {
            Ok(IndexedResult {
                value: 7,
                pending_flush: None,
                stats: ResolutionStats::default(),
            })
        },
        move |_| {
            captured.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    ))
    .unwrap();

    assert_eq!(value, 7);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

// detached flush 失败不能改变已经返回的业务值。
#[test]
fn IndexDispatch_FlushError_Ignored_012() {
    let (called_tx, called_rx) = mpsc::channel();
    let value = run_async(async {
        let value = dispatch_indexed_store(
            "fixture",
            || {
                Ok(IndexedResult {
                    value: "business",
                    pending_flush: Some(pending_flush_fixture()),
                    stats: ResolutionStats::default(),
                })
            },
            move |_| {
                called_tx.send(()).unwrap();
                anyhow::bail!("disk full")
            },
        )
        .await
        .unwrap();
        called_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        value
    });

    assert_eq!(value, "business");
}

// all-recent 等 indexed command 的 blocking 业务错误必须正常向 IPC 传播。
#[test]
fn AllRecent_BlockingError_013() {
    let error = run_async(dispatch_indexed_store::<_, Vec<()>, _>(
        "get_all_recent_sessions",
        || anyhow::bail!("scan failed"),
        |_| Ok(()),
    ))
    .unwrap_err();

    assert!(error.contains("scan failed"));
}

// CAS exhausted 属于后台派生写回失败，不得撤销已返回结果。
#[test]
fn IndexDispatch_CasExhausted_Ignored_014() {
    let (called_tx, called_rx) = mpsc::channel();
    let value = run_async(async {
        let value = dispatch_indexed_store(
            "fixture",
            || {
                Ok(IndexedResult {
                    value: 99,
                    pending_flush: Some(pending_flush_fixture()),
                    stats: ResolutionStats::default(),
                })
            },
            move |_| {
                called_tx.send(()).unwrap();
                anyhow::bail!("session name index whole-file CAS exhausted")
            },
        )
        .await
        .unwrap();
        called_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        value
    });

    assert_eq!(value, 99);
}

// 后端必须与浏览器 maxlength/前端 raw.length 一致，按原始 UTF-16 code unit 计数。
#[test]
fn ValidateDisplayName_Utf16Boundary_001() {
    assert!(validate_display_name_inner(&"😀".repeat(16)).is_ok());
    assert!(validate_display_name_inner(&"😀".repeat(17)).is_err());
}

// 校验发生在 trim 之前，不能让控制字符或超长纯空白绕过前端规则。
#[test]
fn ValidateDisplayName_RawInputBeforeTrim_001() {
    assert!(validate_display_name_inner("\t").is_err());
    assert!(validate_display_name_inner(&" ".repeat(33)).is_err());
    assert!(validate_display_name_inner(&" ".repeat(32)).is_ok());
}
