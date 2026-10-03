//! One-way state tests only. These cannot manufacture SourceHandoffTerminal,
//! a no-launch capability, a terminal installer, or a production ReturnBoundary.
use super::*;

// 开始历史进程创建前即永久撤销全部见证；写意图失败也不能恢复权限。
#[test]
fn HistoryNoHistoricalLaunch_CreateRevokesEveryWitness_001() {
    let state = Arc::new(LaunchState::new());
    let first = state.clone();
    let second = state.clone();
    first.verify(AVAILABLE).unwrap();
    second.verify(AVAILABLE).unwrap();
    state.begin_historical_creation().unwrap();
    assert!(first.verify(AVAILABLE).is_err());
    assert!(second.verify(AVAILABLE).is_err());
    assert!(first.claim_return().is_err());
    assert!(state.begin_historical_creation().is_err());
    state.verify(HISTORICAL_CREATION_BEGAN).unwrap();
}

// Return 的唯一认领阻止后续历史创建与重复认领，不存在重置操作。
#[test]
fn HistoryNoHistoricalLaunch_ReturnClaimBlocksLaunch_002() {
    let state = Arc::new(LaunchState::new());
    let witness = state.clone();
    state.claim_return().unwrap();
    witness.verify(RETURN_CLAIMED).unwrap();
    assert!(witness.begin_historical_creation().is_err());
    assert!(witness.claim_return().is_err());
    state.verify(RETURN_CLAIMED).unwrap();
}

// 两条分支竞争同一原子状态时，只能有一条成功，不能同时授权。
#[test]
fn HistoryNoHistoricalLaunch_ConcurrentBranchesHaveOneWinner_003() {
    let state = Arc::new(LaunchState::new());
    let start = Arc::new(std::sync::Barrier::new(3));
    let launch = {
        let state = state.clone();
        let start = start.clone();
        std::thread::spawn(move || {
            start.wait();
            state.begin_historical_creation().is_ok()
        })
    };
    let returning = {
        let state = state.clone();
        let start = start.clone();
        std::thread::spawn(move || {
            start.wait();
            state.claim_return().is_ok()
        })
    };
    start.wait();
    assert_ne!(launch.join().unwrap(), returning.join().unwrap());
    assert!(state.verify(AVAILABLE).is_err());
    assert!(state.claim_return().is_err());
    assert!(state.begin_historical_creation().is_err());
}
