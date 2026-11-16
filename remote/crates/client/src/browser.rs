//! 固定私有RPC只调用原NativeClient；不解析或另造应用协议。
use super::{DataError, NativeClient, remaining};
use cc_desk_remote_security::BrowserApplicationFailure as Failure;
use std::{
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Poll, Waker},
    time::Duration,
};
const CAP: usize = 32768;
#[link(wasm_import_module = "cc_desk_browser_client_v1")]
unsafe extern "C" {
    fn request(output: *mut u8, capacity: u32) -> i32;
    fn reply(sequence: u32, status: i32, pointer: *const u8, length: u32);
    fn descriptor(pointer: *const u8, length: u32);
    fn invalidated();
}
static WAITER: Mutex<Option<Waker>> = Mutex::new(None);
pub extern "C" fn browser_client_start(local: u32, peer: u32, budget: u32) -> i32 {
    cc_desk_remote_security::browser_application_start(local, peer, budget, application)
}
pub extern "C" fn browser_client_event(kind: u32) {
    if kind == 7 {
        let wake = WAITER.lock().unwrap().take();
        if let Some(wake) = wake {
            wake.wake();
        }
    } else {
        cc_desk_remote_security::browser_tls_event(kind);
    }
}
fn application(
    channel: cc_desk_remote_security::RunningChannel,
) -> Pin<Box<dyn Future<Output = Result<(), Failure>>>> {
    Box::pin(async move {
        let proof = match channel.client_installed().await {
            Ok(proof) => proof,
            Err(error) => {
                channel.authority().request_close();
                let retired = channel.wait_closed_checked().await;
                unsafe { invalidated() };
                retired.map_err(Failure::Retirement)?;
                return Err(Failure::Operation(error));
            }
        };
        let client = match NativeClient::from_installed_retained(channel, proof) {
            Ok(client) => client,
            Err(original) => {
                original.1.authority().request_close();
                let retired = original.1.wait_closed_checked().await;
                unsafe { invalidated() };
                retired.map_err(Failure::Retirement)?;
                return Err(Failure::Operation(original.0));
            }
        };
        let result = run(&client).await;
        unsafe { invalidated() };
        // 原driver/应用future都归同一个root持有，关闭观察不会代替driver完成。
        let retired = client.retire().await;
        retired.map_err(Failure::Retirement)?;
        result.map_err(Failure::Operation)?;
        Ok(())
    })
}
async fn run(client: &NativeClient) -> Result<(), DataError> {
    let value = client.descriptor()?;
    let bytes = serde_json::to_vec(&serde_json::json!({"target":value.target,"binding":value.binding,"role":value.role,"fence":client.fence.to_string()})).map_err(|_|DataError::Codec)?;
    if bytes.len() > 4096 {
        return Err(DataError::Bounds);
    }
    client.channel.authority().check()?;
    unsafe { descriptor(bytes.as_ptr(), bytes.len() as u32) };
    let mut sequence = 0u32;
    loop {
        let mut buffer = vec![0u8; CAP];
        let read = std::future::poll_fn(|cx| {
            *WAITER.lock().unwrap() = Some(cx.waker().clone());
            match unsafe { request(buffer.as_mut_ptr(), CAP as u32) } {
                -1 => Poll::Pending,
                n if n >= 20 && n as usize <= CAP => Poll::Ready(Ok(n as usize)),
                _ => Poll::Ready(Err(DataError::Closed)),
            }
        });
        let length = tokio::select! { biased; result=client.channel.wait_closed_checked()=>return Err(result.err().unwrap_or(DataError::Closed)), result=read=>result? };
        buffer.truncate(length);
        let word = |offset| u32::from_le_bytes(buffer[offset..offset + 4].try_into().unwrap());
        let current = word(0);
        let kind = word(4);
        let budget = word(8);
        if sequence.checked_add(1) != Some(current) || budget == 0 || budget > 1000 {
            return Err(DataError::Context);
        }
        sequence = current;
        let absolute = f64::from_le_bytes(buffer[12..20].try_into().unwrap());
        let end = cc_desk_remote_security::owner_clock::Instant::from_owner_deadline(absolute)
            .ok_or(DataError::Deadline)?;
        if end.saturating_duration_since(cc_desk_remote_security::owner_clock::Instant::now())
            > Duration::from_millis(budget as u64)
        {
            return Err(DataError::Bounds);
        }
        let eligible = || {
            remaining(end)?;
            client.channel.authority().check()
        };
        client.channel.authority().check()?;
        if kind == 3 {
            if length != 20 {
                return Err(DataError::Codec);
            }
            return Ok(());
        }
        let result = cc_desk_remote_security::owner_clock::timeout_at(end, async {
            match kind {
                1 => client
                    .snapshot(&buffer[20..], Duration::from_millis(budget as u64))
                    .await
                    .map(|batch| batch.parts),
                2 => client
                    .exchange(&buffer[20..], Duration::from_millis(budget as u64))
                    .await
                    .map(|part| vec![part]),
                4 => {
                    if length != 20 {
                        return Err(DataError::Codec);
                    }
                    let fact = client
                        .channel
                        .live_keepalive_fact(cc_desk_remote_security::Role::Client)?
                        .ok_or(DataError::Context)?;
                    let remaining = fact
                        .remaining_ms_for(&client.channel, cc_desk_remote_security::Role::Client)?;
                    if remaining > 900000 {
                        return Err(DataError::Bounds);
                    }
                    Ok(vec![remaining.to_le_bytes().to_vec()])
                }
                _ => Err(DataError::Codec),
            }
        })
        .await
        .map_err(|_| DataError::Deadline)?;
        match result {
            Ok(parts) => {
                client.channel.authority().check()?;
                for part in parts {
                    eligible()?;
                    unsafe { reply(sequence, 0, part.as_ptr(), part.len() as u32) };
                }
                client.channel.authority().check()?;
                eligible()?;
                unsafe { reply(sequence, 1, std::ptr::null(), 0) };
            }
            Err(error) => {
                unsafe { reply(sequence, -1, std::ptr::null(), 0) };
                return Err(error);
            }
        }
    }
}
