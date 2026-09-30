#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

unsafe extern "C" {
    fn clock_gettime_nsec_np(clock_id: libc::clockid_t) -> u64;
}

/// @return whether screen recording is already granted; otherwise triggers the system prompt
pub fn ensure_screen_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() }
}

/// @return nanoseconds on the host clock shared by ScreenCaptureKit, AVFoundation and Core Audio
pub fn host_now_ns() -> u64 {
    unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) }
}
