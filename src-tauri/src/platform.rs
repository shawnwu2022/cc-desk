//! Platform boundary: preserve the existing route while introducing explicit launches.
pub(crate) mod admitted_child;
pub(crate) mod launch;
mod launch_limits;
mod legacy;
pub(crate) mod legacy_control;
pub(crate) mod owned_pty;
pub(crate) use legacy::*;
