# macOS capture stack (Rust) — 2026-10-01

## Recommendation (one stack)
- Min macOS **15.0** (SCRecordingOutput + SCStream mic both 15.0).
- Screen + system audio: `screencapturekit` **11.0.0** (feature `macos_15_0`) -> SCRecordingOutput -> `display.mp4/.mov`. Zero manual encoding.
- Mic: `cpal` **0.18.2** input stream -> `hound` WAV writer + same callback computes level/waveform. ~40 LOC.
- Camera: `objc2-av-foundation` **0.3.2** (AVCaptureSession + AVCaptureMovieFileOutput -> `camera.mov`, + AVCaptureVideoDataOutput for 30fps preview). SMOKE TEST FIRST (see Q4); fallback VideoDataOutput -> AVAssetWriter.
- Do NOT set captureMicrophone on SCStream. Mic recorded by cpal, not SCK.
- Permissions: ship as .app bundle from day 1.

## Q1 Bindings (verified crates.io API + source, 2026-10-01)
| crate | ver / last release | verdict |
|---|---|---|
| screencapturekit (doom-fish) | 11.0.0, 2026-09-24; 2.27M dl; MIT/Apache; 709+ commits | BEST. Active. Safe API, Swift bridge (needs Xcode CLT at build) |
| cidre (yury) | 0.29.0 crates.io 2026-09-10; git 0.30.0 commit 2026-09-30; 259k dl | active, broad (sc, av, cm, vt). Has sc::RecordingOutput, AV capture Video/AudioDataOutput. NO MovieFileOutput/AudioFileOutput in src (grep). Unsafe-ish, sparse docs |
| objc2-screen-capture-kit | 0.3.2, 2025-10-04 | raw generated bindings, incl SCRecordingOutput, SCContentSharingPicker. Verbose unsafe; no 2026 release |
| scap | 0.1.0-beta.1, 2025-08-04 | stale, high-level, no recording output/mic/exclude-window control. REJECT |

screencapturekit 11 exposes (read source): SCShareableContent, `SCContentFilter::create().with_display(..).with_excluding_windows(..)` / `.with_window(..)`, `with_source_rect(CGRect)`, `with_captures_audio`, `with_captures_microphone` (macOS 15, returns SCResult), `with_microphone_capture_device_id`, `with_shows_cursor`, `point_pixel_scale()`, `SCRecordingOutput{,Configuration}` with codec H264/HEVC/ProRes and file type MP4/MOV (`examples/10_recording_output.rs`), `SCContentSharingPicker`, async feature. Cap (prior art) used older crate 0.3.5 + own cidre fork.

## Q2 SCRecordingOutput audio
- Apple gives no explicit doc of track layout. Evidence: Apple DTS reply (Feb/Mar 2026) on [forum 805892](https://developer.apple.com/forums/thread/805892): with captureMicrophone=true, SCK delivers separate `.audio` and `.microphone` buffers with different format/clock; SCRecordingOutput file reported CORRUPT/unplayable in that case. => Avoid mic on SCRecordingOutput. UNVERIFIED whether fixed in macOS 26/27 — forum thread only; not tested on this box.
- captureAudio only (system audio) -> audio track inside the screen file. Fine = separate track from mic. Split to standalone file at export: `ffmpeg -i display.mp4 -vn -c copy sys.m4a`.
- Mic options ranked (least code): (1) cpal+hound WAV: live samples free, timestamps available (`StreamInstant` -> host time); (2) AVAudioRecorder (objc2-avf-audio) `meteringEnabled` + poll levels: ~same LOC, no raw samples, poor start timestamp; (3) AVCaptureAudioFileOutput + AudioDataOutput: more ObjC delegate code, and unsure it coexists. Pick (1). Caveat: cpal default device + sample-rate/format conversion (f32/i16) — write whatever format cpal yields; WAV 32-bit float ok.

## Q3 Fallback if SCRecordingOutput bad
Rank by code size: (1) SCStream buffers -> AVAssetWriter via cidre (Cap's `enc-avfoundation/mp4.rs` is 4.5k LOC but mostly pause/drift/edge cases; bare minimum ~150-250 LOC: AssetWriter + 2 inputs, startSession at first PTS); (2) ffmpeg pipe (raw BGRA over stdin, ~80 LOC but CPU copy, extra runtime dep, HW encode via h264_videotoolbox); (3) VideoToolbox VTCompressionSession direct + manual mux: largest, skip. Also fallback gives exact first-PTS for sync.

## Q4 Camera
- nokhwa 0.10.11 (2026-05-15): frames only, no file recording, decode to RGB on CPU, AVFoundation backend; good for preview only. REJECT as sole option (recording = write encoder yourself).
- cidre: has VideoDataOutput/AudioDataOutput/Session/Device; no MovieFileOutput.
- objc2-av-foundation 0.3.2 (1.8M dl): has AVCaptureMovieFileOutput, AVCaptureAudioFileOutput, AVCaptureDeviceDiscoverySession (verified docs.rs). Needs `define_class!` delegates (AVCaptureFileOutputRecordingDelegate, SampleBufferDelegate) ~80-120 LOC.
- MovieFileOutput + VideoDataOutput same session: CONFLICTING/UNVERIFIED on macOS. Apple forum ([98113](https://developer.apple.com/forums/thread/98113), iOS, 2018) says no / only one connection active; search summary says restriction lifted iOS 16 ([thread 807734](https://developer.apple.com/forums/thread/807734) relates). No macOS-specific doc found. ACTION: 20-line smoke test with real camera before committing. If fails: VideoDataOutput only -> frames to bubble AND AVAssetWriter (Cap's approach: camera.mp4 via VideoDataOutput + own encoder).
- Enumeration / no-camera: `AVCaptureDeviceDiscoverySession` (types BuiltInWideAngle, External, Continuity) `.devices()` empty => show mic waveform. Also handle hot-plug (AVCaptureDeviceWasConnected notification) — optional. Dev box Mac mini likely has no camera: test the no-camera path natively, camera path needs external/Continuity cam.

## Q5 Mic level + record
cpal input callback gets `&[f32]`: write to hound, compute RMS/peak per chunk, send over channel/atomic to GPUI. One stream does both. ~40 LOC. AVCaptureAudioDataOutput is the AVFoundation equivalent but needs ObjC delegate + writer. cpal wins.

## Q6 Sync
- SCK: `CMSampleBuffer` PTS is on host time clock (mach_absolute_time domain). AVCapture on macOS also host-clock PTS. cpal CoreAudio `StreamInstant` -> host time (Cap `timestamp/macos.rs` converts via `StreamInstantExt`).
- PROBLEM: file outputs (SCRecordingOutput, MovieFileOutput) start their files at PTS 0; absolute first-frame host time is not exposed. Mitigation: record `mach_absolute_time()` immediately before each start(); ALSO attach a cheap SCStreamOutput (screen type) to capture first-frame PTS (and cpal gives mic's exactly). Store `start_host_ns` per track in a `meta.json` next to files. Export offsets tracks by delta.
- Start latency differs per track (SCK ~100-300ms? camera ~200-500ms?) — my estimate, not sourced; calibrate. Cap ships `sync_calibration.rs` and `MAX_AV_DRIFT_SECS = 2.0` guard: drift handling is a real concern there. Typical long-run drift independent-clock audio vs video: no authoritative figure found; mic (hardware clock) vs display clock can diverge ms/min; plan export to resample/stretch if |drift|>~40ms (assumption).
- Fallback cheap: start all tracks, flash/clap marker. Skip unless drift measured.

## Q7 Window / region
(Apple SCK behaviour from docs/experience; not re-tested here.)
- `SCContentFilter(desktopIndependentWindow:)` captures that window only, even occluded/behind others; follows moves; resize changes content size -> must `updateConfiguration` (width/height) or frames scale/letterbox; minimized/offscreen => frames idle/stop (status `.idle`). Use `SCShareableContent.current` + window list (filter `isOnScreen`, layer 0, title non-empty).
- Region: display filter + `sourceRect` (display-local POINTS, top-left origin, Quartz coords) + output `width/height` = rect_pts * `pointPixelScale` (2.0 on Retina; `filter.point_pixel_scale()`), even numbers for H.264.
- Cursor: `shows_cursor(true)` default; cursor overlays in pixels.
- Bubble exclusion: `SCContentFilter(display:, excludingWindows:[bubble])`; find bubble's `SCWindow` by `window_id == NSWindow.windowNumber` (GPUI: get native handle). Re-acquire list after window shown. Do not rely on NSWindow.sharingType=.none (recollection: ignored by SCK on macOS 15+; unverified here). Window mode: bubble naturally excluded.
- Lightweight alternative UI: `SCContentSharingPicker` (macOS 14+) avoids monthly prompt — but custom drag-region UI requires own picker; picker gives no region.

## Q8 Permissions (TCC)
- Screen: `CGPreflightScreenCaptureAccess()` / `CGRequestScreenCaptureAccess()` (Cap `scap-screencapturekit/permission.rs`). macOS 15+: re-prompt about monthly ([TidBITS 2024-08-19](https://tidbits.com/2024/08/19/apple-reduces-excessive-sequoia-permission-requests-shifts-to-monthly)); prompt text says "bypass the system private window picker". Only SCContentSharingPicker avoids it.
- Camera/Mic: `AVCaptureDevice.authorizationStatus(for:)` / `requestAccess(for:)`; cpal triggers mic prompt implicitly on first stream — request explicitly first.
- Unbundled `cargo run`: TCC attributes to responsible process (Terminal/iTerm/IDE), needs that app granted; CLI not shown reliably. Tahoe 26.1: plain executables do NOT appear in Settings > Screen & System Audio Recording ([forum 807898](https://developer.apple.com/forums/thread/807898), [807323](https://developer.apple.com/forums/thread/807323)); permissions also reported to vanish ([818415](https://developer.apple.com/forums/thread/818415)). => dev loop: bundle.
- Bundle: Info.plist `NSCameraUsageDescription`, `NSMicrophoneUsageDescription`, (`NSScreenCaptureUsageDescription` per screencapturekit README), `LSUIElement` optional. Sign with a STABLE identity (Apple Dev cert or self-signed "taywind-dev"), else TCC grants reset every rebuild with ad-hoc signing. No entitlements needed unsandboxed. `cargo-bundle` or ~15-line script (mkdir .app/Contents/MacOS, cp binary, plist, `codesign -s`).

## Q9 Cap prior art (github.com/CapSoftware/Cap, cloned 2026-09-30 HEAD)
- License: mixed — **AGPLv3** for most; `cap-camera*` and `scap-*` crates MIT (LICENSE). Cannot copy AGPL code into closed app; MIT crates copyable (scap-screencapturekit, scap-targets, cap-camera-avfoundation, camera-avfoundation).
- macOS stack: cidre (own fork, rev bf84b67) for SCK + AVFoundation + AssetWriter; `screencapturekit` 0.3.5 also dep; cpal for mic (`scap-cpal`), ffmpeg for resample. NOT SCRecordingOutput (grep: none) — own AVAssetWriter encoder (enc-avfoundation, 4.5k LOC) fed from SCStream buffers; camera via VideoDataOutput (camera-avfoundation, 335 LOC).
- Studio layout: `content/segments/segment-N/{display.mp4, camera.mp4, audio-input.m4a, system_audio...}` + cursors/ + `recording-meta.json`, separate pipelines per track, timestamps via `Timestamp::{MachAbsoluteTime,Instant,..}` (crate timestamp) aligned to shared start.
- Takeaway: Cap's complexity (segments, pause, fragments, drift heal, recovery) is out of scope; pattern (separate files + start timestamps in meta) is what we copy.

## Trade-off matrix
| | LOC | risk | fit |
|---|---|---|---|
| sck11+SCRecordingOutput + cpal + objc2-av camera (REC) | low | SCRecordingOutput opacity (no first-PTS), camera dual-output unverified | best |
| cidre all-in (SCStream -> AssetWriter, VideoDataOutput) | med-high | dep on git HEAD, no MovieFileOutput, sparse docs | strong control, more code |
| scap + nokhwa + ffmpeg | med | stale, no window-exclude precision | reject |

Adoption risk: screencapturekit single maintainer (doom-fish) but 2.27M dl, released 2026-09-24, major 11.0 (frequent breaking majors: pin version). objc2-av-foundation last 2025-10 but generated bindings stable; macOS 27 SDK additions irrelevant. cpal 0.18 (breaking minors: pin).

## Limitations
No hardware test of camera/SCRecordingOutput/mic (agent context, TCC-gated, no camera on dev box likely). Apple docs pages are JS-rendered, not readable; track layout and window-resize behaviour from memory/forums. Drift numbers not sourced.

## Unresolved
1. Does SCRecordingOutput + captureAudio (system only) yield a clean 1 audio track on macOS 15/26/27? Smoke test.
2. MovieFileOutput + VideoDataOutput same session on macOS 15+? Smoke test with external/Continuity cam.
3. SCRecordingOutput first-frame host time: obtainable without adding stream output?
4. Does SCK on macOS 27 still honor excludingWindows for GPUI/own-process window (and NSWindow.sharingType)?
5. Container: MOV vs MP4 for HEVC/AAC; export tool compat (ffmpeg fine).
6. Min macOS: 15.0 assumed — confirm user base.
7. Does export want mic as WAV f32 (big: ~11MB/min stereo 48k) or AAC? 
