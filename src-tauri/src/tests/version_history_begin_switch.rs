use crate::version_history::manager::{BeginSwitchRequest, InspectSwitchRequest, SwitchDataMode};

// 明确fresh-settings命令只接受准备令牌，不接受安装路径/PID/digest/兼容性布尔值。
#[test]
fn HistoryBeginSwitch_NoCallerAuthority_001() {
    let request: BeginSwitchRequest = serde_json::from_str(
        r#"{"preparationId":"00112233445566778899aabbccddeeff","dataMode":"fresh-settings"}"#,
    )
    .unwrap();
    assert!(matches!(request.data_mode, SwitchDataMode::FreshSettings));
    assert_eq!(request.preparation_id, "00112233445566778899aabbccddeeff");
    for body in [
        r#"{"preparationId":"x","dataMode":"keep-current-data"}"#,
        r#"{"preparationId":"x","dataMode":"fresh-settings","sourceExited":true}"#,
        r#"{"preparationId":"x","dataMode":"fresh-settings","path":"C:\\target"}"#,
        r#"{"preparationId":"x","dataMode":"fresh-settings","pid":42}"#,
    ] {
        assert!(serde_json::from_str::<BeginSwitchRequest>(body).is_err());
    }
    assert!(serde_json::from_str::<InspectSwitchRequest>(
        r#"{"preparationId":"x","transactionId":"foreign"}"#
    )
    .is_err());
}

// 真实Rust序列化固定前端wire；这些只是投影样本，不构造任何OS执行凭证。
#[test]
fn HistoryBeginSwitch_ReviewWireFixture_002() {
    use crate::version_history::manager::{
        SwitchContextPolicy, SwitchReview, SwitchReviewAction as A, SwitchReviewBlock as B,
        SwitchReviewPhase as P, SwitchTicket,
    };
    let samples = [
        (
            P::Preparing,
            None,
            vec![A::Refresh, A::CancelPreparation],
            Some(B::PreparationPending),
        ),
        (
            P::Verified,
            None,
            vec![A::Refresh, A::Review, A::CancelPreparation],
            Some(B::PayloadUnverified),
        ),
        (
            P::Verified,
            None,
            vec![A::Refresh, A::Review, A::CancelPreparation],
            Some(B::CoordinatorUnavailable),
        ),
        (
            P::Verified,
            None,
            vec![A::Refresh, A::Review, A::CancelPreparation, A::BeginSwitch],
            None,
        ),
        (
            P::HandoffIssued,
            Some("00112233-4455-4677-8899-aabbccddeeff".into()),
            vec![A::Refresh],
            Some(B::HandoffIssued),
        ),
        (P::Cancelled, None, vec![A::Refresh], None),
        (
            P::Aborted,
            Some("00112233-4455-4677-8899-aabbccddeeff".into()),
            vec![A::Refresh, A::PrepareAgain],
            None,
        ),
        (
            P::Unavailable,
            None,
            vec![A::Refresh, A::CancelPreparation],
            Some(B::PreparationExpired),
        ),
    ]
    .into_iter()
    .map(
        |(phase, transaction_id, allowed_actions, block_reason)| SwitchReview {
            preparation_id: "00112233445566778899aabbccddeeff".into(),
            version: "0.17.7".into(),
            phase,
            context_policy: SwitchContextPolicy::FreshSettingsPreserveCurrentSharedCli,
            transaction_id,
            allowed_actions,
            block_reason,
        },
    )
    .collect::<Vec<_>>();
    let actual = serde_json::json!({"reviews":samples,"ticket":SwitchTicket{transaction_id:"00112233-4455-4677-8899-aabbccddeeff".into()}});
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/version-switch-wire.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}
