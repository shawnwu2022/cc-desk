#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn browser_client_start(l: u32, p: u32, b: u32) -> i32 {
    cc_desk_remote_client::browser::browser_client_start(l, p, b)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_client_event(k: u32) {
    cc_desk_remote_client::browser::browser_client_event(k)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_start(l: u32, p: u32, b: u32) -> i32 {
    cc_desk_remote_security::browser_tls_start(l, p, b)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_event(k: u32) {
    cc_desk_remote_security::browser_tls_event(k)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_poll() {
    cc_desk_remote_security::browser_tls_poll()
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_cancel() {
    cc_desk_remote_security::browser_tls_cancel()
}
#[unsafe(no_mangle)]
pub extern "C" fn TEST_ONLY_uncertain_start(l: u32, p: u32, b: u32) -> i32 {
    fn application(
        channel: cc_desk_remote_security::RunningChannel,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<(), cc_desk_remote_security::BrowserApplicationFailure>,
                >,
        >,
    > {
        Box::pin(async move {
            let proof = channel
                .client_installed()
                .await
                .map_err(cc_desk_remote_security::BrowserApplicationFailure::Operation)?;
            let client = cc_desk_remote_client::NativeClient::from_installed(channel, proof)
                .map_err(cc_desk_remote_security::BrowserApplicationFailure::Operation)?;
            client
                .retire()
                .await
                .map_err(cc_desk_remote_security::BrowserApplicationFailure::Retirement)?;
            // 仅分类注入，不能声称发生了真实join故障。
            Err(
                cc_desk_remote_security::BrowserApplicationFailure::Retirement(
                    cc_desk_remote_security::DataError::Uncertain,
                ),
            )
        })
    }
    cc_desk_remote_security::browser_application_start(l, p, b, application)
}
