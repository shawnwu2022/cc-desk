#[cfg(windows)]
use std::sync::Arc;
use std::time::Instant;

/// 网络只得到原 token 与原本机期限；没有回调、Lease、service 或 run。
#[derive(Clone)]
pub(crate) struct NativeApprovalStop {
    #[cfg(windows)]
    token: Arc<crate::native_crypto::owner::NativeRevocation>,
    end: Instant,
}
impl NativeApprovalStop {
    #[cfg(windows)]
    pub(crate) fn new(
        token: Arc<crate::native_crypto::owner::NativeRevocation>,
        end: Instant,
    ) -> Self {
        Self { token, end }
    }
    pub(crate) async fn wait(stop: Option<&Self>) {
        let Some(stop) = stop else {
            return std::future::pending().await;
        };
        loop {
            if stop.is_invalid() {
                return;
            }
            // 只在原网络任务中等待，不调用 GrantClock，也不创建新的 owner。
            tokio::time::sleep_until(tokio::time::Instant::from_std(
                stop.end
                    .min(Instant::now() + std::time::Duration::from_millis(10)),
            ))
            .await;
        }
    }
    pub(crate) fn revoke(&self) {
        #[cfg(windows)]
        self.token.revoke();
    }
    pub(crate) fn is_invalid(&self) -> bool {
        if Instant::now() >= self.end {
            self.revoke();
            return true;
        }
        #[cfg(windows)]
        {
            self.token.is_revoked()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[tokio::test]
    #[allow(non_snake_case)]
    async fn HostApproval_StopWaitPreservesOriginalEnd_011() {
        use std::time::Duration;
        assert!(
            tokio::time::timeout(Duration::from_millis(20), NativeApprovalStop::wait(None))
                .await
                .is_err()
        );
        let token = crate::native_crypto::owner::NativeRevocation::new();
        let stop = NativeApprovalStop::new(token.clone(), Instant::now() + Duration::from_secs(30));
        token.revoke();
        tokio::time::timeout(
            Duration::from_millis(100),
            NativeApprovalStop::wait(Some(&stop)),
        )
        .await
        .unwrap();
        let token = crate::native_crypto::owner::NativeRevocation::new();
        let end = Instant::now() + Duration::from_millis(30);
        let stop = NativeApprovalStop::new(token.clone(), end);
        assert!(!token.is_revoked());
        tokio::time::timeout(
            Duration::from_secs(1),
            NativeApprovalStop::wait(Some(&stop)),
        )
        .await
        .unwrap();
        assert!(token.is_revoked());
        assert!(Instant::now() >= end);
    }
}
