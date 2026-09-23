//! Bounded, backend-private request change detection. This is not authentication.
#![allow(dead_code)] // Consumed by the staged D11 coordinator, not live IPC yet.

use super::profiles::error;
use super::types::{LaunchRequest, SafeError};
use std::collections::hash_map::{DefaultHasher, RandomState};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};

const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_ROUTING_BYTES: usize = 128;

pub(super) type Fingerprint = [u64; 2];

pub(super) fn validate_routing_id(field: &str, value: &str) -> Result<(), SafeError> {
    if value.is_empty() || value.len() > MAX_ROUTING_BYTES || value.chars().any(char::is_control) {
        return Err(SafeError::invalid(field));
    }
    Ok(())
}

pub(super) struct RequestFingerprinter {
    keys: [RandomState; 2],
}

impl RequestFingerprinter {
    pub(super) fn new() -> Self {
        Self {
            keys: [RandomState::new(), RandomState::new()],
        }
    }

    pub(super) fn fingerprint(&self, request: &LaunchRequest) -> Result<Fingerprint, SafeError> {
        for (field, value) in [
            ("requestId", request.request_id.as_str()),
            ("runId", request.run_id.as_str()),
            ("tabId", request.tab_id.as_str()),
        ] {
            validate_routing_id(field, value)?;
        }
        let mut sink = FingerprintSink {
            hashers: self.keys.each_ref().map(RandomState::build_hasher),
            written: 0,
            exceeded: false,
        };
        // Stream the exact canonical serialization. Do not retain a second full
        // prompt buffer or expose the digest/key as a wire identity or secret.
        if serde_json::to_writer(&mut sink, request).is_err() {
            return Err(if sink.exceeded {
                error("REQUEST_TOO_LARGE")
            } else {
                SafeError::invalid("request")
            });
        }
        Ok(sink.hashers.each_ref().map(Hasher::finish))
    }
}

struct FingerprintSink {
    hashers: [DefaultHasher; 2],
    written: usize,
    exceeded: bool,
}

impl Write for FingerprintSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let total = self.written.checked_add(bytes.len());
        let Some(total) = total.filter(|total| *total <= MAX_REQUEST_BYTES) else {
            self.exceeded = true;
            return Err(io::Error::other("request budget exceeded"));
        };
        for hasher in &mut self.hashers {
            hasher.write(bytes);
        }
        self.written = total;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
