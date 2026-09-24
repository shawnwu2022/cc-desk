use crate::cli::commands::{cli_ack_output, cli_stop};

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
