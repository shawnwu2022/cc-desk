use crate::{bound_test_support as support, observer_output::ObserverOutput};
use cc_desk_remote_record::grant::DataRole;
use std::sync::{Arc, atomic::AtomicU64};

#[tokio::test]
async fn observer_output_is_scoped_to_a_live_read_only_grant() {
    let (client, host, _gate, cancel, _grant) =
        support::channels_as(Arc::new(AtomicU64::new(1000)), [4; 32], DataRole::Observer).await;
    let scope = support::scope();
    let mut other = scope.clone();
    other.run.push_str("-other");
    assert!(ObserverOutput::new(host.authority(), &other, 2, 8).is_err());
    assert!(ObserverOutput::new(client.authority(), &scope, 2, 8).is_err());
    let mut output = ObserverOutput::new(host.authority(), &scope, 2, 8).unwrap();
    output.append(&scope, 0, b"VIEW").unwrap();
    assert!(output.snapshot(&other).is_err());
    let snapshot = output.snapshot(&scope).unwrap();
    assert_eq!((snapshot.rows(), snapshot.columns()), (2, 8));
    assert_eq!(snapshot.cell(0, 0).unwrap().scalar, 'V');
    cancel.cancel();
    assert!(output.snapshot(&scope).is_err());
    host.close().await;
    client.close().await;
}

#[tokio::test]
async fn observer_output_refuses_discontinuous_stream_and_controller() {
    let (client, host, _gate, _cancel, _grant) =
        support::channels_as(Arc::new(AtomicU64::new(1000)), [4; 32], DataRole::Observer).await;
    let scope = support::scope();
    let mut output = ObserverOutput::new(host.authority(), &scope, 2, 8).unwrap();
    assert!(output.append(&scope, 4, b"GAP").is_err());
    assert!(output.append(&scope, 0, b"VIEW").is_err());
    assert!(output.snapshot(&scope).is_err());
    host.close().await;
    client.close().await;

    let (client, host, _gate, _cancel, _grant) =
        support::channels(Arc::new(AtomicU64::new(1000))).await;
    assert!(ObserverOutput::new(host.authority(), &scope, 2, 8).is_err());
    host.close().await;
    client.close().await;
}
