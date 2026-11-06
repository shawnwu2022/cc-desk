#![no_main]
// 只使生产入口进入链接；握手future/IO/取消/轮询全部由生产模块拥有。
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_start(local: u32, peer: u32, budget: u32) -> i32 {
    cc_desk_remote_security::browser_tls_start(local, peer, budget)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_poll() {
    cc_desk_remote_security::browser_tls_poll();
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_cancel() {
    cc_desk_remote_security::browser_tls_cancel();
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_tls_event(kind: u32) {
    cc_desk_remote_security::browser_tls_event(kind);
}
