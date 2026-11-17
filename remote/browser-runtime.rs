//! Link-only browser artifact. All state machines remain in the original crates.
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
