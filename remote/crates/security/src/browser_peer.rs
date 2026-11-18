use crate::{
    BoundDataAuthority, CancelToken, DataError, Role,
    data::ChannelGrant,
    peer_install::Shared,
    tls_clock::{self, Deadline},
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::watch,
};
pub(crate) struct Installed {
    peer: Arc<Shared>,
    authority: BoundDataAuthority,
    access: ChannelGrant,
    pub expiry: Deadline,
    pub fence: u64,
}
impl Drop for Installed {
    fn drop(&mut self) {
        self.peer.terminal();
        self.authority.request_close();
    }
}
impl Installed {
    pub fn check(&self) -> Result<(), DataError> {
        self.authority.check()?;
        if self.peer.fence()? != self.fence {
            return Err(DataError::Fence);
        }
        Ok(())
    }
    pub fn revoked(&self) -> (watch::Receiver<bool>, watch::Receiver<bool>) {
        self.access.revoked()
    }
    pub fn receipt(&self) -> Result<Vec<u8>, DataError> {
        self.check()?;
        serde_json::to_vec(&serde_json::json!({"kind":"PeerInstalledHistorical","fence":self.fence.to_string(),"grantId":hex(&self.authority.grant_id()),"scope":self.authority.scope()})).map_err(|_|DataError::Codec)
    }
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}
pub(crate) async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut tokio_rustls::TlsStream<S>,
    authority: BoundDataAuthority,
    access: ChannelGrant,
    mut cancel: CancelToken,
    mut stopped: watch::Receiver<bool>,
    budget: Duration,
) -> Result<Installed, DataError> {
    authority.check()?;
    let sample_before = tls_clock::sample_floor()?;
    let remaining = access.check().map_err(DataError::Grant)?;
    let budget = budget
        .min(Duration::from_secs(10))
        .min(Duration::from_millis(remaining));
    let (end, expiry) = tls_clock::peer_times(budget, remaining, sample_before)?;
    let (peer, ports) = Shared::new_browser(end);
    let installed = Installed {
        peer: peer.clone(),
        authority,
        access,
        expiry,
        fence: 0,
    };
    let (mut revoked, mut superseded) = installed.access.revoked();
    tokio::select! {biased;
     _=cancel.cancelled()=>Err(DataError::Cancelled),_=stopped.changed()=>Err(DataError::Closed),_=super::shutdown_requested()=>Err(DataError::Cancelled),
     _=revoked.changed()=>Err(DataError::Grant(crate::GrantError::Revoked)),_=superseded.changed()=>Err(DataError::Grant(crate::GrantError::Revoked)),
     _=tls_clock::sleep(end)=>Err(DataError::Deadline),
     result=crate::peer_install::bootstrap(io,&installed.authority,&installed.access,Role::Client,peer.clone(),ports)=>result,
    }?;
    installed.authority.check()?;
    if tls_clock::expired(end) {
        return Err(DataError::Deadline);
    }
    // 原bootstrap的已完成oneshot只取一次；不启动第二timer，不从DTO构造proof。
    let proof = peer.completed_client()?;
    let fence = proof.consume_for_owner(&installed.authority, &peer)?;
    if cancel.is_cancelled() {
        return Err(DataError::Cancelled);
    }
    if tls_clock::expired(end) {
        return Err(DataError::Deadline);
    }
    let mut installed = installed;
    installed.fence = fence;
    installed.check()?;
    Ok(installed)
}
