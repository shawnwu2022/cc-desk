use crate::cli::output_route::{parse_channel, OutputRoutes, CHANNEL_HEADER};
use crate::cli::profiles::error;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use tauri::http::HeaderMap;
use tauri::ipc::{Channel, InvokeResponseBody};

// 检查只接收规范 u32 Channel 描述，拒绝重复、空值和非规范编号。
#[test]
fn D11_Channel_Descriptor_001() {
    for (text, id) in [("__CHANNEL__:0", 0), ("__CHANNEL__:4294967295", u32::MAX)] {
        let mut headers = HeaderMap::new();
        headers.insert(CHANNEL_HEADER, text.parse().unwrap());
        assert_eq!(parse_channel(&headers).unwrap(), id);
    }
    for text in [
        "",
        "0",
        "__CHANNEL__:01",
        "__CHANNEL__:+1",
        "__CHANNEL__:4294967296",
        "__CHANNEL__:1, __CHANNEL__:2",
        "private-value",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(CHANNEL_HEADER, text.parse().unwrap());
        let failure = parse_channel(&headers).unwrap_err();
        assert_eq!(failure.code, "INVALID_REQUEST");
        assert!(!format!("{failure:?}").contains(text) || text.is_empty());
    }
    let mut headers = HeaderMap::new();
    assert!(parse_channel(&headers).is_err());
    headers.append(CHANNEL_HEADER, "__CHANNEL__:1".parse().unwrap());
    headers.append(CHANNEL_HEADER, "__CHANNEL__:1".parse().unwrap());
    assert!(parse_channel(&headers).is_err());
}

// 检查未授权调用不分配回调、不执行 Channel 构造器。
#[test]
fn D11_Channel_AuthBeforeFactory_002() {
    let routes = OutputRoutes::new(1);
    let result = routes.bind::<Value>(0, Box::new(|| Err(error("FORBIDDEN"))), || {
        panic!("unauthorized factory")
    });
    assert_eq!(result.unwrap_err().code, "FORBIDDEN");
    assert!(routes
        .bind(0, Box::new(|| Ok(())), || Ok(Channel::<Value>::new(
            |_| Ok(())
        )))
        .is_ok());
}

// 检查回调独占、容量拒绝及最后一个路由引用释放后的回收。
#[test]
fn D11_Channel_LeaseAndCapacity_003() {
    let routes = OutputRoutes::new(1);
    let route = Arc::new(
        routes
            .bind(7, Box::new(|| Ok(())), || {
                Ok(Channel::<Value>::new(|_| Ok(())))
            })
            .unwrap(),
    );
    assert_eq!(
        routes
            .bind::<Value>(7, Box::new(|| Ok(())), || panic!("duplicate factory"))
            .unwrap_err()
            .code,
        "OUTPUT_CHANNEL_BUSY"
    );
    assert_eq!(
        routes
            .bind::<Value>(8, Box::new(|| Ok(())), || panic!("capacity factory"))
            .unwrap_err()
            .code,
        "OUTPUT_ROUTE_CAPACITY"
    );
    let reader_reference = route.clone();
    drop(route);
    assert!(routes
        .bind::<Value>(7, Box::new(|| Ok(())), || panic!("live reader factory"))
        .is_err());
    drop(reader_reference);
    assert!(routes
        .bind(8, Box::new(|| Ok(())), || Ok(Channel::<Value>::new(
            |_| Ok(())
        )))
        .is_ok());
}

// 检查 Channel 构造失败或展开异常都会释放未提交的路由租约。
#[test]
fn D11_Channel_FactoryRollback_004() {
    let routes = OutputRoutes::new(1);
    assert_eq!(
        routes
            .bind::<Value>(3, Box::new(|| Ok(())), || Err(error("FACTORY_FAILED")))
            .unwrap_err()
            .code,
        "FACTORY_FAILED"
    );
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _ = routes.bind::<Value>(3, Box::new(|| Ok(())), || panic!("factory panic"));
    }))
    .is_err());
    assert!(routes
        .bind(3, Box::new(|| Ok(())), || Ok(Channel::<Value>::new(
            |_| Ok(())
        )))
        .is_ok());
}

// 检查构造 Channel 时身份失效，不返回可发送的路由。
#[test]
fn D11_Channel_RevokeDuringBind_005() {
    let routes = OutputRoutes::new(1);
    let active = Arc::new(AtomicBool::new(true));
    let checked = active.clone();
    let failure = routes
        .bind(
            1,
            Box::new(move || {
                if checked.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err(error("FORBIDDEN"))
                }
            }),
            || {
                active.store(false, Ordering::SeqCst);
                Ok(Channel::<Value>::new(|_| panic!("revoked route sent")))
            },
        )
        .unwrap_err();
    assert_eq!(failure.code, "FORBIDDEN");
    assert!(routes
        .bind(2, Box::new(|| Ok(())), || Ok(Channel::<Value>::new(
            |_| Ok(())
        )))
        .is_ok());
}

// 检查每次发送重新鉴权，撤权后正文不进入 Channel。
#[test]
fn D11_Channel_RevokeStopsSend_006() {
    let routes = OutputRoutes::new(1);
    let active = Arc::new(AtomicBool::new(true));
    let checked = active.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let sent = calls.clone();
    let route = routes
        .bind(
            1,
            Box::new(move || {
                if checked.load(Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err(error("FORBIDDEN"))
                }
            }),
            || {
                Ok(Channel::new(move |_| {
                    sent.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }))
            },
        )
        .unwrap();
    route.send(json!({"bytes":[0,255,27]})).unwrap();
    active.store(false, Ordering::SeqCst);
    assert_eq!(
        route.send(json!("not-delivered")).unwrap_err().code,
        "FORBIDDEN"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

// 检查发送错误成为终态，不重发、不让后续帧越过失败帧。
#[test]
fn D11_Channel_SendFailureIsFinal_007() {
    let calls = Arc::new(AtomicUsize::new(0));
    let sent = calls.clone();
    let route = OutputRoutes::new(1)
        .bind(1, Box::new(|| Ok(())), || {
            Ok(Channel::new(move |_| {
                sent.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::other("private-transport-error").into())
            }))
        })
        .unwrap();
    let failure = route.send(json!("private-body")).unwrap_err();
    assert_eq!(failure.code, "OUTPUT_ROUTE_LOST");
    assert_eq!(
        route.send(json!("later")).unwrap_err().code,
        "OUTPUT_ROUTE_CLOSED"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!format!("{failure:?}{route:?}").contains("private"));
}

// 检查真实 Channel 序列化保留字节数组与十进制 offset，不进行文本解码。
#[test]
fn D11_Channel_ExactWireBytes_008() {
    let received = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sink = received.clone();
    let route = OutputRoutes::new(1)
        .bind(9, Box::new(|| Ok(())), || {
            Ok(Channel::new(move |body| {
                let InvokeResponseBody::Json(text) = body else {
                    panic!("JSON event expected");
                };
                sink.lock().push(serde_json::from_str(&text).unwrap());
                Ok(())
            }))
        })
        .unwrap();
    let event = json!({"runId":"run","generation":7,"offset":"9007199254740993","bytes":[0,255,27,91,50,48,48,126,228,184,173]});
    route.send(event.clone()).unwrap();
    assert_eq!(*received.lock(), vec![event]);
}

// 检查构造器可以重入其他回调的登记，不持有回调表全局锁。
#[test]
fn D11_Channel_FactoryOutsideLock_009() {
    let routes = OutputRoutes::new(2);
    let route = routes
        .bind(1, Box::new(|| Ok(())), || {
            let other = routes.bind(2, Box::new(|| Ok(())), || {
                Ok(Channel::<Value>::new(|_| Ok(())))
            })?;
            other.send(json!("other"))?;
            Ok(Channel::<Value>::new(|_| Ok(())))
        })
        .unwrap();
    route.send(json!("first")).unwrap();
}

// 检查并发争用同一回调只构造一个 Channel，成功路由留存到线程结束。
#[test]
fn D11_Channel_ConcurrentBind_010() {
    let routes = OutputRoutes::new(32);
    let calls = AtomicUsize::new(0);
    let barrier = Barrier::new(32);
    let retained = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..32 {
            scope.spawn(|| {
                barrier.wait();
                let result = routes.bind(1, Box::new(|| Ok(())), || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Channel::<Value>::new(|_| Ok(())))
                });
                if let Ok(route) = result {
                    retained.lock().push(route);
                }
            });
        }
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(retained.lock().len(), 1);
}
