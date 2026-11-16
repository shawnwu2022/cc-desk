//! 仅链接原始登记入口；状态机、协议、IO 和取消仍由 security crate 拥有。
#![no_main]
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_start(setup: u32, total: u32) -> i32 {
    cc_desk_remote_security::browser_registration_start(setup, total)
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_poll() {
    cc_desk_remote_security::browser_registration_poll()
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_cancel() {
    cc_desk_remote_security::browser_registration_cancel()
}
#[unsafe(no_mangle)]
pub extern "C" fn browser_registration_event(kind: u32) {
    cc_desk_remote_security::browser_registration_event(kind)
}
