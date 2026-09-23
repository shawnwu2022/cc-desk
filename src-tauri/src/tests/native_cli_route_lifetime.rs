use crate::cli::output_route::OutputRoutes;
use crate::cli::profiles::error;
use serde_json::{json, Value};
use std::sync::{mpsc, Arc};
use std::time::Duration;
use tauri::ipc::Channel;

// Missing production method: RED-only interface scaffold.
impl OutputRoutes { pub(crate) fn revoke(&self) {} }

// 撤权必须释放 Channel 和鉴权闭包持有的宿主引用，不能只设置授权标志。
#[test]
fn D11_Lifetime_RevokeDropsNativeOwners_001() {
    let routes = OutputRoutes::new(2);
    let host = Arc::new(());
    let weak = Arc::downgrade(&host);
    let guard_owner = host.clone();
    let channel_owner = host.clone();
    let route = routes.bind(1, Box::new(move || { let _ = &guard_owner; Ok(()) }), || {
        Ok(Channel::<Value>::new(move |_| { let _ = &channel_owner; Ok(()) }))
    }).unwrap();
    drop(host);
    assert!(weak.upgrade().is_some());
    routes.revoke();
    assert!(weak.upgrade().is_none(), "revoked route retains native manager");
    assert_eq!(route.send(json!(null)).unwrap_err().code, "FORBIDDEN");
    assert_eq!(routes.bind::<Value>(2, Box::new(|| Ok(())), || panic!("revoked factory")).unwrap_err().code, "FORBIDDEN");
}

// 传输失败同样释放鉴权闭包，保留路由对象不应延长宿主生命周期。
#[test]
fn D11_Lifetime_SendFailureDropsGuard_002() {
    let host = Arc::new(());
    let weak = Arc::downgrade(&host);
    let route = OutputRoutes::new(1).bind(1, Box::new(move || { let _ = &host; Ok(()) }), || {
        Ok(Channel::<Value>::new(|_| Err(std::io::Error::other("fixture").into())))
    }).unwrap();
    assert_eq!(route.send(json!(null)).unwrap_err().code, "OUTPUT_ROUTE_LOST");
    assert!(weak.upgrade().is_none(), "failed route retains admission guard");
    assert_eq!(route.send(json!(null)).unwrap_err().code, "OUTPUT_ROUTE_CLOSED");
}

// UI 撤权不能等待正在发送的线程，否则可能与 WebView 主线程形成互等。
#[test]
fn D11_Lifetime_RevokeDoesNotWaitForDispatch_003() {
    let routes = Arc::new(OutputRoutes::new(1));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let receiver = parking_lot::Mutex::new(release_rx);
    let host = Arc::new(());
    let weak = Arc::downgrade(&host);
    let route = Arc::new(routes.bind(1, Box::new(|| Ok(())), || {
        Ok(Channel::<Value>::new(move |_| {
            let _ = &host;
            entered_tx.send(()).unwrap();
            receiver.lock().recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        }))
    }).unwrap());
    let sending = route.clone();
    let sender = std::thread::spawn(move || sending.send(json!(null)));
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let closing = routes.clone();
    let (closed_tx, closed_rx) = mpsc::channel();
    let closer = std::thread::spawn(move || { closing.revoke(); closed_tx.send(()).unwrap(); });
    let closed = closed_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    closer.join().unwrap();
    // 已投递的这一帧允许完成；撤权并不声称能撤回它。
    let _ = sender.join().unwrap();
    assert!(closed.is_ok(), "revocation blocked behind dispatch");
    assert!(weak.upgrade().is_none(), "dispatch restored a revoked native owner");
    assert_eq!(route.send(json!(null)).unwrap_err().code, "FORBIDDEN");
}

// 工厂在锁外运行期间发生撤权，不能提交新路由或重新开放表。
#[test]
fn D11_Lifetime_RevokeDuringFactory_004() {
    let routes = OutputRoutes::new(1);
    let result = routes.bind(1, Box::new(|| Ok(())), || {
        routes.revoke();
        Ok(Channel::<Value>::new(|_| Ok(())))
    });
    assert_eq!(result.unwrap_err().code, "FORBIDDEN");
}

// 拒绝后的闭包析构必须在路由表锁之外，以允许宿主做自己的清理。
#[test]
fn D11_Lifetime_ClosedTableRejectsBeforeFactory_005() {
    let routes = OutputRoutes::new(1);
    routes.revoke();
    assert_eq!(routes.bind::<Value>(0, Box::new(|| Err(error("FORBIDDEN"))), || panic!("factory")).unwrap_err().code, "FORBIDDEN");
}
