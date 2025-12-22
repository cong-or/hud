#![no_std]
#![no_main]

use aya_ebpf::{
    macros::tracepoint,
    programs::TracePointContext,
};

#[tracepoint]
pub fn runtime_scope(_ctx: TracePointContext) -> i64 {
    0
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe { core::hint::unreachable_unchecked() }
}
