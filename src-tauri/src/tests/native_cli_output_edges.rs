use super::native_cli_routed_launch::Fixture;
use crate::cli::launch::LaunchCoordinator;
use crate::cli::output_route::{OutputRoute, OutputRoutes};
use crate::cli::profiles::error;
use crate::cli::routed_launch::RoutedResource;
use crate::cli::run_registry::LaunchPhase;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tauri::ipc::Channel;

// 检查发送回调展开异常后不再投递，避免结果不明时发送后续正文。
#[test]
fn D11_Channel_SendPanicCloses_012() {
    let calls = Arc::new(AtomicUsize::new(0));
    let sent = calls.clone();
    let route = OutputRoutes::new(1)
        .bind(1, Box::new(|| Ok(())), || {
            Ok(Channel::new(move |_| {
                if sent.fetch_add(1, Ordering::SeqCst) == 0 {
                    panic!("synthetic send unwind");
                }
                Ok(())
            }))
        })
        .unwrap();
    assert!(catch_unwind(AssertUnwindSafe(|| route.send(json!("first")))).is_err());
    assert_eq!(
        route.send(json!("later")).unwrap_err().code,
        "OUTPUT_ROUTE_CLOSED"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// 检查生产协调器重放不新建或释放原路由，退出且回收后才释放槽位。
#[test]
fn D11_Channel_ReplayKeepsLease_013() {
    let mut f = Fixture::<usize>::new();
    let driver = LaunchCoordinator::<RoutedResource<usize, OutputRoute<Value>>>::new(4);
    f.caller = driver.registry().activate_window("main").unwrap();
    let routes = OutputRoutes::new(1);
    let status = driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| routes.bind(3, Box::new(|| Ok(())), || Ok(Channel::new(|_| Ok(())))) ,
            |_| Ok(7),
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Running);
    let replay = driver
        .start_routed(
            &f.caller,
            &f.request,
            || panic!("replay read profile"),
            |_| panic!("replay replaced channel"),
            |_| panic!("replay spawned process"),
        )
        .unwrap();
    assert_eq!(status, replay);
    driver.registry().mark_exited(&status.run).unwrap();
    assert_eq!(
        routes.bind::<Value>(3, Box::new(|| Ok(())), || panic!("early release"))
            .unwrap_err().code,
        "OUTPUT_CHANNEL_BUSY"
    );
    driver.registry().retire(&status.run).unwrap();
    assert!(routes.bind(3, Box::new(|| Ok(())), || Ok(Channel::<Value>::new(|_| Ok(())))).is_ok());
}

// 检查启动失败释放已分配路由，但失败回执仍阻止重建与重新启动。
#[test]
fn D11_Channel_FailureKeepsReceipt_014() {
    let mut f = Fixture::<usize>::new();
    let driver = LaunchCoordinator::<RoutedResource<usize, OutputRoute<Value>>>::new(4);
    f.caller = driver.registry().activate_window("main").unwrap();
    let routes = OutputRoutes::new(1);
    let status = driver
        .start_routed(
            &f.caller,
            &f.request,
            || f.freeze(),
            |_| routes.bind(3, Box::new(|| Ok(())), || Ok(Channel::new(|_| Ok(())))) ,
            |_| Err(error("SYNTHETIC_START_FAILURE")),
        )
        .unwrap();
    assert_eq!(status.phase, LaunchPhase::Failed);
    let next = routes.bind(3, Box::new(|| Ok(())), || Ok(Channel::<Value>::new(|_| Ok(())))).unwrap();
    let replay = driver
        .start_routed(
            &f.caller,
            &f.request,
            || panic!("failed replay read profile"),
            |_| panic!("failed replay replaced channel"),
            |_| panic!("failed replay spawned process"),
        )
        .unwrap();
    assert_eq!(status, replay);
    next.send(json!("independent route remains usable")).unwrap();
}
