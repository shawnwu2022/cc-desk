#[cfg(windows)]
#[test]
#[allow(non_snake_case)]
fn CodexReporter_WindowsActualWrapperAndDirectRoutes_001() {
    d13_observer_core::reporter_fixture::verify(std::path::Path::new(env!(
        "CARGO_BIN_EXE_codex-observer-reporter-fixture"
    )));
}
