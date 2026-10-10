#[test]
fn updater_transport_urls_never_enter_formatted_logs() {
    for target in ["reqwest", "reqwest::async_impl::client"] {
        let record = log::Record::builder()
            .target(target)
            .level(log::Level::Warn)
            .args(format_args!("redirecting https://release-assets.githubusercontent.com/file?TOKEN=SECRET using http://USER:PASSWORD@proxy"))
            .build();
        let line = super::format_line(&record);
        assert!(line.contains("upstream detail redacted; see update_diag"));
        assert!(!line.contains("TOKEN"));
        assert!(!line.contains("SECRET"));
        assert!(!line.contains("USER"));
        assert!(!line.contains("PASSWORD"));
        assert!(!line.contains("https://"));
    }
}

#[test]
fn upstream_update_strings_never_enter_any_formatted_log() {
    let record = log::Record::builder()
        .target("tauri_plugin_updater::updater")
        .level(log::Level::Error)
        .args(format_args!(
            "failed to deserialize PRIVATE_PATH TOKEN=SECRET"
        ))
        .build();
    let line = super::format_line(&record);
    assert!(line.contains("upstream detail redacted; see update_diag"));
    assert!(!line.contains("PRIVATE_PATH"));
    assert!(!line.contains("TOKEN"));
    assert!(!line.contains("SECRET"));
    let diagnostic = log::Record::builder()
        .target("cc_desk::desktop_updater")
        .level(log::Level::Warn)
        .args(format_args!(
            "update_diag code=UPDATER_MANIFEST_INVALID stage=check"
        ))
        .build();
    assert!(super::format_line(&diagnostic)
        .contains("update_diag code=UPDATER_MANIFEST_INVALID stage=check"));
}
