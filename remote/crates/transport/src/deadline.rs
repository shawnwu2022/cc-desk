use std::future::Future;
use tokio::time::Instant;
#[derive(Debug, PartialEq, Eq)]
pub struct Expired;
pub async fn bounded<T>(deadline: Instant, work: impl Future<Output = T>) -> Result<T, Expired> {
    if Instant::now() >= deadline {
        return Err(Expired);
    }
    let value = tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Expired)?;
    if Instant::now() >= deadline {
        return Err(Expired);
    }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn already_ready_work_finishing_after_deadline_is_ineligible() {
        let d = Instant::now() + std::time::Duration::from_millis(1);
        let result = bounded(d, async {
            std::thread::sleep(std::time::Duration::from_millis(10));
            7
        })
        .await;
        assert_eq!(result, Err(Expired));
    }
}
