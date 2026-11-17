//! 仅真实 TLS 计量；不证明浏览器内存释放。
#![allow(non_snake_case)]
use super::*;
use std::{
    io,
    pin::Pin as IoPin,
    sync::Mutex,
    task::{Context, Poll},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream, ReadBuf};
use crate::test_identities as identities;
#[derive(Default)]
struct Meter {
    written: Vec<u8>,
    read: usize,
}
struct Wire {
    io: DuplexStream,
    meter: Arc<Mutex<Meter>>,
    short: usize,
}
impl AsyncRead for Wire {
    fn poll_read(
        mut self: IoPin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let n = out.filled().len();
        let r = IoPin::new(&mut self.io).poll_read(cx, out);
        self.meter.lock().unwrap().read += out.filled().len() - n;
        r
    }
}
impl AsyncWrite for Wire {
    fn poll_write(
        mut self: IoPin<&mut Self>,
        cx: &mut Context<'_>,
        b: &[u8],
    ) -> Poll<io::Result<usize>> {
        let n = b.len().min(self.short);
        match IoPin::new(&mut self.io).poll_write(cx, &b[..n]) {
            Poll::Ready(Ok(n)) => {
                let mut m = self.meter.lock().unwrap();
                assert!(m.written.len() + n <= 262144);
                m.written.extend_from_slice(&b[..n]);
                Poll::Ready(Ok(n))
            }
            r => r,
        }
    }
    fn poll_flush(mut self: IoPin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        IoPin::new(&mut self.io).poll_flush(cx)
    }
    fn poll_shutdown(mut self: IoPin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        IoPin::new(&mut self.io).poll_shutdown(cx)
    }
}
fn local(c: bool) -> LocalIdentity {
    let (k, p) = identities::identity(c, false);
    LocalIdentity::new(
        if c { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
        k.key.clone(),
    )
    .unwrap()
}
fn peer(c: bool) -> PeerPin {
    let (_, p) = identities::identity(c, false);
    PeerPin::new(
        if c { Role::Client } else { Role::Host },
        P256Spki::new(&p).unwrap(),
    )
}
fn records(b: &[u8]) -> Vec<usize> {
    let mut p = 0;
    let mut v = Vec::new();
    while p < b.len() {
        assert!(b.len() - p >= 5);
        let n = u16::from_be_bytes([b[p + 3], b[p + 4]]) as usize + 5;
        assert!(p + n <= b.len());
        v.push(n);
        p += n;
    }
    v
}
async fn probe(short: usize, key_update: bool) {
    let (c, s) = tokio::io::duplex(65536);
    let cm = Arc::new(Mutex::new(Meter::default()));
    let sm = Arc::new(Mutex::new(Meter::default()));
    let (_cancel, rx) = crate::cancellation();
    let (c, s) = tokio::join!(
        connect(
            Unbound::new(Wire {
                io: c,
                meter: cm.clone(),
                short
            }),
            local(true),
            peer(false),
            Duration::from_secs(2),
            rx.clone()
        ),
        accept(
            Unbound::new(Wire {
                io: s,
                meter: sm.clone(),
                short
            }),
            local(false),
            peer(true),
            Duration::from_secs(2),
            rx
        )
    );
    let mut c = c.unwrap();
    let mut s = s.unwrap();
    let cipher = s.evidence().cipher;
    // 保留握手基线，不将未消费残余从账中抹去。
    let baseline = sm.lock().unwrap().written.len();
    let read_base = cm.lock().unwrap().read;
    let client_base = cm.lock().unwrap().written.len();
    let residual = baseline - read_base;
    assert_eq!(residual, 0);
    assert!(!s._io.get_ref().1.wants_write());
    if key_update {
        match &mut s._io {
            tokio_rustls::TlsStream::Server(io) => io.get_mut().1.refresh_traffic_keys().unwrap(),
            _ => unreachable!(),
        }
    }
    s._io.write_all(&vec![0x5a; 32776]).await.unwrap();
    let after_write = sm.lock().unwrap().written.len() - baseline;
    assert_eq!(cm.lock().unwrap().read, read_base, "写成功不是对端消费");
    s._io.flush().await.unwrap();
    let after_flush = sm.lock().unwrap().written.len() - baseline;
    let wire = records(&sm.lock().unwrap().written[baseline..]);
    assert!(wire.iter().all(|n| *n <= 18437));
    let mut first = [0; 1];
    c._io.read_exact(&mut first).await.unwrap();
    assert_eq!(first, [0x5a]);
    let after_one = cm.lock().unwrap().read - read_base;
    let mut retained = [0; 32776];
    let buffered = match &mut c._io {
        tokio_rustls::TlsStream::Client(io) => {
            std::io::Read::read(&mut io.get_mut().1.reader(), &mut retained).unwrap()
        }
        _ => unreachable!(),
    };
    assert!(buffered > 0);
    assert!(retained[..buffered].iter().all(|b| *b == 0x5a));
    let mut rest = vec![0; 32775 - buffered];
    c._io.read_exact(&mut rest).await.unwrap();
    assert!(rest.iter().all(|b| *b == 0x5a));
    let consumed = cm.lock().unwrap().read - read_base;
    assert_eq!(consumed, after_flush);
    let empty = match &mut c._io {
        tokio_rustls::TlsStream::Client(io) => {
            std::io::Read::read(&mut io.get_mut().1.reader(), &mut first)
                .unwrap_err()
                .kind()
        }
        _ => unreachable!(),
    };
    assert_eq!(empty, io::ErrorKind::WouldBlock);
    c._io.flush().await.unwrap();
    let control = cm.lock().unwrap().written.len() - client_base;
    // KeyUpdate 回应可能仍在 rustls 中，flush 空输出不能当控制状态清空。
    c._io.write_all(&[0x33]).await.unwrap();
    c._io.flush().await.unwrap();
    let reverse = records(&cm.lock().unwrap().written[client_base..]);
    let mut request = [0];
    s._io.read_exact(&mut request).await.unwrap();
    assert_eq!(request, [0x33]);
    assert_eq!(reverse, if key_update { vec![27, 23] } else { vec![23] });
    assert_eq!(
        wire,
        if key_update {
            vec![27, 16406, 16406, 30]
        } else {
            vec![16406, 16406, 30]
        }
    );
    eprintln!(
        "TEST_ONLY_TLS_BATCH cipher={cipher:?} short={short} key_update={key_update} baseline={baseline} residual={residual} after_write={after_write} after_flush={after_flush} records={wire:?} after_one={after_one} retained_plaintext={buffered} consumed={consumed} reverse_control={control} next_request_records={reverse:?}"
    );
}
#[tokio::test]
async fn TlsBatchProbe_RealWireAndRetainedPlaintext_001() {
    tokio::time::timeout(Duration::from_secs(5), probe(16384, false))
        .await
        .unwrap();
}
#[tokio::test]
async fn TlsBatchProbe_KeyUpdateAndShortWrites_002() {
    tokio::time::timeout(Duration::from_secs(5), probe(7, true))
        .await
        .unwrap();
}
