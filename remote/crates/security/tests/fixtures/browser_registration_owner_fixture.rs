#![no_main]
// 只使生产入口进入链接；握手future/IO/取消/轮询全部由生产模块拥有。
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_start(setup: u32, total: u32) -> i32 {
    cc_desk_remote_security::browser_registration_start(setup, total)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_poll() {
    cc_desk_remote_security::browser_registration_poll();
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_cancel() {
    cc_desk_remote_security::browser_registration_cancel();
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_event(kind: u32) {
    cc_desk_remote_security::browser_registration_event(kind);
}
