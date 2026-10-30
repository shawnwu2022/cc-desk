//! Experimental transport authentication only. Default feature exposes no endpoint API.
#[cfg(feature = "experimental")]
mod pin;
#[cfg(feature = "experimental")]
mod tls;
#[cfg(feature = "experimental")]
pub use tls::*;
#[cfg(feature = "experimental")]
mod cancellation;
#[cfg(feature = "experimental")]
pub use cancellation::{CancelHandle, CancelToken, cancellation};
#[cfg(feature = "experimental")]
mod bind;
#[cfg(feature = "experimental")]
pub use bind::*;
#[cfg(feature = "experimental")]
mod grant;
#[cfg(feature = "experimental")]
pub use grant::{GrantClock, GrantError, GrantGate, VerifiedGrant};
#[cfg(feature = "experimental")]
mod data;
#[cfg(feature = "experimental")]
pub use data::{
    ClientGrantState, ClientGrantTrust, DataError, DataRecord, ManagedChannel, NativeGrantPolicy,
    NativeGrantSigner, RemoteGrant, VerifiedCanonicalGrant,
};
#[cfg(feature = "experimental")]
mod data_owner;
#[cfg(feature = "experimental")]
pub use data_owner::{DATA_QUEUE_SLOTS, RunningChannel};
#[cfg(feature = "experimental")]
mod data_authority;
#[cfg(feature = "experimental")]
pub use data_authority::{BoundDataAuthority, GrantClockDomain};
#[cfg(all(test, feature = "experimental"))]
mod data_tests;
#[cfg(all(test, feature = "experimental"))]
mod grant_tests;
#[cfg(all(test, feature = "experimental"))]
#[path = "../tests/support/identities.rs"]
#[allow(dead_code)]
mod test_identities;
