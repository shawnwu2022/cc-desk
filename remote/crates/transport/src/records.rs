//! Bounded raw records for an already authenticated endpoint TLS stream.
//! No TLS, JSON validation, peer binding or endpoint authorization is supplied here.
//! An adapter failure drops its supplied IO and retires that adapter. For split or
//! shared IO this does not close sibling handles: the enclosing TLS/channel owner
//! must invalidate and cancel both directions before it can dispatch further data.
pub use cc_desk_remote_record::Kind;
#[derive(Debug, PartialEq, Eq)]
pub enum RecordError {
    Limit,
    Closed,
    Deadline,
    Io,
    Wire(cc_desk_remote_record::Error),
}
pub struct RawRecord {
    pub kind: Kind,
    pub minor: u16,
    pub payload: Vec<u8>,
}
impl std::fmt::Debug for RawRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawRecord")
            .field("kind", &self.kind)
            .field("minor", &self.minor)
            .field("payload_len", &self.payload.len())
            .finish()
    }
}
pub struct RecordReader<R> {
    io: Option<R>,
    minor: u16,
}
impl<R> RecordReader<R> {
    pub fn new(io: R, minor: u16) -> Result<Self, RecordError> {
        if minor > 1 {
            return Err(RecordError::Limit);
        }
        Ok(Self {
            io: Some(io),
            minor,
        })
    }
    pub fn is_closed(&self) -> bool {
        self.io.is_none()
    }
}
impl<R: tokio::io::AsyncRead + Unpin> RecordReader<R> {
    /// One deadline covers the entire header and body. Once valid IO work begins,
    /// cancellation drops this adapter's IO; its partial frame cannot be resumed.
    pub async fn read(
        &mut self,
        budget: std::time::Duration,
    ) -> Result<Option<RawRecord>, RecordError> {
        use tokio::io::AsyncReadExt;
        let end = deadline(budget)?;
        let mut io = self.io.take().ok_or(RecordError::Closed)?;
        let result = crate::deadline::bounded(end, async {
            let mut header = [0; 8];
            if io
                .read(&mut header[..1])
                .await
                .map_err(|_| RecordError::Io)?
                == 0
            {
                return Ok(None);
            }
            io.read_exact(&mut header[1..])
                .await
                .map_err(|_| RecordError::Io)?;
            let h = cc_desk_remote_record::Header::decode(header, self.minor)
                .map_err(RecordError::Wire)?;
            // Length, type-specific cap and selected minor are checked before allocation.
            let mut payload = vec![0; h.payload_len];
            io.read_exact(&mut payload)
                .await
                .map_err(|_| RecordError::Io)?;
            Ok(Some(RawRecord {
                kind: h.kind,
                minor: h.minor,
                payload,
            }))
        })
        .await
        .map_err(|_| RecordError::Deadline)?;
        if matches!(result, Ok(Some(_))) {
            self.io = Some(io);
        }
        result
    }
}
pub struct RecordWriter<W> {
    io: Option<W>,
    minor: u16,
}
impl<W> RecordWriter<W> {
    pub fn selected_minor(&self) -> u16 {
        self.minor
    }
    pub fn new(io: W, minor: u16) -> Result<Self, RecordError> {
        if minor > 1 {
            return Err(RecordError::Limit);
        }
        Ok(Self {
            io: Some(io),
            minor,
        })
    }
    pub fn is_closed(&self) -> bool {
        self.io.is_none()
    }
}
impl<W: tokio::io::AsyncWrite + Unpin> RecordWriter<W> {
    /// An IO failure, deadline or future cancellation makes the write uncertain and
    /// permanently retires this adapter. Success is byte transport, not a Host receipt.
    pub async fn write(
        &mut self,
        kind: Kind,
        payload: &[u8],
        budget: std::time::Duration,
    ) -> Result<(), RecordError> {
        use tokio::io::AsyncWriteExt;
        let end = deadline(budget)?;
        if self.io.is_none() {
            return Err(RecordError::Closed);
        }
        let minor = if kind == Kind::AppBind { 0 } else { self.minor };
        let bytes =
            cc_desk_remote_record::encode(kind, minor, payload).map_err(RecordError::Wire)?;
        let mut io = self.io.take().ok_or(RecordError::Closed)?;
        crate::deadline::bounded(end, async {
            io.write_all(&bytes).await.map_err(|_| RecordError::Io)?;
            io.flush().await.map_err(|_| RecordError::Io)
        })
        .await
        .map_err(|_| RecordError::Deadline)??;
        self.io = Some(io);
        Ok(())
    }
}
fn deadline(budget: std::time::Duration) -> Result<tokio::time::Instant, RecordError> {
    if budget.is_zero() || budget > crate::MAX_IO_BUDGET {
        return Err(RecordError::Limit);
    }
    tokio::time::Instant::now()
        .checked_add(budget)
        .ok_or(RecordError::Limit)
}
