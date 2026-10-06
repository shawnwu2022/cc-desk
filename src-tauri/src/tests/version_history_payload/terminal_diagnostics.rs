//! Pure, test-only policy for bounded observations after an already failed worker.
use std::time::Duration;

pub(super) const WORKER_WINDOW_MS: u32 = 300_000;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct MembershipSummary {
    pub(super) complete: Option<bool>,
    pub(super) worker_listed: Option<bool>,
    pub(super) other_members: Option<u32>,
}

impl MembershipSummary {
    pub(super) fn unknown() -> Self {
        Self {
            complete: None,
            worker_listed: None,
            other_members: None,
        }
    }
}

pub(super) fn summarize_members(
    assigned: u32,
    listed: u32,
    members: &[usize],
    worker: usize,
) -> MembershipSummary {
    if assigned != listed || listed as usize > members.len() {
        return MembershipSummary {
            complete: Some(false),
            worker_listed: None,
            other_members: None,
        };
    }
    let members = &members[..listed as usize];
    MembershipSummary {
        complete: Some(true),
        worker_listed: Some(members.contains(&worker)),
        other_members: Some(members.iter().filter(|member| **member != worker).count() as u32),
    }
}

pub(super) fn remaining_ms(worker_elapsed: Duration, diagnostic_elapsed: Duration) -> u32 {
    let worker = Duration::from_millis(u64::from(WORKER_WINDOW_MS)).saturating_sub(worker_elapsed);
    let diagnostic = Duration::from_millis(20).saturating_sub(diagnostic_elapsed);
    worker.min(diagnostic).as_millis() as u32
}

pub(super) fn sample_failure(
    mut remaining: impl FnMut() -> u32,
    mut pause: impl FnMut(u32),
    mut observe: impl FnMut(u8),
) {
    for sample in 0..3 {
        if remaining() == 0 {
            break;
        }
        observe(sample);
        // This never produces a success receipt. A late zero is only evidence.
        if sample == 2 || remaining() <= 5 {
            break;
        }
        pause(5);
    }
}

#[cfg(test)]
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    // 检查受限观察始终同时服从原 worker 窗口与独立 20ms 观察上限。
    #[test]
    fn WorkerTerminalDiagnostic_OriginalBudget_001() {
        assert_eq!(WORKER_WINDOW_MS, 300_000);
        let ms = Duration::from_millis;
        assert_eq!(remaining_ms(ms(0), ms(0)), 20);
        assert_eq!(remaining_ms(ms(299_997), ms(2)), 3);
        assert_eq!(remaining_ms(ms(1), ms(19)), 1);
        assert_eq!(remaining_ms(ms(300_000), ms(0)), 0);
        assert_eq!(remaining_ms(Duration::MAX, ms(0)), 0);
        assert_eq!(remaining_ms(ms(1), ms(20)), 0);
        assert_eq!(remaining_ms(ms(1), Duration::MAX), 0);
        assert_eq!(remaining_ms(Duration::from_micros(299_999_999), ms(0)), 0);
        assert_eq!(remaining_ms(ms(1), Duration::from_micros(19_999)), 0);
    }

    // 检查最多记录三次且固定暂停总量不超过 20ms，不会形成无界重试。
    #[test]
    fn WorkerTerminalDiagnostic_BoundedSamples_002() {
        let pauses = RefCell::new(Vec::new());
        let samples = RefCell::new(Vec::new());
        sample_failure(
            || 20,
            |ms| pauses.borrow_mut().push(ms),
            |n| samples.borrow_mut().push(n),
        );
        assert_eq!(*samples.borrow(), [0, 1, 2]);
        assert_eq!(*pauses.borrow(), [5, 5]);
    }

    // 检查 API 观察或暂停消耗最后预算后不再发起后续采样。
    #[test]
    fn WorkerTerminalDiagnostic_ConsumedBudget_003() {
        let remaining = Cell::new(20);
        let samples = Cell::new(0);
        sample_failure(
            || remaining.get(),
            |_| panic!("no budget to pause"),
            |_| {
                samples.set(samples.get() + 1);
                remaining.set(0);
            },
        );
        assert_eq!(samples.get(), 1);
        sample_failure(
            || 0,
            |_| panic!("expired pause"),
            |_| panic!("expired observation"),
        );
        let remaining = Cell::new(6);
        let samples = Cell::new(0);
        sample_failure(
            || remaining.get(),
            |_| remaining.set(0),
            |_| samples.set(samples.get() + 1),
        );
        assert_eq!(samples.get(), 1);
    }

    // 检查不足一个采样间隔时仅保留当前观察，不消耗超出剩余窗口的等待。
    #[test]
    fn WorkerTerminalDiagnostic_ShortBudget_004() {
        let samples = Cell::new(0);
        sample_failure(
            || 5,
            |_| panic!("insufficient pause budget"),
            |_| samples.set(samples.get() + 1),
        );
        assert_eq!(samples.get(), 1);
    }

    // 检查完整成员列表只发布自身成员布尔值和其他成员数，不携带标识。
    #[test]
    fn WorkerTerminalDiagnostic_CompleteMembership_005() {
        assert_eq!(
            summarize_members(2, 2, &[42, 88, 0], 42),
            MembershipSummary {
                complete: Some(true),
                worker_listed: Some(true),
                other_members: Some(1),
            }
        );
        assert_eq!(
            summarize_members(1, 1, &[88, 42], 42),
            MembershipSummary {
                complete: Some(true),
                worker_listed: Some(false),
                other_members: Some(1),
            }
        );
        assert_eq!(
            summarize_members(0, 0, &[42], 42),
            MembershipSummary {
                complete: Some(true),
                worker_listed: Some(false),
                other_members: Some(0),
            }
        );
    }

    // 检查截断、容量不符与 API 错误均保留 unknown，绝不冒充零成员。
    #[test]
    fn WorkerTerminalDiagnostic_UnknownMembership_006() {
        for (assigned, listed, members) in
            [(2, 1, vec![42]), (1, 2, vec![42, 88]), (2, 2, vec![42])]
        {
            assert_eq!(
                summarize_members(assigned, listed, &members, 42),
                MembershipSummary {
                    complete: Some(false),
                    worker_listed: None,
                    other_members: None,
                }
            );
        }
        assert_eq!(
            MembershipSummary::unknown(),
            MembershipSummary {
                complete: None,
                worker_listed: None,
                other_members: None,
            }
        );
    }
}
