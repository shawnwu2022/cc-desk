//! Platform boundary: preserve the existing route while introducing explicit launches.
pub(crate) mod admitted_child;
pub(crate) mod launch;
pub(crate) mod legacy_control;
mod launch_limits;
mod legacy;
pub(crate) mod owned_pty;
pub(crate) use legacy::*;
