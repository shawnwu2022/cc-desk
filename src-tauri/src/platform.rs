//! Platform boundary: preserve the existing route while introducing explicit launches.
mod legacy;
pub(crate) use legacy::*;
pub(crate) mod launch;
