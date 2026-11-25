//! Android's one explicitly authorized Controller. All terminal operations
//! use the installed NativeClient; there is no raw JNI-input/wire escape.
use cc_desk_remote_client::{DataError, NativeClient};
use cc_desk_remote_record::{control_io as wire, input, projection};
use ring::rand::{SecureRandom, SystemRandom};
use std::{sync::Mutex, time::Duration};
use tokio::sync::{Notify, watch};
use zeroize::Zeroizing;

const IO: Duration = Duration::from_secs(1);
const MAX_TEXT: usize = 2048;
struct Frame {
    id: i64,
    text: String,
    taken: bool,
    confirmed: bool,
}
struct Input {
    id: i64,
    bytes: Zeroizing<Vec<u8>>,
}
struct State {
    next: i64,
    frame: Option<Frame>,
    input: Option<Input>,
    result: (i64, i32),
    synced: bool,
    active: bool,
}
pub(crate) struct Control {
    state: Mutex<State>,
    change: Notify,
}
impl Control {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                next: 0,
                frame: None,
                input: None,
                result: (0, 0),
                synced: false,
                active: true,
            }),
            change: Notify::new(),
        }
    }
    pub(crate) fn take_frame(&self) -> Option<String> {
        let mut s = self.state.lock().ok()?;
        if !s.active {
            return None;
        }
        let f = s.frame.as_mut()?;
        if f.taken {
            return None;
        }
        f.taken = true;
        serde_json::to_string(&serde_json::json!({"id":f.id,"text":f.text})).ok()
    }
    pub(crate) fn confirm(&self, id: i64) -> bool {
        let Ok(mut s) = self.state.lock() else {
            return false;
        };
        if !s.active {
            return false;
        }
        let Some(f) = s.frame.as_mut() else {
            return false;
        };
        if f.id != id || !f.taken || f.confirmed {
            return false;
        }
        f.confirmed = true;
        self.change.notify_one();
        true
    }
    pub(crate) fn enqueue(&self, bytes: Vec<u8>) -> Option<i64> {
        let mut s = self.state.lock().ok()?;
        if !s.active || !s.synced || s.result.1 == 1 {
            return None;
        }
        let id = s.next.checked_add(1)?;
        s.next = id;
        s.result = (id, 1);
        s.input = Some(Input {
            id,
            bytes: Zeroizing::new(bytes),
        });
        self.change.notify_one();
        Some(id)
    }
    pub(crate) fn outcome(&self, id: i64) -> i32 {
        self.state
            .lock()
            .ok()
            .filter(|s| s.result.0 == id)
            .map(|s| s.result.1)
            .unwrap_or(0)
    }
    pub(crate) fn finish(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.active = false;
            s.synced = false;
            s.input = None;
            s.frame = None;
            if s.result.1 == 1 {
                s.result.1 = 4;
            }
        }
        self.change.notify_one();
    }
    fn resolve(&self, id: i64, result: i32) {
        if let Ok(mut s) = self.state.lock() {
            if s.result == (id, 1) {
                s.result = (id, result)
            }
        }
    }
}

pub(crate) fn text(text: String) -> Option<Vec<u8>> {
    if text.is_empty()
        || text.len() > MAX_TEXT
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return None;
    }
    Some(text.into_bytes())
}
pub(crate) fn key(code: i32) -> Option<Vec<u8>> {
    let bytes: &[u8] = match code {
        1 => b"\r",
        2 => b"\x7f",
        3 => b"\t",
        4 => b"\x1b",
        5 => b"\x1b[A",
        6 => b"\x1b[B",
        7 => b"\x1b[D",
        8 => b"\x1b[C",
        9 => b"\x03",
        _ => return None,
    };
    Some(bytes.to_vec())
}

fn counter(sequence: &mut u64) -> Result<u64, DataError> {
    *sequence = sequence.checked_add(1).ok_or(DataError::Bounds)?;
    Ok(*sequence)
}
fn request(kind: &str, number: u64) -> Result<Vec<u8>, DataError> {
    serde_json::to_vec(
        &serde_json::json!({"message":kind,"profile":1,"request":number.to_string()}),
    )
    .map_err(|_| DataError::Codec)
}
async fn exchange(client: &NativeClient, bytes: &[u8]) -> Result<wire::Message, DataError> {
    wire::decode(&client.exchange(bytes, IO).await?).map_err(|_| DataError::Codec)
}
async fn command(
    client: &NativeClient,
    message: wire::Message,
) -> Result<wire::Message, DataError> {
    exchange(
        client,
        &wire::encode(&message).map_err(|_| DataError::Codec)?,
    )
    .await
}
fn decimal(text: &str) -> Result<u64, DataError> {
    let n = text.parse::<u64>().map_err(|_| DataError::Codec)?;
    if n == 0 || n.to_string() != text {
        return Err(DataError::Codec);
    }
    Ok(n)
}

async fn send(
    client: &NativeClient,
    c: &Control,
    seq: &mut u64,
    input: &Input,
) -> Result<(), DataError> {
    let wire::Message::InputCursorState {
        next_sequence: Some(next),
        pending: None,
        ..
    } = command(
        client,
        wire::Message::InputCursor {
            profile: 1,
            request: counter(seq)?.to_string(),
        },
    )
    .await?
    else {
        return Err(DataError::Context);
    };
    let wire::Message::WindowGranted {
        window,
        not_after_host_ms,
        ..
    } = exchange(client, &request("Window", counter(seq)?)?).await?
    else {
        return Err(DataError::Context);
    };
    let mut op = [0; 16];
    SystemRandom::new()
        .fill(&mut op)
        .map_err(|_| DataError::Context)?;
    let body = Zeroizing::new(
        input::encode(&input::Input {
            operation: op,
            sequence: decimal(&next)?,
            window: crate::enrollment::lower_hex(&window).map_err(|_| DataError::Codec)?,
            not_after_host_ms: decimal(&not_after_host_ms)?,
            bytes: input.bytes.to_vec(),
        })
        .map_err(|_| DataError::Codec)?,
    );
    match exchange(client, &body).await? {
        wire::Message::InputNotAccepted { .. } => {
            c.resolve(input.id, 3);
            Ok(())
        }
        wire::Message::InputReceipt {
            operation,
            sequence,
            outcome,
            proof: Some(proof),
            ..
        } if matches!(
            outcome,
            wire::Outcome::HostWritten | wire::Outcome::Rejected
        ) =>
        {
            c.resolve(
                input.id,
                if outcome == wire::Outcome::HostWritten {
                    2
                } else {
                    3
                },
            );
            let reply = command(
                client,
                wire::Message::ReceiptAck {
                    profile: 1,
                    request: counter(seq)?.to_string(),
                    operation,
                    sequence,
                    outcome,
                    proof,
                },
            )
            .await?;
            if !matches!(
                reply,
                wire::Message::ReceiptSettled {
                    disposition: wire::Disposition::Advanced | wire::Disposition::AlreadySettled,
                    ..
                }
            ) {
                return Err(DataError::Context);
            }
            Ok(())
        }
        _ => Err(DataError::Uncertain),
    }
}

struct Lease {
    id: String,
    fence: String,
    renewal: u64,
    deadline: u64,
    due: tokio::time::Instant,
}

impl Lease {
    async fn open(client: &NativeClient, seq: &mut u64) -> Result<Self, DataError> {
        let wire::Message::LeaseStateReply {
            lease,
            fence,
            not_after_host_ms,
            next_renewal_sequence,
            ..
        } = command(
            client,
            wire::Message::LeaseState {
                profile: 1,
                request: counter(seq)?.to_string(),
            },
        )
        .await?
        else {
            return Err(DataError::Context);
        };
        Ok(Self {
            id: lease,
            fence,
            renewal: decimal(&next_renewal_sequence)?,
            deadline: decimal(&not_after_host_ms)?,
            due: tokio::time::Instant::now() + Duration::from_secs(3),
        })
    }
    async fn refresh(&mut self, client: &NativeClient, seq: &mut u64) -> Result<(), DataError> {
        let wire::Message::LeaseRenewed {
            not_after_host_ms, ..
        } = command(
            client,
            wire::Message::LeaseRenew {
                profile: 1,
                request: counter(seq)?.to_string(),
                lease: self.id.clone(),
                fence: self.fence.clone(),
                renewal_sequence: self.renewal.to_string(),
            },
        )
        .await?
        else {
            return Err(DataError::Context);
        };
        let next = decimal(&not_after_host_ms)?;
        if next < self.deadline {
            return Err(DataError::Context);
        }
        self.deadline = next;
        self.renewal = self.renewal.checked_add(1).ok_or(DataError::Bounds)?;
        self.due = tokio::time::Instant::now() + Duration::from_secs(3);
        Ok(())
    }
}

pub(crate) async fn serve(
    client: &NativeClient,
    c: &Control,
    mut stop: watch::Receiver<bool>,
) -> Result<(), DataError> {
    let mut seq = 0;
    let mut lease =
        tokio::select! {biased;_=stop.changed()=>return Ok(()),r=Lease::open(client,&mut seq)=>r?};
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        {
            let mut s = c.state.lock().map_err(|_| DataError::Context)?;
            s.synced = false;
            if let Some(late) = s.input.take() {
                if s.result == (late.id, 1) {
                    s.result = (late.id, 3);
                }
            }
        }
        if tokio::time::Instant::now() >= lease.due {
            tokio::select! {biased;_=stop.changed()=>return Ok(()),r=lease.refresh(client,&mut seq)=>r?};
        }
        // This exact Host lane negotiates classic full-frame snapshots. There is
        // no fallback or second socket when a frame/ACK is rejected.
        let snapshot_request = request("Snapshot", counter(&mut seq)?)?;
        let batch = tokio::select! {biased;_=stop.changed()=>return Ok(()),work=client.snapshot(&snapshot_request,IO)=>work?};
        let text = crate::observer_text(&batch.parts).ok_or(DataError::Codec)?;
        let id = {
            let mut s = c.state.lock().map_err(|_| DataError::Context)?;
            s.next = s.next.checked_add(1).ok_or(DataError::Bounds)?;
            let id = s.next;
            s.frame = Some(Frame {
                id,
                text,
                taken: false,
                confirmed: false,
            });
            id
        };
        let confirm =
            async {
                loop {
                    if c.state.lock().ok().is_some_and(|s| {
                        s.frame.as_ref().is_some_and(|f| f.id == id && f.confirmed)
                    }) {
                        break;
                    }
                    c.change.notified().await;
                }
            };
        tokio::select! {biased;_=stop.changed()=>return Ok(()),r=tokio::time::timeout(Duration::from_secs(2),confirm)=>{r.map_err(|_|DataError::Deadline)?;}}
        let ack = projection::encode_ack(
            &projection::Ack {
                profile: 1,
                stamp: batch.stamp,
            },
            1,
        )
        .map_err(|_| DataError::Codec)?;
        tokio::select! {biased;_=stop.changed()=>return Ok(()),r=exchange(client,&ack)=> {if !matches!(r?,wire::Message::ProjectionAcknowledged{..}){return Err(DataError::Context);}}}
        {
            let mut s = c.state.lock().map_err(|_| DataError::Context)?;
            s.synced = true;
        }
        let next = async {
            loop {
                if let Some(input) = c.state.lock().ok().and_then(|mut s| s.input.take()) {
                    break input;
                }
                c.change.notified().await;
            }
        };
        let value = tokio::select! {biased;_=stop.changed()=>return Ok(()),r=tokio::time::timeout(Duration::from_millis(750),next)=>r.ok()};
        if let Some(input) = value {
            let result = tokio::select! {biased;_=stop.changed()=>return Ok(()),r=send(client,c,&mut seq,&input)=>r};
            match result {
                Ok(()) => {}
                Err(e) => {
                    c.resolve(input.id, 4);
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_frame_confirmation_is_single_use_and_retirement_drops_queued_bytes() {
        let c = Control::new();
        assert!(c.enqueue(vec![1]).is_none());
        c.state.lock().unwrap().frame = Some(Frame {
            id: 7,
            text: "snapshot".into(),
            taken: false,
            confirmed: false,
        });
        assert!(!c.confirm(7));
        assert_eq!(
            c.take_frame().as_deref(),
            Some(r#"{"id":7,"text":"snapshot"}"#)
        );
        assert!(c.take_frame().is_none());
        assert!(!c.confirm(8));
        assert!(c.confirm(7));
        assert!(!c.confirm(7));
        // Authenticated ProjectionAcknowledged is the production transition
        // enabling this state, not JNI's confirmation by itself.
        assert!(c.enqueue(vec![1]).is_none());
        c.state.lock().unwrap().synced = true;
        let id = c.enqueue(vec![1]).unwrap();
        assert!(c.enqueue(vec![2]).is_none());
        c.finish();
        assert!(c.state.lock().unwrap().input.is_none());
        assert!(c.enqueue(vec![3]).is_none());
        assert!(!c.confirm(7));
        assert_eq!(c.outcome(id), 4);
        assert_eq!(c.outcome(id + 1), 0);
        assert!(text("x\n".into()).is_none());
        assert!(key(10).is_none());
    }
}
