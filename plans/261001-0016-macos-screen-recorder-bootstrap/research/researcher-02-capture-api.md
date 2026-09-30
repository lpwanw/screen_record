# Capture crates exact API (saved by orchestrator from researcher hand-back)

[V] verified from fetched source; [U] unverified (memory/inference) — confirm via `cargo doc`/source before coding.

## 1. screencapturekit 11.0.0
- Cargo [V]: `screencapturekit = { version = "11", features = ["macos_15_0"] }` (`async` optional; macos_* cumulative).
- Build needs Xcode CLT (Swift bridge) [V]; README lists `NSScreenCaptureUsageDescription` [V].
```rust
let content = SCShareableContent::get()?;          // [V]
let display = &content.displays()[0];              // [V]
let windows = content.windows();                   // [U]
// SCWindow [U]: window_id() u32, title() Option<String>, owning_application()->application_name(), frame(), is_on_screen(), window_layer()
// SCDisplay [U]: display_id(), width(), height(), frame()
let f = SCContentFilter::create().with_display(display).with_excluding_windows(&[]).build()?; // [V shape]
let f = SCContentFilter::create().with_desktop_independent_window(&window).build()?;         // [U args]
let cfg = SCStreamConfiguration::new()
    .with_width(1920).with_height(1080).with_captures_audio(true)
    .with_shows_cursor(true).with_frame_rate(30);  // [V]
// .with_source_rect(CGRect) name [V], owner (config vs filter) [U], unit display points [U]
use screencapturekit::recording_output::{SCRecordingOutput, SCRecordingOutputCodec,
    SCRecordingOutputConfiguration, SCRecordingOutputFileType};
let rc = SCRecordingOutputConfiguration::new()?     // Err(FeatureNotAvailable) < macOS 15
    .with_output_url(&PathBuf::from("/x/out.mp4"))?
    .with_video_codec(SCRecordingOutputCodec::H264)
    .with_output_file_type(SCRecordingOutputFileType::MP4);
let ro = SCRecordingOutput::new(&rc);               // [V] Option<SCRecordingOutput>
// stats [V]: ro.recorded_duration(), ro.recorded_file_size()
let stream = SCStream::new(&filter, &cfg);          // [U] wiring not seen in example
stream.add_recording_output(&ro)?; stream.start_capture()?; stream.stop_capture()?; // [U]
```
- Finish: Apple `SCRecordingOutputDelegate.recordingOutputDidFinishRecording`; crate delegate trait [U]. Fallback: after stop, poll until file size stable.
- point_pixel_scale [U]: `SCShareableContent::info_for_filter(&filter).point_pixel_scale()`; fallback NSScreen backingScaleFactor.

## 2. objc2-av-foundation 0.3.2 (camera) — mostly [U]
- Deps: objc2 0.6, objc2-foundation, objc2-av-foundation, objc2-core-media, objc2-core-video, block2, dispatch2. Per-class features (AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureSession, AVCaptureInput, AVCaptureOutput, AVCaptureFileOutput, AVCaptureVideoDataOutput, AVMediaFormat...). Dev shortcut: `features=["all"]`, trim later.
- Discovery [V sig]: `discoverySessionWithDeviceTypes_mediaType_position(&NSArray<AVCaptureDeviceType>, Option<&AVMediaType>, AVCaptureDevicePosition)` → `.devices()`. Types [U]: BuiltInWideAngleCamera, External (14+), ContinuityCamera (13+), DeskViewCamera.
- Pipeline [U]: `AVCaptureSession::new()`, `AVCaptureDeviceInput::deviceInputWithDevice_error`, `canAddInput/addInput`, `AVCaptureMovieFileOutput::new()`, `startRecordingToOutputFileURL_recordingDelegate`, `AVCaptureVideoDataOutput` + `setVideoSettings` (32BGRA) + `setSampleBufferDelegate_queue` with `dispatch2` serial queue.
```rust
define_class!(
  #[unsafe(super(NSObject))] #[thread_kind = AnyThread] #[name="CamDelegate"]
  struct CamDelegate;
  unsafe impl NSObjectProtocol for CamDelegate {}
  unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CamDelegate {
    #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
    fn did_output(&self,_o:&AVCaptureOutput,sb:&CMSampleBuffer,_c:&AVCaptureConnection){}
  }
);
```
- BGRA [U]: `sb.image_buffer()` → CVPixelBufferLockBaseAddress(ReadOnly) → GetBaseAddress/BytesPerRow/Width/Height → copy rows (stride) → Unlock. Send via mpsc; no heavy work in callback.
- Auth [U]: `AVCaptureDevice::authorizationStatusForMediaType(AVMediaTypeVideo)`, `requestAccessForMediaType_completionHandler(.., &RcBlock::new(|granted: Bool| ..))`; same with AVMediaTypeAudio. Requires bundle + usage string or process killed.

## 3. cpal 0.18 + hound
- 0.18 changes [V CHANGELOG]: StreamConfig by value; streams don't auto-start → `stream.play()`; SampleRate = u32; unified Error/ErrorKind; CallbackInfo merged; DeviceTrait: Display+Eq+Hash. Confirm 0.18.x published (`cargo add cpal`) [U].
```rust
let dev = cpal::default_host().default_input_device().ok_or(..)?;
let cfg = dev.default_input_config()?;  // sample_format may be I16/U16/F32 — match
let stream = dev.build_input_stream(cfg.config(), move |d:&[f32], _:&cpal::CallbackInfo| { tx.send(d.to_vec()).ok(); },
    move |e| eprintln!("{e}"), None)?;   // [U] exact generics
stream.play()?;
let spec = hound::WavSpec{channels:c,sample_rate:sr,bits_per_sample:32,sample_format:hound::SampleFormat::Float};
let mut w = hound::WavWriter::create(path, spec)?; w.write_sample(x)?; w.finalize()?; // writer thread, not callback
```

## 4. windowNumber + screen permission
```rust
let view: &NSView = unsafe { &*(ns_view_ptr as *const NSView) }; // from AppKitWindowHandle.ns_view [U]
let n: isize = view.window().map(|w| w.windowNumber()).unwrap_or(-1); // main thread [U]
#[link(name="CoreGraphics",kind="framework")]
extern "C" { fn CGPreflightScreenCaptureAccess()->bool; fn CGRequestScreenCaptureAccess()->bool; }
```
- windowNumber == SCWindow::window_id. Request prompts once; grant effective after relaunch.

## 5. .app bundling (dev loop)
- Recommend ~15-line script (bun/shell) > cargo-packager (release later) > cargo-bundle (aging).
- Script: mkdir `Recorder.app/Contents/MacOS`, copy binary, write Info.plist (CFBundleIdentifier `dev.taywind.record`, CFBundleExecutable, CFBundlePackageType APPL, LSMinimumSystemVersion 15.0, NSCameraUsageDescription, NSMicrophoneUsageDescription, NSScreenCaptureUsageDescription, NSHighResolutionCapable), `codesign --force --sign "TaywindDev"`. Launch via `open` (direct exec attributes TCC to terminal).
- Stable self-signed identity: Keychain Access → Certificate Assistant (Code Signing, self-signed) or openssl req (codeSigning EKU) → pkcs12 → `security import ... -T /usr/bin/codesign`; `security find-identity -v -p codesigning`. Ad-hoc signing changes each build → TCC grant lost.

## Top risks
1. SCStream + add_recording_output + finish-await unverified → read crate `src/stream/` + examples first.
2. objc2-av-foundation feature list / define_class! syntax from memory → cargo check.
3. cpal 0.18 exact callback types.
4. screencapturekit MSRV vs toolchain.

## Unresolved
- SCStream wiring/delegate/finish signatures? source_rect owner + unit?
- point_pixel_scale path? objc2-core-graphics exposes CGPreflight*?
- Self-signed cert TCC persistence on macOS 27?
