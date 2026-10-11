use crate::cli::commands::{
    cli_ack_output, cli_input_abort, cli_input_begin, cli_input_chunk, cli_input_commit,
    cli_input_protocol, cli_stop,
};
use crate::cli::native_runtime::decode_stop_query;
use tauri::ipc::InvokeBody;

#[test]
fn D14_Ack_FormalCommandIsRegisteredAtTheNativeBoundary_001() {
    let _ = cli_ack_output;
    assert!(
        include_str!("../lib.rs").contains("cli::commands::cli_ack_output,"),
        "formal ACK command is missing from the application invoke handler"
    );
}

#[test]
fn D15_Stop_FormalCommandIsRegisteredAtTheNativeBoundary_002() {
    let _ = cli_stop;
    assert!(
        include_str!("../lib.rs").contains("cli::commands::cli_stop,"),
        "formal stop command is missing from the application invoke handler"
    );
}

#[test]
fn D17_Input_FormalCommandsAreRegisteredAtTheNativeBoundary_003() {
    let _ = (
        cli_input_begin,
        cli_input_chunk,
        cli_input_commit,
        cli_input_abort,
        cli_input_protocol,
    );
    let source = include_str!("../lib.rs");
    for command in [
        "cli::commands::cli_input_begin,",
        "cli::commands::cli_input_chunk,",
        "cli::commands::cli_input_commit,",
        "cli::commands::cli_input_abort,",
        "cli::commands::cli_input_protocol,",
    ] {
        assert!(
            source.contains(command),
            "missing formal input command: {command}"
        );
    }
}

// stop 的真实 wire 解码必须携带 requestId，并继续拒绝未知字段。
#[test]
fn D15_StopWire_RequiresRequestId_004() {
    let (request_id, run) = decode_stop_query(&InvokeBody::Raw(
        br#"{"requestId":"request-a","runId":"run-a","generation":2}"#.to_vec(),
    ))
    .unwrap();
    assert_eq!(request_id, "request-a");
    assert_eq!(run.run_id, "run-a");
    assert_eq!(run.generation, 2);

    for body in [
        br#"{"runId":"run-a","generation":2}"#.as_slice(),
        br#"{"requestId":"request-a","runId":"run-a","generation":2,"extra":true}"#.as_slice(),
    ] {
        assert_eq!(
            decode_stop_query(&InvokeBody::Raw(body.to_vec()))
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );
    }
}
