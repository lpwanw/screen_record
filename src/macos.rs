use std::{ffi::c_void, ptr::NonNull};

use block2::RcBlock;
use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyObject, Bool},
};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureDevice, AVMediaType, AVMediaTypeAudio, AVMediaTypeVideo,
};
use objc2_foundation::NSRect;

use crate::project::Rect;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> NSRect;
}

unsafe extern "C" {
    fn clock_gettime_nsec_np(clock_id: libc::clockid_t) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Media {
    Camera,
    Mic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Granted,
    Denied,
    Undetermined,
}

pub fn screen_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Shows the system prompt once per app; a grant takes effect after the app relaunches.
pub fn request_screen_access() -> bool {
    unsafe { CGRequestScreenCaptureAccess() }
}

fn media_type(media: Media) -> &'static AVMediaType {
    let media_type = match media {
        Media::Camera => unsafe { AVMediaTypeVideo },
        Media::Mic => unsafe { AVMediaTypeAudio },
    };
    media_type.expect("AVFoundation media type constants exist")
}

pub fn av_access(media: Media) -> Access {
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type(media)) } {
        AVAuthorizationStatus::Authorized => Access::Granted,
        AVAuthorizationStatus::NotDetermined => Access::Undetermined,
        _ => Access::Denied,
    }
}

/// Prompts when undetermined; `done` runs on an arbitrary dispatch queue.
pub fn request_av(media: Media, done: impl Fn(bool) + Send + 'static) {
    let handler = RcBlock::new(move |granted: Bool| done(granted.as_bool()));
    unsafe {
        AVCaptureDevice::requestAccessForMediaType_completionHandler(media_type(media), &handler)
    };
}

/// @param ns_view the AppKit view backing a window
/// @return the window frame in global points with a top-left origin, as ScreenCaptureKit reports
pub fn window_frame(ns_view: NonNull<c_void>) -> Option<Rect> {
    let view = unsafe { ns_view.cast::<AnyObject>().as_ref() };
    let window: Option<Retained<AnyObject>> = unsafe { msg_send![view, window] };
    let frame: NSRect = unsafe { msg_send![&*window?, frame] };
    let main_height = unsafe { CGDisplayBounds(CGMainDisplayID()) }.size.height;
    Some(Rect {
        x: frame.origin.x,
        y: main_height - frame.origin.y - frame.size.height,
        w: frame.size.width,
        h: frame.size.height,
    })
}

/// @return nanoseconds on the host clock shared by ScreenCaptureKit, AVFoundation and Core Audio
pub fn host_now_ns() -> u64 {
    unsafe { clock_gettime_nsec_np(libc::CLOCK_UPTIME_RAW) }
}

/// @return a CMTime on the host clock in nanoseconds, `None` when invalid or negative
pub fn cmtime_ns(value: i64, timescale: i32) -> Option<u64> {
    (timescale > 0 && value >= 0)
        .then(|| (value as i128 * 1_000_000_000 / timescale as i128) as u64)
}
