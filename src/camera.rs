use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context, anyhow};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::{
    AnyThread, DefinedClass, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject},
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDeviceInput,
    AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeContinuityCamera, AVCaptureDeviceTypeExternal, AVCaptureFileOutput,
    AVCaptureFileOutputRecordingDelegate, AVCaptureMovieFileOutput, AVCaptureOutput,
    AVCaptureSession, AVCaptureSessionPreset1280x720, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVErrorRecordingSuccessfullyFinishedKey,
    AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress, kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL,
};

use crate::macos::{self, Access, Media};

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Latest-frame handoff between a capture thread and the bubble; older frames are overwritten.
pub type FrameSlot = Arc<Mutex<Option<Frame>>>;

struct Ivars {
    slot: FrameSlot,
    start_host_ns: Arc<AtomicU64>,
    finished: Mutex<Option<mpsc::Sender<Result<(), String>>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "TaywindCameraDelegate"]
    #[ivars = Ivars]
    struct CameraDelegate;

    unsafe impl NSObjectProtocol for CameraDelegate {}

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_output(
            &self,
            _output: &AVCaptureOutput,
            sample: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            if let Some(frame) = square_frame(sample) {
                *self.ivars().slot.lock().unwrap() = Some(frame);
            }
        }
    }

    unsafe impl AVCaptureFileOutputRecordingDelegate for CameraDelegate {
        #[unsafe(method(captureOutput:didStartRecordingToOutputFileAtURL:startPTS:fromConnections:))]
        fn did_start(
            &self,
            _output: &AVCaptureFileOutput,
            _url: &NSURL,
            start_pts: CMTime,
            _connections: &NSArray<AVCaptureConnection>,
        ) {
            if let Some(ns) = macos::cmtime_ns(start_pts.value, start_pts.timescale) {
                self.ivars().start_host_ns.store(ns, Ordering::Release);
            }
        }

        #[unsafe(method(captureOutput:didFinishRecordingToOutputFileAtURL:fromConnections:error:))]
        fn did_finish(
            &self,
            _output: &AVCaptureFileOutput,
            _url: &NSURL,
            _connections: &NSArray<AVCaptureConnection>,
            error: Option<&NSError>,
        ) {
            let result = match error {
                Some(error) if !finished_successfully(error) => {
                    Err(error.localizedDescription().to_string())
                }
                _ => Ok(()),
            };
            if let Some(finished) = self.ivars().finished.lock().unwrap().take() {
                finished.send(result).ok();
            }
        }
    }
);

impl CameraDelegate {
    fn new(ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }
}

pub struct Camera {
    session: Retained<AVCaptureSession>,
    movie: Retained<AVCaptureMovieFileOutput>,
    frames: Retained<AVCaptureVideoDataOutput>,
    delegate: Retained<CameraDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
}

/// A camera file being written; `wait` blocks, so call it off the main thread.
pub struct CameraRecording {
    finished: mpsc::Receiver<Result<(), String>>,
    start_host_ns: Arc<AtomicU64>,
}

/// @return built-in, external and Continuity cameras
pub fn devices() -> Vec<Device> {
    discover()
        .iter()
        .map(|device| unsafe {
            Device {
                id: device.uniqueID().to_string(),
                name: device.localizedName().to_string(),
            }
        })
        .collect()
}

/// Starts the preview feeding `slot`; requires camera permission.
pub fn open(id: &str, slot: FrameSlot) -> anyhow::Result<Camera> {
    anyhow::ensure!(
        macos::av_access(Media::Camera) == Access::Granted,
        "camera permission not granted"
    );
    let device = unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(id)) }
        .context("camera disconnected")?;

    let delegate = CameraDelegate::new(Ivars {
        slot,
        start_host_ns: Arc::default(),
        finished: Mutex::new(None),
    });
    let queue = DispatchQueue::new("dev.taywind.camera", None);
    let session = unsafe { AVCaptureSession::new() };
    let movie = unsafe { AVCaptureMovieFileOutput::new() };
    let frames = unsafe { AVCaptureVideoDataOutput::new() };
    unsafe {
        let input = AVCaptureDeviceInput::deviceInputWithDevice_error(&device)
            .map_err(|error| anyhow!("camera input: {}", error.localizedDescription()))?;
        let key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
        let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
        let value: &AnyObject = &format;
        frames.setVideoSettings(Some(&NSDictionary::from_slices(&[key], &[value])));
        frames.setAlwaysDiscardsLateVideoFrames(true);
        frames.setSampleBufferDelegate_queue(
            Some(ProtocolObject::from_ref(&*delegate)),
            Some(&queue),
        );

        session.beginConfiguration();
        session.setSessionPreset(AVCaptureSessionPreset1280x720);
        anyhow::ensure!(session.canAddInput(&input), "camera input rejected");
        session.addInput(&input);
        anyhow::ensure!(
            session.canAddOutput(&frames),
            "camera preview output rejected"
        );
        session.addOutput(&frames);
        anyhow::ensure!(session.canAddOutput(&movie), "camera file output rejected");
        session.addOutput(&movie);
        session.commitConfiguration();
        session.startRunning();
    }
    Ok(Camera {
        session,
        movie,
        frames,
        delegate,
        _queue: queue,
    })
}

fn discover() -> Retained<NSArray<AVCaptureDevice>> {
    unsafe {
        let types = NSArray::from_slice(&[
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal,
            AVCaptureDeviceTypeContinuityCamera,
        ]);
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &types,
            AVMediaTypeVideo,
            AVCaptureDevicePosition::Unspecified,
        )
        .devices()
    }
}

impl Camera {
    pub fn start_recording(&self, path: &Path) -> CameraRecording {
        let ivars = self.delegate.ivars();
        let (done, finished) = mpsc::channel();
        *ivars.finished.lock().unwrap() = Some(done);
        ivars.start_host_ns.store(0, Ordering::Release);
        let url = NSURL::from_file_path(path).expect("absolute recording path");
        unsafe {
            self.movie.startRecordingToOutputFileURL_recordingDelegate(
                &url,
                ProtocolObject::from_ref(&*self.delegate),
            )
        };
        CameraRecording {
            finished,
            start_host_ns: ivars.start_host_ns.clone(),
        }
    }

    pub fn stop_recording(&self) {
        unsafe { self.movie.stopRecording() };
    }
}

impl CameraRecording {
    /// @return host time (ns) of the first frame written to the file
    pub fn wait(self) -> anyhow::Result<u64> {
        self.finished
            .recv_timeout(Duration::from_secs(10))
            .context("camera recording did not finish")?
            .map_err(|error| anyhow!("camera recording failed: {error}"))?;
        let start = self.start_host_ns.load(Ordering::Acquire);
        anyhow::ensure!(start > 0, "camera recording reported no start time");
        Ok(start)
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        unsafe {
            self.frames.setSampleBufferDelegate_queue(None, None);
            self.movie.stopRecording();
            self.session.stopRunning();
        }
    }
}

fn finished_successfully(error: &NSError) -> bool {
    let Some(key) = (unsafe { AVErrorRecordingSuccessfullyFinishedKey }) else {
        return false;
    };
    error
        .userInfo()
        .objectForKey(key)
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .is_some_and(|flag| flag.boolValue())
}

fn square_frame(sample: &CMSampleBuffer) -> Option<Frame> {
    let pixels = unsafe { sample.image_buffer() }?;
    if unsafe { CVPixelBufferLockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) } != 0 {
        return None;
    }
    let (width, height) = (
        CVPixelBufferGetWidth(&pixels),
        CVPixelBufferGetHeight(&pixels),
    );
    let stride = CVPixelBufferGetBytesPerRow(&pixels);
    let base = CVPixelBufferGetBaseAddress(&pixels) as *const u8;
    let side = width.min(height);
    let (left, top) = ((width - side) / 2, (height - side) / 2);
    let mut bgra = Vec::with_capacity(side * side * 4);
    if !base.is_null() {
        for row in top..top + side {
            let start = row * stride + left * 4;
            bgra.extend_from_slice(unsafe {
                std::slice::from_raw_parts(base.add(start), side * 4)
            });
        }
    }
    unsafe { CVPixelBufferUnlockBaseAddress(&pixels, CVPixelBufferLockFlags::ReadOnly) };
    (bgra.len() == side * side * 4).then_some(Frame {
        width: side as u32,
        height: side as u32,
        bgra,
    })
}
