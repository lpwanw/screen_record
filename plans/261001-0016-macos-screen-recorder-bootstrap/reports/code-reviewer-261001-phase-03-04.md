# Code Review: Phases 3-4 (capture backend + UI)

## Scope
- Files: src/screen.rs, mic.rs, camera.rs, macos.rs, recording.rs, app.rs, bubble.rs, region_overlay.rs, main.rs, export.rs (`has_audio`), Cargo.toml. ~1900 LOC. autotest ignored.
- Read-only. `cargo build` ok, `cargo clippy` clean, `cargo test` 16 pass / 1 ignored. No hardware run.
- Plan files not modified (read-only task).

## Overall
Solid. Threading split is right (Send `Finishing`, !Send `Camera`/`Mic` on main, waits off main). No deadlock / Send-soundness / coordinate-math defect found. Findings are robustness gaps, ranked below.

## Critical
None.

## High
None.

## Medium
1. **Bubble outside capture rect is silently clamped onto the recording edge** (recording.rs:126, project.rs:77 `normalize_bubble`). Bubble opens bottom-right of primary display. Record display 2, a Region elsewhere, or a Window not under the bubble -> `x/y` clamp to `[0, 1-d]`, export draws bubble glued to a corner/edge that has no relation to where user saw it. Display mode on primary is fine. Decision needed (see questions): omit bubble when centre is outside capture, or keep clamp as intended.
2. **Record start blocks main on SCK with 30s worst case** (app.rs:383-393 -> screen.rs:145,218,219). `SCShareableContent::get`, `add_recording_output`, `start_capture` each wait on a completion with crate default 30s timeout (`utils/completion.rs:21`); `mic.start` adds up to 2s. Normal ~0.2-0.35s (spike) but SCK stalls (sleep/wake, TCC prompt pending) freeze the whole UI incl. bubble. Phase-4 todo "start path async" still open. `ScreenRecording` is Send: run `screen::start` on background executor, then mic/camera start back on main (state `Starting`). Must also block Record/close during `Starting`.
3. **App termination mid-recording leaves unfinalized files.** `quit_when_idle` only covers the window close button. Dock "Quit"/logout -> gpui `will_terminate` (gpui-macos platform.rs:1392) -> process exits; `Session` dropped without `stop_capture`/`stopRecording` -> screen.mp4/camera.mov have no moov, unplayable, no project.json. No `cx.on_app_quit` registered (gpui app.rs:2458, cannot cancel). Minimum: on_app_quit that calls stop + bounded wait; or accept and document.
4. **Failure after start leaves orphan folder without `project.json`** (recording.rs:96-99). Stop within first ~0.3s -> `screen_start == 0` -> Err "no screen frames", dir with camera.mov/mic.wav/empty mp4 remains, unreachable from UI (`last_dir` not set). Same for `screen.finish()?` failure (e.g. `on_fail` Err buffered, file may be valid but dropped). Remove dir on "no frames"; on other screen errors keep dir and surface its path in the error.

## Low
5. **Mic stream always open from launch + per-callback alloc** (mic.rs:91-100). `data.to_vec()` allocated on the CoreAudio RT thread and pushed to an unbounded channel every buffer, even when idle (writer discards). Also keeps macOS mic indicator lit whenever a mic is selected (needed for bubble waveform, so acceptable). Cheap fix: `Arc<AtomicBool> recording` set in `MicHandle::start/stop`, callback sends samples only when set (peak still updated). Unbounded channel grows if disk stalls.
6. **Mic `Start` timeout orphan** (mic.rs:122): if reply misses 2s, `recording.rs` drops mic (`None`) but writer later creates the take and keeps writing WAV forever until next Start/Stop. Send Stop on timeout.
7. **WAV 4GiB RIFF limit**: 32-bit float 48k stereo = 384KB/s -> ~3.1h. hound overflows the u32 header. Unlikely; note only.
8. **First-frame stamp not filtered by frame status** (screen.rs:208-214). Handler takes PTS of the first Screen buffer of any status; SCK may emit Idle/blank buffers that the recording output does not write. `SampleBuffer::frame_status()` exists (cm/sample_buffer.rs:92). Verify on hardware; filter to `Complete` if a skew shows up in the flash/beep test.
9. **Camera track silently dropped when `didStart` PTS invalid** (recording.rs:107 `.filter(>0)`) - no log line, unlike other drops.
10. **Window mode captures frame snapshot at start** (screen.rs:157). Window moved during recording -> bubble normalisation drifts; resize is documented non-goal, move is not.
11. **Stale lists**: displays fixed at `Panel::new`; `screen::displays().unwrap_or_default()` hides SCK errors (app.rs:118). Hot-plug -> "display disconnected" at record, acceptable.
12. `export::has_audio(...).unwrap_or(true)` (recording.rs:114): ffprobe missing -> assumes audio -> export maps `[0:a]` that may not exist -> ffmpeg fails. ffprobe ships with ffmpeg so near-impossible; `unwrap_or(false)` is the safer default.
13. `main.rs:18,35` `expect` on bubble/panel window open (plan wanted errors to UI). Launch-time only.
14. `select_region` can open multiple overlays on multi-display setups (button only disabled while busy). Guard with a flag.
15. Overlay menu-bar/notch coverage and Esc delivery: hardware check only (Esc deferred).
16. No tests for `recording.rs` (`new_dir` suffix loop) or `macos::cmtime_ns` edge (value 0 + invalid flags returns `Some(0)`; harmless because 0 is treated as unset).

## Verified non-issues (checked against source)
- Bubble/region coordinate math: `window_frame` flip (`main_h - y - h`) correct for any display incl. negative origins; region `capture = display.frame + rect` consistent with bubble global Quartz; `source_rect` display-local points; `normalize_bubble` uses same space.
- `SCStream::stop_capture` after stream already died: crate sets `capturing=false` on stop-with-error (sc_stream.rs:226) -> returns Ok, so user "Stop sharing" does not turn a valid file into an Err.
- Updating bubble window from inside a Panel update (`WindowHandle::update`) is a different window -> no re-entrancy; `on_window_should_close` -> `panel.update` -> `stop` path is safe.
- Subscribing to `SelectEvent` and reading `selected_index` inside callback: emit is queued, entity not leased.
- `Finishing` Send: only receivers, Arcs, `SCStream`/`SCRecordingOutput` (crate `unsafe impl Send`), plain data. `Camera`/`Mic` never leave main.
- Main never waits on AVF delegate: camera `stopRecording` is async, `wait` on background.
- Mic first-sample time: first `Samples` after `Start` in channel order -> `capture_ns` is truly first written sample. Offsets +/- handled by export `-itsoffset`/`-ss`; `aresample first_pts=0` pads silence so delayed mic keeps offset.
- Image leaks: stale `RenderImage`s dropped each render; mic writer thread exits when last `MicHandle` clone drops (bubble clone replaced in `update_bubble`).
- State machine: Recording->close: stop + quit_when_idle -> quits in `become_idle` incl. failure path; Finishing/Exporting close waits. Export/Record buttons gated by `busy`.

## Positives
Clean track-failure isolation (`log_failure`), single host clock, `inspect_err` dir cleanup on screen start failure, graceful permission states, small focused modules, good mic writer unit tests.

## Recommended actions
1. Decide bubble-outside-capture policy (Q1) -> small change in `normalize_bubble`/export.
2. Move `screen::start` off main (Medium 2).
3. Add `on_app_quit` finalize or document (Medium 3).
4. Clean dir on zero-frame stop; surface dir on other failures (Medium 4).
5. Low items 5, 6, 9, 12 are one-liners.

## Metrics
- Tests: 16 pass, 1 ignored. Clippy: 0 warnings (only a dep future-incompat note: block v0.1.6). Coverage not measured.

## Unresolved questions
1. Bubble centre outside capture rect: clamp to edge (current) or omit bubble from export?
2. Is a ~0.3s (worst 30s) UI freeze on Record acceptable for this phase, or do it async now?
3. Is data loss on Dock-Quit acceptable for v1?
