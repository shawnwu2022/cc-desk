use crate::cli::commands::{
    cli_ack_output, cli_input_abort, cli_input_begin, cli_input_chunk, cli_input_commit,
    cli_input_protocol, cli_stop,
};

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
