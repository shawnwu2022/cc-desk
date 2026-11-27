//! Adapter from the independent management TLS lane to the original native provider.
use super::{CallerIdentity, ManagementPortfolio, NativeRuntime};
use crate::cli::{
    native_projection::{
        catalogue_history_source::RetainedHistory, service::CurrentManagementPeer,
    },
    profiles::error,
    types::WireU64,
};
use cc_desk_remote_security::{
    session_management_channel::{
        self, ChannelError, ManagementDispatch, ManagementOutcome, ManagementRequest,
        VerifiedManagementRoute,
    },
    CancelToken, LocalIdentity, PeerPin, Unbound,
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::io::{AsyncRead, AsyncWrite};

/// Built by the independent desktop management owner after it consumes its
/// signed selection. The phone cannot construct or choose these backend facts.
pub(crate) struct ManagementRenameAdmission {
    pub(crate) caller: CallerIdentity,
    pub(crate) portfolio: Arc<ManagementPortfolio>,
    pub(crate) inventory_epoch: WireU64,
    pub(crate) prepare_epoch: WireU64,
    pub(crate) commit_epoch: WireU64,
    pub(crate) current_peer: CurrentManagementPeer,
}

impl NativeRuntime {
    /// No renderer/IPC command exposes this handler. The management route
    /// owns the single lifetime and only returns a sanitized receipt.
    pub(crate) async fn serve_management_rename<S>(
        &self,
        stream: Unbound<S>,
        identity: LocalIdentity,
        pin: PeerPin,
        route: VerifiedManagementRoute,
        cancel: CancelToken,
        admission: ManagementRenameAdmission,
    ) -> Result<(), ChannelError>
    where
        S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        self.binding()
            .and_then(|binding| binding.check_admitted(&admission.caller))
            .map_err(|_| ChannelError::Denied)?;
        if admission.inventory_epoch.get() >= admission.prepare_epoch.get()
            || admission.prepare_epoch.get() >= admission.commit_epoch.get()
        {
            return Err(ChannelError::Denied);
        }
        let admission = Arc::new(admission);
        let current = admission.clone();
        let preparing = admission.clone();
        let listing = admission.clone();
        let targets = Arc::new(parking_lot::Mutex::new(BTreeMap::<
            [u8; 32],
            (Arc<RetainedHistory>, [u8; 32]),
        >::new()));
        let inventory_targets = targets.clone();
        let inventory_service = self.projections.clone();
        let read = self.projections.clone();
        let write = self.projections.clone();
        let close = self.supervisor.clone();
        let run_supervisor = self.supervisor.clone();
        let launch = self.service.clone();
        session_management_channel::serve_with_inventory(
            stream,
            identity,
            pin,
            route,
            cancel,
            move |route| {
                let current = current.clone();
                async move {
                    tokio::task::spawn_blocking(move || {
                        (current.current_peer)()
                            .is_ok_and(|pair| pair == (route.phone, route.membership_binding))
                    })
                    .await
                    .unwrap_or(false)
                }
            },
            move |mode| async move {
                tokio::task::spawn_blocking(move || {
                    use cc_desk_remote_security::session_catalogue::ListMode;
                    let mut physical_rows = BTreeMap::new();
                    for source in &listing.portfolio.sources {
                        let records = inventory_service
                            .management_inventory(
                                &listing.caller,
                                source,
                                listing.inventory_epoch,
                                &listing.current_peer,
                            )
                            .map_err(|_| ChannelError::Denied)?;
                        for (row, physical) in records {
                            if row.archived != (mode == ListMode::Archived) {
                                continue;
                            }
                            physical_rows
                                .entry(physical)
                                .and_modify(|slot| *slot = None)
                                .or_insert_with(|| Some((row, source.clone())));
                            if physical_rows.len() > 512 {
                                return Err(ChannelError::Denied);
                            }
                        }
                    }
                    let mut map = inventory_targets.lock();
                    let mut rows = Vec::new();
                    for (physical, (row, source)) in physical_rows
                        .into_iter()
                        .filter_map(|(physical, found)| found.map(|record| (physical, record)))
                    {
                        if map.insert(row.reference, (source, physical)).is_some() {
                            return Err(ChannelError::Denied);
                        }
                        rows.push(row);
                    }
                    rows.sort_by(|a, b| {
                        a.archived
                            .cmp(&b.archived)
                            .then_with(|| a.title.cmp(&b.title))
                    });
                    Ok(rows)
                })
                .await
                .unwrap_or(Err(ChannelError::Unavailable))
            },
            move |request: ManagementRequest| async move {
                let (retained, physical) = targets.lock().remove(&request.reference)?;
                tokio::task::spawn_blocking(move || {
                    let grant = read
                        .prepare_remote_rename(
                            &preparing.caller,
                            &retained,
                            preparing.prepare_epoch,
                            physical,
                            request.action,
                            &preparing.current_peer,
                        )
                        .ok()?;
                    Some((grant.token(), (retained, grant)))
                })
                .await
                .ok()
                .flatten()
            },
            move |(retained, grant), action, token, cancel, end| async move {
                tokio::task::spawn_blocking(move || {
                    let peer = admission.current_peer.clone();
                    let view_cancel = cancel.clone();
                    let fenced: CurrentManagementPeer = Arc::new(move || {
                        let open = || !cancel.is_cancelled() && tokio::time::Instant::now() < end;
                        if !open() {
                            return Err(error("REMOTE_MANAGEMENT_ROUTE_EXPIRED"));
                        }
                        let exact = peer()?;
                        if !open() {
                            return Err(error("REMOTE_MANAGEMENT_ROUTE_EXPIRED"));
                        }
                        Ok(exact)
                    });
                    let mut view = None;
                    if write
                        .execute_remote_rename(
                            &admission.caller,
                            &retained,
                            admission.commit_epoch,
                            grant,
                            token,
                            &action,
                            fenced,
                            |receipt, check| {
                                close
                                    .as_ref()
                                    .ok_or_else(|| error("NATIVE_RUNTIME_NOT_READY"))?
                                    .stop_from_paired_management(&admission.caller, receipt, || {
                                        check()
                                    })
                            },
                            |receipt, check| {
                                check()?;
                                let access = launch.access(&admission.caller, &receipt.run)?;
                                let original = run_supervisor
                                    .as_ref()
                                    .ok_or_else(|| error("NATIVE_RUNTIME_NOT_READY"))?
                                    .admit_remote_run(access, &receipt.run)?;
                                let live = Arc::new(move || {
                                    !view_cancel.is_cancelled() && tokio::time::Instant::now() < end
                                });
                                let source = Arc::new(move || {
                                    check().ok()?;
                                    let text = original.management_projection(live.clone()).ok()?;
                                    check().ok()?;
                                    Some(text)
                                });
                                view = Some(ManagementDispatch::Projection(source));
                                Ok(())
                            },
                        )
                        .is_ok()
                    {
                        view.unwrap_or_else(|| ManagementOutcome::Applied.into())
                    } else {
                        // The provider could have written before the final fence.
                        ManagementOutcome::Unknown.into()
                    }
                })
                .await
                .unwrap_or_else(|_| ManagementOutcome::Unknown.into())
            },
        )
        .await
    }
}
