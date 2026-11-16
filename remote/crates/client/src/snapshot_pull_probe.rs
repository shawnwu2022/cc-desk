//! 仅测试的未协商协议实验：不导出、不接生产 dispatcher，不伪造 capability fact。
use super::*;
use serde_json::{Value, json};
const TRANSFER: &str = "77777777777777777777777777777777";
const BEGIN: &[u8] = br#"{"message":"SnapshotBegin","profile":1,"request":"1","mode":"pull-v1"}"#;
fn batch(part: &Value, index: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({"message":"SnapshotBatch","request":"1","transfer":TRANSFER,"index":index,"part":part})).unwrap()
}
fn decode(bytes: &[u8], index: usize) -> Result<Vec<u8>, DataError> {
    if bytes.len() > 32768 - cc_desk_remote_record::grant::DATA_PREFIX {
        return Err(DataError::Bounds);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| DataError::Codec)?;
    let object = value.as_object().ok_or(DataError::Codec)?;
    if object.len() != 5
        || value["message"] != "SnapshotBatch"
        || value["request"] != "1"
        || value["transfer"] != TRANSFER
        || value["index"].as_u64() != Some(index as u64)
    {
        return Err(DataError::Context);
    }
    serde_json::to_vec(&value["part"]).map_err(|_| DataError::Codec)
}
// 消费点用真实 DataRecord 的词法作用域，不把 postMessage 或本地 flush 当收据。
async fn pull(native: &NativeClient, budget: Duration) -> Result<Vec<Vec<u8>>, DataError> {
    let end = deadline(budget)?;
    let _pending = native.pending.try_lock().map_err(|_| DataError::Full)?;
    let mut close = CancelOnDrop(Some(native.channel.authority()));
    native
        .channel
        .send_control(&native.scope, native.fence, BEGIN, remaining(end)?)
        .await?;
    let mut parts = Vec::new();
    let mut next = 0usize;
    let mut total = 0usize;
    let mut current = None;
    loop {
        let complete = {
            let record = native
                .channel
                .receive(&native.scope, native.fence, remaining(end)?)
                .await?;
            native.record(&record)?;
            let bytes = decode(record.payload(), parts.len())?;
            total = total.checked_add(bytes.len()).ok_or(DataError::Bounds)?;
            if total > MAX_SNAPSHOT_BYTES {
                return Err(DataError::Bounds);
            }
            let message = projection::decode(&bytes, 1).map_err(|_| DataError::Codec)?;
            let projection::Content::SnapshotPart(part) = message.content else {
                return Err(DataError::Codec);
            };
            let a = native.channel.authority();
            let t = &part.stamp.target;
            let binding: String = a.binding_id().iter().map(|b| format!("{b:02x}")).collect();
            if t.host != a.context().host_endpoint_id
                || t.session != native.scope.session
                || t.run != native.scope.run
                || t.generation != native.scope.generation
                || t.incarnation != a.context().incarnation
                || part.stamp.binding != binding
            {
                return Err(DataError::Context);
            }
            if usize::from(part.start) != next || part.cells.is_empty() {
                return Err(DataError::Context);
            }
            if let Some((stamp, state)) = &current {
                if stamp != &part.stamp || state != &part.state {
                    return Err(DataError::Context);
                }
            } else {
                current = Some((part.stamp.clone(), part.state.clone()));
            }
            next = next
                .checked_add(part.cells.len())
                .ok_or(DataError::Bounds)?;
            let cells = usize::from(part.state.rows) * usize::from(part.state.columns);
            if next > cells {
                return Err(DataError::Bounds);
            }
            parts.push(bytes);
            next == cells
        }; // record/其真实许可及解析临时在 Next 前 drop；累计 parts 仍持有内存。
        remaining(end)?;
        if complete {
            native.channel.authority().check()?;
            close.0 = None;
            return Ok(parts);
        }
        let next=serde_json::to_vec(&json!({"message":"SnapshotNext","profile":1,"request":"1","transfer":TRANSFER,"index":parts.len(),"offset":next})).unwrap();
        native
            .channel
            .send_control(&native.scope, native.fence, &next, remaining(end)?)
            .await?;
    }
}
#[tokio::test]
async fn snapshot_pull_probe_real_channel_pending_and_exact_batches() {
    for wrong in [false, true] {
        let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
        let native = NativeClient::new(c, 4).unwrap();
        let value: Value = serde_json::from_slice(&projection(&native)).unwrap();
        let mut one = value.clone();
        one["body"]["cells"] = json!([value["body"]["cells"][0]]);
        let mut two = value.clone();
        two["body"]["cells"] = json!([value["body"]["cells"][1]]);
        two["body"]["start"] = json!(if wrong { 0 } else { 1 });
        let server = async {
            let begin = h.receive(&native.scope, 4, BUDGET).await.unwrap();
            assert_eq!(begin.payload(), BEGIN);
            drop(begin);
            assert_eq!(
                native
                    .exchange(br#"{"message":"Window","profile":1,"request":"2"}"#, BUDGET)
                    .await
                    .err(),
                Some(DataError::Full)
            );
            h.send_projection(&native.scope, 4, &batch(&one, 0), BUDGET)
                .await
                .unwrap();
            let next = h.receive(&native.scope, 4, BUDGET).await.unwrap();
            let v: Value = serde_json::from_slice(next.payload()).unwrap();
            assert_eq!(
                v,
                json!({"message":"SnapshotNext","profile":1,"request":"1","transfer":TRANSFER,"index":1,"offset":1})
            );
            drop(next);
            h.send_projection(&native.scope, 4, &batch(&two, 1), BUDGET)
                .await
                .unwrap();
        };
        let (result, ()) = tokio::join!(pull(&native, BUDGET), server);
        if wrong {
            assert_eq!(result.err(), Some(DataError::Context));
            assert!(native.channel.authority().check().is_err());
        } else {
            assert_eq!(result.unwrap().len(), 2);
            assert!(native.pending.try_lock().is_ok());
            let request2=br#"{"message":"Window","profile":1,"request":"2"}"#;
            let host=async {
                let record=h.receive(&native.scope,4,BUDGET).await.unwrap();assert_eq!(record.payload(),request2);drop(record);
                let reply=wire::encode(&wire::Message::WindowGranted{profile:1,request:"2".into(),window:"09".repeat(32),not_after_host_ms:"2000".into()}).unwrap();
                h.send_projection(&native.scope,4,&reply,BUDGET).await.unwrap();
            };
            let(reply,())=tokio::join!(native.exchange(request2,BUDGET),host);reply.unwrap();

        }
        native.retire().await.expect("original Client owner joined");
        h.close().await;
        h.wait_closed_checked().await.expect("original Host owner joined");
    }
}
#[tokio::test]
async fn snapshot_pull_probe_next_does_not_refresh_deadline() {
    let (c, h, _gate, _cancel) = channels(Arc::new(AtomicU64::new(1000))).await;
    let native = NativeClient::new(c, 4).unwrap();
    let mut one: Value = serde_json::from_slice(&projection(&native)).unwrap();
    one["body"]["cells"].as_array_mut().unwrap().truncate(1);
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let server = async {
        h.receive(&native.scope, 4, BUDGET).await.unwrap();
        tokio::time::advance(Duration::from_millis(30)).await;
        h.send_projection(&native.scope, 4, &batch(&one, 0), BUDGET)
            .await
            .unwrap();
        h.receive(&native.scope, 4, BUDGET).await.unwrap();
    };
    let (result, ()) = tokio::join!(pull(&native, Duration::from_millis(40)), server);
    assert!((Duration::from_millis(40)..=Duration::from_millis(42)).contains(&started.elapsed()), "fixed deadline elapsed {:?}", started.elapsed());
    assert_eq!(result.err(), Some(DataError::Deadline));
    assert!(native.channel.authority().check().is_err());
    native.retire().await.expect("original Client owner joined");
    h.close().await;
        h.wait_closed_checked().await.expect("original Host owner joined");
}
#[test]
fn snapshot_pull_probe_old_mode_refuses_and_codec_rejects_replay_or_extra_fields() {
    assert!(command::decode(BEGIN).is_err());
    let p = json!({});
    let bytes = batch(&p, 0);
    assert!(decode(&bytes, 1).is_err());
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v["extra"] = json!(true);
    assert!(decode(&serde_json::to_vec(&v).unwrap(), 0).is_err());
    assert!(decode(&vec![0; 32768], 0).is_err());
}

#[tokio::test]
async fn snapshot_pull_probe_cancel_polled_future_retires_original_owner() {
    let(c,h,_gate,_cancel)=channels(Arc::new(AtomicU64::new(1000))).await;
    let native=NativeClient::new(c,4).unwrap();
    let mut operation=Box::pin(pull(&native,BUDGET));
    tokio::select!{r=&mut operation=>panic!("unexpected completion {r:?}"),r=h.receive(&native.scope,4,BUDGET)=>{assert_eq!(r.unwrap().payload(),BEGIN);}}
    drop(operation);
    assert!(native.channel.authority().check().is_err());
    native.retire().await.expect("original Client owner joined");
    h.close().await;h.wait_closed_checked().await.expect("original Host owner joined");
}
