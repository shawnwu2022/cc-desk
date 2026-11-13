use super::*;

/// 展示、消费和供应共用原移动对象；不持原 run/service 或任何已打开的 key。
pub(crate) struct FrozenHostApproval {
    pub(super) fields: HostApprovalDescription,
    pub(super) origin: NativeRunNetworkOrigin,
    cap: Instant,
    #[cfg(windows)]
    pub(super) supplier: Option<crate::native_crypto::host::FrozenHostSupplier>,
}
impl FrozenHostApproval {
    pub(crate) fn description(
        fields: HostApprovalDescription,
        admission: &NativeRunAdmission,
        cap: Instant,
    ) -> Result<Self, HostNetworkError> {
        if admission.scope().ok().as_ref() != Some(&fields.policy.scope)
            || fields.endpoint_role != cc_desk_remote_security::Role::Host
            || fields.relay_binding.1 != cc_desk_remote_transport::wss::RelayRole::Host
            || cap <= Instant::now()
        {
            return Err(HostNetworkError::Authority);
        }
        let origin = admission
            .network_origin()
            .map_err(|_| HostNetworkError::Authority)?;
        Ok(Self {
            fields,
            origin,
            cap,
            #[cfg(windows)]
            supplier: None,
        })
    }
    #[cfg(windows)]
    pub(crate) fn supplied(
        fields: HostApprovalDescription,
        admission: &NativeRunAdmission,
        cap: Instant,
        supplier: crate::native_crypto::host::FrozenHostSupplier,
    ) -> Result<Self, HostNetworkError> {
        let binding = supplier.binding();
        binding.validate().map_err(|_| HostNetworkError::Bounds)?;
        let endpoint =
            P256Spki::new(&binding.endpoint_spki).map_err(|_| HostNetworkError::Bounds)?;
        if fields.endpoint_id != endpoint.endpoint_id(cc_desk_remote_security::Role::Host)
            || fields.binding.origin != binding.origin
            || fields.relay_binding
                != (
                    binding.relay_id,
                    cc_desk_remote_transport::wss::RelayRole::Host,
                )
        {
            return Err(HostNetworkError::Authority);
        }
        let cap = cap.min(binding.profile_cap);
        let mut frozen = Self::description(fields, admission, cap)?;
        frozen.supplier = Some(supplier);
        Ok(frozen)
    }
    pub(crate) fn cap(&self) -> Instant {
        self.cap
    }
    pub(crate) fn matches(&self, admission: &NativeRunAdmission) -> bool {
        self.origin.matches(admission)
            && admission.scope().ok().as_ref() == Some(&self.fields.policy.scope)
    }
    pub(super) fn constrain_from_clock(&mut self) -> Result<(), HostNetworkError> {
        // 在外部 clock 前锚定；阻塞返回不能把同一次期限向后平移。
        let anchor = Instant::now();
        let now = self
            .fields
            .clock
            .now_ms()
            .map_err(|_| HostNetworkError::Authority)?;
        let remaining = self
            .fields
            .expires_host_ms
            .checked_sub(now)
            .filter(|ms| *ms > 0)
            .ok_or(HostNetworkError::Deadline)?;
        let mapped = anchor
            .checked_add(Duration::from_millis(remaining))
            .ok_or(HostNetworkError::Bounds)?;
        self.cap = self.cap.min(mapped);
        if self.cap <= Instant::now() {
            return Err(HostNetworkError::Deadline);
        }
        Ok(())
    }
    pub(super) fn signing_text(&self) -> String {
        #[cfg(windows)]
        if let Some(supplier) = &self.supplier {
            use sha2::{Digest, Sha256};
            let binding = supplier.binding();
            let references=supplier.key_references().map(|names|format!("Microsoft Software KSP / current user / silent open-only\nEndpoint key reference: {}\nRelay key reference: {}\n",public_text(&names[0]),public_text(&names[1]))).unwrap_or_default();
            return format!("{references}Native signing profile: {:02x?}; revision: {}\nEndpoint SPKI SHA256: {:02x?}\nRelay SPKI SHA256: {:02x?}\n",
                binding.native_profile.id, binding.native_profile.revision,
                Sha256::digest(binding.endpoint_spki), Sha256::digest(binding.relay_spki));
        }
        String::new()
    }
    #[cfg(test)]
    pub(super) fn fixture(
        fields: HostApprovalDescription,
        origin: NativeRunNetworkOrigin,
        cap: Instant,
    ) -> Self {
        Self {
            fields,
            origin,
            cap,
            #[cfg(windows)]
            supplier: None,
        }
    }
    #[cfg(test)]
    pub(crate) fn into_fixture_parts(self) -> (HostApprovalDescription, Instant) {
        #[cfg(windows)]
        assert!(
            self.supplier.is_none(),
            "legacy fixture cannot replace a frozen supplier"
        );
        (self.fields, self.cap)
    }
}
