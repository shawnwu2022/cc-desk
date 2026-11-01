//! Backend-approved connection inputs only. No Serde, JS factory, key store or
//! fixture identity is provided. Native signer implementations must satisfy the
//! security/transport cooperative poll and drop contracts before use here.
use crate::NativeClient;
use cc_desk_remote_security::{
    self as security, ClientGrantState, ClientGrantTrust, ClientTrust, LocalIdentity,
    NativeGrantSigner, PeerPin, RunningChannel,
};
use cc_desk_remote_transport::{self as transport, wss::RelayAdmissionIdentity};
use std::{net::SocketAddr, time::Duration};
use tokio::{sync::watch, time::Instant};
#[cfg(test)]
#[path = "bootstrap_tests.rs"]
mod tests;

pub struct NativeProfile {
    pub address: SocketAddr,
    pub relay: RelayAdmissionIdentity,
    pub tunnel: [u8; 16],
    pub local: LocalIdentity,
    pub host_pin: PeerPin,
    pub binding: ClientTrust,
    pub grant: ClientGrantTrust,
    pub signer: Box<dyn NativeGrantSigner>,
    pub state: ClientGrantState,
    pub setup_budget: Duration,
    pub io_budget: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapError {
    Bounds,
    Cancelled,
    Deadline,
    Relay,
    Security,
    Grant,
    Install,
    RetirementUnknown,
}
fn remaining(end: Instant) -> Result<Duration, BootstrapError> {
    let b = end.saturating_duration_since(Instant::now());
    if b.is_zero() {
        Err(BootstrapError::Deadline)
    } else {
        Ok(b)
    }
}

/// Run only inside the application service's retained original creation task.
/// The service cancels via stop and joins this whole function; it must never
/// implement a caller deadline by dropping this construction/cleanup future.
pub async fn connect_native(
    profile: NativeProfile,
    mut stop: watch::Receiver<bool>,
) -> Result<NativeClient, BootstrapError> {
    if *stop.borrow() || stop.has_changed().is_err() {
        return Err(BootstrapError::Cancelled);
    }
    if profile.setup_budget.is_zero()
        || profile.setup_budget > Duration::from_secs(10)
        || profile.io_budget.is_zero()
        || profile.io_budget > transport::MAX_IO_BUDGET
        || profile.tunnel == [0; 16]
    {
        return Err(BootstrapError::Bounds);
    }
    let end = Instant::now()
        .checked_add(profile.setup_budget)
        .ok_or(BootstrapError::Bounds)?;
    let socket = {
        let connect = transport::wss::connect_admitted(
            profile.binding.origin.as_str(),
            profile.address,
            remaining(end)?,
            &profile.relay,
        );
        tokio::pin!(connect);
        tokio::select! {biased;_=stop.changed()=>return Err(BootstrapError::Cancelled),result=&mut connect=>result.map_err(|_|BootstrapError::Relay)?}
    };
    if *stop.borrow() || stop.has_changed().is_err() {
        return Err(BootstrapError::Cancelled);
    }
    let (stream, owner) = transport::start_admitted(socket, profile.tunnel, profile.io_budget)
        .map_err(|_| BootstrapError::Relay)?;
    finish_channel(profile, stop, end, stream, owner).await
}
async fn finish_channel(
    mut profile: NativeProfile,
    mut stop: watch::Receiver<bool>,
    end: Instant,
    stream: tokio::io::DuplexStream,
    owner: transport::OwnedTunnel,
) -> Result<NativeClient, BootstrapError> {
    let (cancel, token) = security::cancellation();
    let mut mux = Some(owner);
    let mut running: Option<RunningChannel> = None;
    let mut installed = None;
    let result = {
        let work = async {
            let pending = security::connect(
                security::Unbound::new(stream),
                profile.local,
                profile.host_pin,
                remaining(end)?,
                token,
            )
            .await
            .map_err(|_| BootstrapError::Security)?;
            let managed = security::bind_client_peer_channel(pending, profile.binding)
                .await
                .map_err(|_| BootstrapError::Security)?;
            let mut managed = managed
                .grant_client(
                    &profile.grant,
                    profile.signer.as_mut(),
                    &mut profile.state,
                    remaining(end)?,
                )
                .await
                .map_err(|_| BootstrapError::Grant)?;
            running = Some(
                managed
                    .start_peer_pending(end)
                    .map_err(|_| BootstrapError::Install)?,
            );
            installed = Some(
                running
                    .as_ref()
                    .expect("original pending channel")
                    .client_installed()
                    .await
                    .map_err(|_| BootstrapError::Install)?,
            );
            remaining(end)?;
            Ok(())
        };
        tokio::pin!(work);
        // The original mux/channel slots live outside this future. A native
        // poll panic must not unwind past them and discard their JoinHandles.
        let guarded = std::future::poll_fn(|cx| {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::future::Future::poll(work.as_mut(), cx)
            })) {
                Ok(poll) => poll,
                Err(_) => std::task::Poll::Ready(Err(BootstrapError::RetirementUnknown)),
            }
        });
        tokio::pin!(guarded);
        tokio::select! {biased;_=stop.changed()=>Err(BootstrapError::Cancelled),r=&mut guarded=>r}
    };
    let mut result = if *stop.borrow() || stop.has_changed().is_err() {
        Err(BootstrapError::Cancelled)
    } else {
        result
    };
    if result.is_ok() {
        let channel = running.take().expect("installed original channel");
        match NativeClient::from_installed_retained(
            channel,
            installed.take().expect("original install proof"),
        ) {
            Ok(mut client) => {
                client.mux = mux.take();
                client.lifetime_cancel = Some(cancel);
                let ineligible = if *stop.borrow() || stop.has_changed().is_err() {
                    Some(BootstrapError::Cancelled)
                } else if remaining(end).is_err() {
                    Some(BootstrapError::Deadline)
                } else {
                    None
                };
                if let Some(error) = ineligible {
                    client
                        .retire()
                        .await
                        .map_err(|_| BootstrapError::RetirementUnknown)?;
                    return Err(error);
                }
                return Ok(client);
            }
            Err(original) => {
                let (error, channel) = *original;
                running = Some(channel);
                result = Err(if error == crate::DataError::Uncertain {
                    BootstrapError::RetirementUnknown
                } else {
                    BootstrapError::Install
                });
            }
        }
    }
    // Signal every owner before joining any. No timeout wraps retirement and no
    // original handle is moved into a disposable caller's waiting future.
    cancel.cancel();
    if let Some(channel) = &running {
        channel.authority().request_close();
    }
    if let Some(owner) = &mux {
        owner.request_close();
    }
    if let Some(channel) = &running {
        channel.wait_closed().await;
    }
    if let Some(owner) = &mux {
        owner
            .wait_closed()
            .await
            .map_err(|_| BootstrapError::RetirementUnknown)?;
    }
    Err(result.err().unwrap_or(BootstrapError::Install))
}
