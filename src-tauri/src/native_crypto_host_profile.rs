//! 原生 Host 两用途公开快照；没有配置发现、持久化或批准能力。
#![allow(dead_code, reason = "native profile selection is not installed")]
use super::{
    existing::ExistingKeyLoader,
    host::{FrozenHostBinding, FrozenHostSupplier, Opener},
    owner::SignFailure,
    SigningPurpose,
};
pub(crate) struct NativeHostProfile {
    binding: FrozenHostBinding,
    keys: [ExistingKeyLoader; 2],
}
impl NativeHostProfile {
    pub(crate) fn new(
        binding: FrozenHostBinding,
        endpoint_name: &str,
        relay_name: &str,
    ) -> Result<Self, SignFailure> {
        binding.validate()?;
        if endpoint_name == relay_name {
            return Err(SignFailure::Bounds);
        }
        let keys = [
            ExistingKeyLoader::new(
                endpoint_name,
                SigningPurpose::Endpoint,
                binding.endpoint_spki,
            )?,
            ExistingKeyLoader::new(relay_name, SigningPurpose::Relay, binding.relay_spki)?,
        ];
        Ok(Self { binding, keys })
    }
    pub(crate) fn freeze(self) -> FrozenHostSupplier {
        let [endpoint, relay] = self.keys;
        FrozenHostSupplier::from_openers(
            self.binding,
            Opener::Existing(endpoint),
            Opener::Existing(relay),
        )
    }
    #[cfg(test)]
    pub(crate) fn test_anonymous(
        mut self,
        scalars: [[u8; 32]; 2],
        opened: std::sync::Arc<[std::sync::atomic::AtomicUsize; 2]>,
    ) -> Self {
        for (i, scalar) in scalars.into_iter().enumerate() {
            self.keys[i].test_anonymous(scalar);
            self.keys[i].test_count(opened.clone(), i);
        }
        self
    }
    #[cfg(test)]
    pub(crate) fn test_api(
        mut self,
        endpoint: std::sync::Arc<dyn super::existing::MockKeyApi>,
        relay: std::sync::Arc<dyn super::existing::MockKeyApi>,
    ) -> Self {
        self.keys[0].test_api(endpoint);
        self.keys[1].test_api(relay);
        self
    }
}
