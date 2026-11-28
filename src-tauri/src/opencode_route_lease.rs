//! Native freshness state for one Desk-owned OpenCode terminal selection.
//! Callers must separately validate the exact run and authenticated binding.
//! This module does not grant terminal access.

use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Challenge(u64);

impl Challenge {
    pub(crate) fn wire(self) -> String {
        self.0.to_string()
    }

    pub(crate) fn parse(text: &str) -> Option<Self> {
        let value = text.parse::<u64>().ok()?;
        (value != 0 && value.to_string() == text).then_some(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Epoch(u64);

#[derive(Debug, Default)]
pub(crate) struct SelectionLease {
    next: u64,
    epoch: u64,
    pending: Option<(Challenge, Instant)>,
    valid: Option<(Epoch, Instant)>,
}

impl SelectionLease {
    pub(crate) fn issue(&mut self, now: Instant) -> Option<Challenge> {
        self.next = self.next.checked_add(1)?;
        let challenge = Challenge(self.next);
        self.pending = Some((challenge, now));
        Some(challenge)
    }

    pub(crate) fn complete(
        &mut self,
        answer: Challenge,
        exact: bool,
        now: Instant,
    ) -> Option<Epoch> {
        let (expected, issued) = self.pending?;
        if answer != expected {
            return None;
        }
        self.pending = None;
        if !exact || now < issued || now.duration_since(issued) >= WINDOW {
            self.revoke();
            return None;
        }
        let epoch = match self.valid.filter(|(_, expiry)| now < *expiry) {
            Some((epoch, _)) => epoch,
            None => {
                self.epoch = self.epoch.checked_add(1)?;
                Epoch(self.epoch)
            }
        };
        // The deadline starts at challenge issuance, not response arrival.
        self.valid = issued.checked_add(WINDOW).map(|expiry| (epoch, expiry));
        self.valid.map(|(epoch, _)| epoch)
    }

    pub(crate) fn epoch(&self, now: Instant) -> Option<Epoch> {
        self.valid
            .map(|(epoch, _)| epoch)
            .filter(|&epoch| self.current(epoch, now))
    }

    pub(crate) fn current(&self, epoch: Epoch, now: Instant) -> bool {
        self.valid
            .is_some_and(|(current, expiry)| current == epoch && now < expiry)
    }

    pub(crate) fn revoke(&mut self) {
        self.valid = None;
        self.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn superseded_or_replayed_response_does_not_extend_access() {
        let now = Instant::now();
        let mut lease = SelectionLease::default();
        let old = lease.issue(now).unwrap();
        let challenge = lease.issue(now).unwrap();
        assert_eq!(lease.complete(old, true, now), None);
        let epoch = lease.complete(challenge, true, now).unwrap();
        assert_eq!(lease.epoch(now), Some(epoch));
        assert_eq!(lease.complete(challenge, true, now), None);
        assert!(!lease.current(epoch, now + WINDOW));
    }

    #[test]
    fn losing_and_reselecting_route_cannot_reactivate_original_lease() {
        let now = Instant::now();
        let mut lease = SelectionLease::default();
        let challenge = lease.issue(now).unwrap();
        let old = lease.complete(challenge, true, now).unwrap();
        let mismatch = lease.issue(now).unwrap();
        assert_eq!(lease.complete(mismatch, false, now), None);
        let back = lease.issue(now).unwrap();
        let next = lease.complete(back, true, now).unwrap();
        assert_ne!(old, next);
        assert!(!lease.current(old, now));
    }

    #[test]
    fn delayed_reply_does_not_extend_deadline_and_revoke_discards_pending() {
        let now = Instant::now();
        let mut lease = SelectionLease::default();
        let challenge = lease.issue(now).unwrap();
        let epoch = lease
            .complete(challenge, true, now + Duration::from_millis(950))
            .unwrap();
        assert!(!lease.current(epoch, now + WINDOW));
        let stale = lease.issue(now).unwrap();
        assert_eq!(lease.complete(stale, true, now + WINDOW), None);
        let pending = lease.issue(now).unwrap();
        lease.revoke();
        assert_eq!(lease.complete(pending, true, now), None);
    }

    #[test]
    fn challenge_wire_form_is_canonical_and_nonzero() {
        assert_eq!(Challenge::parse("10").unwrap().wire(), "10");
        for invalid in ["0", "01", " 1", "+1", "18446744073709551616"] {
            assert_eq!(Challenge::parse(invalid), None);
        }
    }
}
