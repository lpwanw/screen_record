---
title: "Phase 3: Capture backend"
status: implemented
effort: 15h
---

# Phase 3: Capture backend

## Context links
- Parent: [plan.md](./plan.md). Deps: phase 1 (spike verdicts b-f, first cut of `screen.rs`/`camera.rs`/`macos.rs`), phase 2 (`Project`, `normalize_bubble`, `FPS`, `export::tool`). Blocks: phase 4.
- Docs: `research/researcher-02-capture-api.md` (§1-4), `../reports/researcher-261001-0009-macos-capture-stack.md` (Q2, Q5-Q8).

## Overview
- Date: 2026-10-01. Priority: P2. Implementation: Done (hardware checks pending, Mac locked overnight). Review: Done (`reports/code-reviewer-261001-phase-03-04.md`, 0 critical/high).
- Grow spike modules into the real backend: screen in 3 modes, mic (cpal→hound + live level), camera recorder + preview, permission helpers, and `recording.rs` orchestrating start/stop of all tracks with host-clock start stamps → `project.json`. No GPUI in these files.

## Key Insights
- One clock domain: `host_now_ns()` = `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` = mach_absolute_time in ns = CMSampleBuffer host-clock PTS in ns. cpal CoreAudio `StreamInstant` is derived from `mHostTime` (same domain) [U: exact accessor].
- File outputs start at PTS 0 and don't expose absolute start (capture-stack Q6). Stamps: screen = first-frame PTS from a screen output handler; camera = `didStartRecordingTo:startPTS:` (macOS 15 delegate) [U in objc2 0.3.2] else host time at `didStartRecording`; mic = capture timestamp of first buffer written. Residual bias measured in phase 5.
- cpal `Stream` and `AVCaptureSession` may be `!Send` → `Mic`/`Camera` owned by UI on main thread; recording gets a Send `MicHandle` (channel + level atomic). Never block main waiting on AVF/SCK delegate callbacks (may be delivered on main → deadlock): stop issues commands, `Finishing::wait` runs on a background thread.
- Region `sourceRect` is display-local points, top-left; output px = rect·`point_pixel_scale`, even (capture-stack Q7). Owner (filter vs config) [U] → read crate `src/` first.
- Window mode captures only that window → bubble excluded naturally; resize mid-recording scales into fixed output (non-goal, documented).
- Spike branches: (d) no-audio-when-silent → ffprobe check at stop; (e) conflict → AVAssetWriter fallback (below).

## Requirements
- Screen: list displays + windows (on-screen, layer 0, titled, not own pid); `Target::{Display(id), Window(id), Region{display, rect_pt}}`; HEVC MP4 via SCRecordingOutput; `captures_audio` = toggle (never `captures_microphone`); cursor on; `FPS` (60); own app (bubble + panel) excluded via `with_excluding_applications`.
- Mic: list input devices; open by name or default; live peak level; WAV (32-bit float, native rate/channels) only between Start/Stop.
- Camera: list devices (BuiltInWideAngle, External, ContinuityCamera); open by uniqueID; preview frames to slot; record `camera.mov`.
- Permissions: screen preflight/request; camera + mic status/request (async callback).
- Recording: start all selected tracks, stop, wait all finishes (10s timeout each), write `project.json`; partial track failure → that track `None` + logged, other tracks kept.

## Architecture
```
UI (main thread) ──start(opts, Option<&MicHandle>, Option<&Camera>)──> recording::Session
  mic.start(dir/mic.wav) ; camera.start_recording(dir/camera.mov) ; screen::start(..) (last; screen = anchor)
UI ──session.stop(bubble_rect_pt)──> Finishing (Send: receivers + plain data)
background ──finishing.wait()──> ScreenResult{start_ns,duration,w,h} + cam start_ns + mic start_ns
  → Project{..., bubble: normalize_bubble(bubble_rect_pt, capture_rect_pt)} → dir/project.json → Ok(dir)
```
- `screen.rs`: `displays()`, `windows()`, `start(target, system_audio, path) -> ScreenRecording` (own app always excluded), `ScreenRecording::stop()` (main thread) + `wait()` (off-thread, Send). First-frame PTS into `Arc<AtomicU64>` (only first write kept). Capture rect (global pt) = display frame / window frame / display origin + region.
- `mic.rs`: `input_devices()`, `open(name) -> Mic{ _stream, handle }`, `MicHandle{ tx: Sender<Cmd>, level: Arc<AtomicU32> }` (Clone+Send). Callback: peak → level; `Cmd::Samples(Vec<f32>, capture_ns)` → writer thread. Writer: `Start(path)` opens `hound::WavWriter`, first buffer sets start_ns; `Stop(reply)` finalizes, replies `Option<u64>`. I16 input converted to f32; other formats → Err.
- `camera.rs`: `devices()`, `open(unique_id, slot) -> Camera`, `start_recording(path)`, `stop_recording() -> CameraFinish` (Send receiver). VideoDataOutput: 32BGRA, try width/height 640x360 [U], `alwaysDiscardsLateVideoFrames`, copy centered square (row stride aware, lock/unlock base address).
  - Fallback (spike e failed): drop MovieFileOutput; same delegate appends to `AVAssetWriter` (H.264, `startSessionAtSourceTime(first PTS)`, start_ns = first PTS), `finishWriting` on stop. +4h.
- `macos.rs`: add `cmtime_ns(value, timescale)`, `av_status(Media)`, `request_av(Media, FnOnce(bool)+Send)`.
- `recording.rs`: `Options{ target, system_audio }`, `start`, `Session::stop`, `Finishing::wait`. Dir `~/Movies/Taywind/<YYYY-MM-DD HH-MM-SS>/` (`$HOME`, `chrono::Local`). Start error → stop already-started tracks, Err. If spike (d) branch: `tool("ffprobe")` on screen.mp4, no audio stream → `system_audio=false`.

## Related code files
- Create: `src/mic.rs`, `src/recording.rs`.
- Modify: `src/screen.rs`, `src/camera.rs`, `src/macos.rs`, `src/main.rs` (mods), `src/app.rs` (only: Record/Stop buttons call `recording` with defaults — primary display, default mic, first camera, sys-audio switch), `Cargo.toml` (+`cpal = "0.18"`, `hound = "3"`, `chrono` clock-only; trim `objc2-av-foundation` features from `all`).

## Implementation Steps
1. Read crate sources in `~/.cargo/registry/src/*/screencapturekit-11*` for: stream output handler, recording delegate/finish, `source_rect` owner+unit, `point_pixel_scale` path. Resolve every [U] before coding.
2. `screen.rs`: listing, `Target`, filter per mode, config (even px dims, `with_captures_audio`, cursor, `FPS`), HEVC recording output, first-PTS handler, stop/finish.
3. `mic.rs` with writer thread + level.
4. `camera.rs`: device list, open by id, recording delegate (startPTS/didStart/didFinish → channel). Fallback branch if spike (e) failed.
5. `macos.rs` permission helpers + `cmtime_ns`.
6. `recording.rs` orchestration + `project.json` write; unwind on start failure; per-track timeouts.
7. Wire spike Record/Stop in `app.rs` to `recording`; run on `cx.background_spawn` for `wait`.
8. Manual check per mode (below); trim objc2 features; `cargo clippy`.

## Todo list
- [x] Resolve [U] APIs from crate sources
- [x] screen.rs 3 modes + listing + first-PTS (Complete frames only) + finish
- [x] mic.rs writer thread + level (unit-tested writer)
- [x] camera.rs device list + open by id (fallback not needed: spike e passed)
- [x] macos.rs permissions + cmtime_ns + bubble window frame
- [x] recording.rs start_screen (off main) / attach / stop / wait → project.json
- [x] app.rs Record/Stop wired (full panel, phase 4)
- [ ] Manual ffprobe checks pass for display/window/region (needs unlocked Mac)

## Success Criteria
- Per mode, 10s recording → dir has `screen.mp4` (hevc, px dims = rect·scale, even; +1 aac track iff sys audio on), `mic.wav`, `camera.mov` (if camera), valid `project.json` (`Project::load` ok, all `start_host_ns` within 1s of each other).
- Region: `screen.mp4` shows exactly the selected rect (checked with a known on-screen landmark).
- `ffmpeg -ss 2 -i screen.mp4 -frames:v 1` shows no bubble (display + region).
- Main thread never blocked > 500ms on start/stop (log timings).

## Risk Assessment
| Risk | L×I | Signal it broke | Response |
|---|---|---|---|
| `source_rect` unit/owner wrong | M×M | region video shows wrong area or 2x zoom | switch unit/owner per crate source (adjust) |
| SCK/AVF types `!Send` | H×L | "cannot be sent between threads" | keep on main, move only receivers to background (adjust) |
| SCK start/stop blocks main > 500ms | M×M | UI hitch in logs | dedicated recorder thread + command channel (adjust) |
| startPTS delegate absent in bindings | M×M | compile error / method never called | `didStartRecording` host time; calibrate in phase 5 (adjust) |
| cpal 0.18 timestamp accessor differs | M×L | compile error | `host_now_ns() - frames/rate` at callback (adjust) |
| Mic unplugged mid-recording | L×M | cpal error callback | writer finalizes partial WAV; track kept (built in) |
| Delegate on main + main waits → deadlock | M×H | app hangs on Stop | never wait on main (built in); if seen, audit call sites |
| Dual camera outputs fail at runtime despite spike | L×M | empty camera.mov | AVAssetWriter fallback (adjust, +4h) |

## Security Considerations
- Access only after TCC grant; denied camera/mic → track disabled, no crash (usage strings present via bundle).
- Recordings local (`~/Movies/Taywind`), user-readable perms default; no network.
- SCK never set to capture microphone (corrupt-file risk, capture-stack Q2).
- Window list excludes own process windows; titles only shown locally.

## Implementation notes (2026-10-01)
- Start order changed: screen first (off main via `recording::start_screen`), then mic + camera on main (`Started::attach`). Screen failure removes the empty folder; nothing else to unwind. Later tracks get `-itsoffset`/`aresample first_pts=0` in export.
- `chrono` not added: folder name via `libc::localtime_r`.
- Screen audio presence checked with `export::has_audio` (ffprobe) at stop; missing → `system_audio=false`.
- objc2 feature trimming skipped (compile time only).

## Next steps
Phase 4 builds the real panel, bubble (camera/waveform), and region overlay on these APIs.
