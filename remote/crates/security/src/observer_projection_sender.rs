use super::*;
use cc_desk_remote_record::projection::{self, Content, Message};

#[derive(Clone)]
pub(super) struct ObserverWriteCheck {
    end: Instant,
    source_current: Arc<dyn Fn() -> bool + Send + Sync>,
}

impl ObserverWriteCheck {
    pub(super) fn deadline(&self) -> Instant {
        self.end
    }
    pub(super) fn check(&self, now: Instant) -> Result<(), DataError> {
        if now >= self.end {
            return Err(DataError::Deadline);
        }
        if !(self.source_current)() {
            return Err(DataError::Context);
        }
        Ok(())
    }
}

/// Non-cloneable output-only view of the exact installed Observer channel.
/// Only trusted native code may supply the source predicate: it must verify
/// the original producer + run, not a caller-supplied scope or cached Boolean.
/// The native owner must close the original channel when the source retires;
/// a predicate alone cannot wake an already pending transport poll. The
/// predicate must be nonblocking and should hold a weak original owner.
/// The caller must also derive snapshot parts/barrier/epochs at the verified
/// capture boundary; matching scope and channel metadata cannot establish
/// the origin of rendered cells. This API never installs a peer or falls back
/// to legacy, and dropping it closes only this exact Observer channel.
pub struct ObserverProjectionSender<'a> {
    channel: &'a RunningChannel,
    authority: crate::BoundDataAuthority,
    fence: u64,
    check: ObserverWriteCheck,
}

impl RunningChannel {
    pub fn observer_projection_sender(
        &self,
        total_budget: Duration,
        source_current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<ObserverProjectionSender<'_>, DataError> {
        let authority = self.authority();
        authority.check()?;
        if authority.local_role() != Role::Host
            || authority.data_role() != DataRole::Observer
            || authority.control_lease_renew_enabled()
            || authority.data_keepalive_negotiated()
        {
            return Err(DataError::Role);
        }
        if total_budget.is_zero() || total_budget > Duration::from_secs(60) {
            return Err(DataError::Bounds);
        }
        // Never use check_data with a caller fence: it also accepts legacy.
        let fence = self.peer.fence()?;
        let now = Instant::now();
        let check = ObserverWriteCheck {
            end: now.checked_add(total_budget).ok_or(DataError::Bounds)?,
            source_current: Arc::new(source_current),
        };
        check.check(now)?;
        self.observer_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| DataError::Full)?;
        Ok(ObserverProjectionSender {
            channel: self,
            authority,
            fence,
            check,
        })
    }
}

impl ObserverProjectionSender<'_> {
    fn check(&self, now: Instant) -> Result<(), DataError> {
        self.authority.check()?;
        if self.channel.peer.fence()? != self.fence {
            return Err(DataError::Fence);
        }
        self.check.check(now)
    }

    pub async fn send_snapshot_part(&mut self, part: Message) -> Result<(), DataError> {
        struct CloseOnDrop<'a>(Option<&'a crate::BoundDataAuthority>);
        impl Drop for CloseOnDrop<'_> {
            fn drop(&mut self) {
                if let Some(authority) = self.0 {
                    authority.request_close();
                }
            }
        }
        let mut fail = CloseOnDrop(Some(&self.authority));
        let now = Instant::now();
        self.check(now)?;
        let Content::SnapshotPart(value) = &part.content else {
            return Err(DataError::Codec);
        };
        check_snapshot_target(&self.authority, &value.stamp)?;
        let scope = self.authority.scope();
        let frame = projection::encode(&part, 1).map_err(|_| DataError::Codec)?;
        self.channel
            .send(
                scope,
                self.fence,
                &frame,
                self.check
                    .end
                    .saturating_duration_since(now)
                    .min(Duration::from_secs(10)),
                Some(self.check.clone()),
            )
            .await?;
        self.check(Instant::now())?;
        fail.0 = None;
        Ok(())
    }

    pub async fn retire(self) -> Result<DataError, DataError> {
        self.authority.request_close();
        self.channel.wait_closed_checked().await
    }
}

fn check_snapshot_target(
    authority: &crate::BoundDataAuthority,
    stamp: &projection::Stamp,
) -> Result<(), DataError> {
    let scope = authority.scope();
    let context = authority.context();
    let binding: String = authority
        .binding_id()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if stamp.binding != binding
        || stamp.target.host != context.host_endpoint_id
        || stamp.target.incarnation != context.incarnation
        || stamp.target.session != scope.session
        || stamp.target.run != scope.run
        || stamp.target.generation != scope.generation
    {
        return Err(DataError::Context);
    }
    Ok(())
}
impl Drop for ObserverProjectionSender<'_> {
    fn drop(&mut self) {
        self.authority.request_close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observer_emission_requires_ready_not_legacy_or_pending() {
        let (legacy, _) = crate::peer_install::Shared::new(None);
        let (pending, _) = crate::peer_install::Shared::new(Some(
            tokio::time::Instant::now() + Duration::from_secs(1),
        ));
        assert_eq!(legacy.fence(), Err(DataError::Context));
        assert_eq!(pending.fence(), Err(DataError::Context));
    }

    #[test]
    fn observer_emission_expires_before_consulting_source_and_rechecks_after_revocation() {
        let now = Instant::now();
        let live = Arc::new(AtomicBool::new(true));
        let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let check = ObserverWriteCheck {
            end: now + Duration::from_secs(1),
            source_current: Arc::new({
                let live = live.clone();
                let seen = seen.clone();
                move || {
                    seen.fetch_add(1, Ordering::Relaxed);
                    live.load(Ordering::Acquire)
                }
            }),
        };
        assert_eq!(check.check(now), Ok(()));
        live.store(false, Ordering::Release);
        assert_eq!(check.check(now), Err(DataError::Context));
        assert_eq!(check.check(check.end), Err(DataError::Deadline));
        assert_eq!(seen.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn observer_emission_does_not_arm_a_legitimate_but_uninstalled_pair() {
        use crate::grant_tests::{admin, scope, signed};
        let (mut client, mut host, _cancel) = crate::data_tests::pair().await;
        let mut gate =
            crate::GrantGate::for_binding(host.context(), admin(), [4; 32], Arc::new(|| Ok(1000)))
                .unwrap();
        let challenge = gate
            .challenge(scope(), DataRole::Observer, Duration::from_secs(4))
            .unwrap();
        let grant = gate.accept(&signed(&challenge, gate.issuer_kid())).unwrap();
        client.install(&grant).unwrap();
        host.install(&grant).unwrap();
        let client = client.start().unwrap();
        let host = host.start().unwrap();
        assert!(matches!(
            host.observer_projection_sender(Duration::from_secs(1), || panic!(
                "source consulted before install"
            )),
            Err(DataError::Context),
        ));
        assert!(matches!(
            client.observer_projection_sender(Duration::from_secs(1), || true),
            Err(DataError::Role),
        ));
        // Pure context check only; no peer installation and no frame is sent.
        let authority = host.authority();
        let scope = authority.scope();
        let mut stamp = projection::Stamp {
            target: projection::Target {
                host: authority.context().host_endpoint_id.clone(),
                session: scope.session.clone(),
                run: scope.run.clone(),
                generation: scope.generation.clone(),
                incarnation: authority.context().incarnation.clone(),
            },
            binding: authority
                .binding_id()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            barrier: "11".repeat(16),
            revision: "1".into(),
            dimension_epoch: "1".into(),
            mode_epoch: "1".into(),
            structure_epoch: "1".into(),
            sync_epoch: "1".into(),
        };
        assert_eq!(check_snapshot_target(&authority, &stamp), Ok(()));
        stamp.target.generation = "999".into();
        assert_eq!(
            check_snapshot_target(&authority, &stamp),
            Err(DataError::Context)
        );
        host.close().await;
        client.close().await;
    }
}
