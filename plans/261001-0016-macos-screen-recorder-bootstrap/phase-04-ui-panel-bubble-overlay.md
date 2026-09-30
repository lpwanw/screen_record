---
title: "Phase 4: UI - panel, bubble, region overlay"
status: implemented
effort: 12h
---

# Phase 4: UI - panel, bubble, region overlay

## Context links
- Parent: [plan.md](./plan.md). Deps: phase 1 (window/bubble spike, verdict b), phase 2 (`export::run`, `Project`), phase 3 (screen/mic/camera/recording/macos APIs). Blocks: phase 5.
- Docs: `research/researcher-01-gpui-api.md` (§3-7), `../reports/researcher-261001-0009-gpui-ui-framework.md` (§2-3).

## Overview
- Date: 2026-10-01. Priority: P2. Implementation: Done (walk-through pending, Mac locked overnight). Review: Done (`reports/code-reviewer-261001-phase-03-04.md`).
- Real control panel, final bubble (camera image or mic waveform, drag, bounds for project.json), region drag-select overlay. All GPUI code lives only in `main.rs`, `app.rs`, `bubble.rs`, `region_overlay.rs`.

## Key Insights
- GPUI content masks are rectangular: only `img(..).rounded_full()` and quad corner radii clip to circles. Waveform must be drawn geometrically inside the circle (bar height ≤ chord at its x), not clipped.
- PopUp = nonactivating NSPanel (level 101, all spaces): right for bubble + overlay, but key events (Esc) may not arrive (gpui-ui-framework.md:27). Panel stays `Normal`.
- `start_window_move()` on mouse down drags a titlebar-less window.
- `cx.spawn(async move |this, cx| ..)` + background executor timer for 30fps pump and 1s elapsed tick; `AsyncApp::update` not fallible in this snapshot (researcher-01-gpui-api.md:49).
- Mic/Camera are owned here (main thread, possibly `!Send`); only `MicHandle`/receivers cross threads (phase 3).
- Bubble position is sampled at Stop (default; see plan Q1), in global points, then normalized by `recording` against the capture rect.

## Requirements
- Panel: mode (Display / Window / Region); display select; window select + refresh; "Select region…" (shows W×H); camera select incl. None; mic select incl. None; system-audio switch; Record/Stop + elapsed; Export + progress bar; Open folder; permission banners (screen needs relaunch after grant); error line. Inputs disabled while recording/finishing/exporting.
- Bubble: 200pt circle, PopUp, transparent, bottom-right of primary display (40pt margin), draggable, camera frames when camera selected, else live mic waveform; own-app exclusion hides it (no windowNumber).
- Region overlay: fullscreen transparent PopUp on selected display, dimmed, crosshair, drag rect with border + size label, mouse-up commits (≥16pt each side), Esc or right-click cancels.
- Closing panel while recording: stop + wait (no lost recording) before quit.

## Architecture
- `app.rs` `Panel` entity. `Status = Idle | Recording{ session, since } | Finishing | Exporting(f32)`; `last_dir: Option<PathBuf>`; `error: Option<String>`. Holds `Option<Mic>`, `Option<Camera>`, `WindowHandle<Bubble>`, `bubble_number: Option<u32>`, lists + selections.
  - Startup: `macos::screen_access()` else banner + Grant (`request_screen_access`); `av_status`/`request_av` for camera + mic (callback → channel → `cx.spawn` update); list devices/displays/windows; open bubble; open default mic + first camera.
  - Record: build `recording::Options` (target from mode; region requires rect; own app excluded by screen filter) → `recording::start` → `Recording`.
  - Stop: `bubble.update(cx, |_, w, _| w.bounds())` → `Rect` pt → `session.stop(rect)` → `cx.background_spawn(finishing.wait())` → `Idle`, `last_dir`.
  - Export: `cx.background_spawn(export::run(dir, |p| tx.send(p)))`; UI task drains `tx` → `Exporting(p)`; done → Open folder (`Command::new("open").arg(dir)`); Err → error line with `export.log` path.
- `bubble.rs` `Bubble { source: Camera(slot) | Mic(level) | Empty, image: Option<Arc<RenderImage>>, stale: Vec<Arc<RenderImage>>, levels: VecDeque<f32> (48) }`.
  - 30fps task: camera → `slot.take()` → BGRA `RenderImage` → swap, old → `stale`; mic → push level; `notify`.
  - `render`: `window.drop_image` each stale; root `div().size_full().rounded_full().bg(0x1e1e1e).on_mouse_down(Left, start_window_move)`; child `img(image).size_full().rounded_full()` or `canvas` waveform: 48 bars, bar i at x_i, height = max(2px, level_i·0.9·2·sqrt(r²-(x_i-r)²)), centered, rounded quads.
  - (dropped) window_number: screen filter excludes own app (spike c).
- `region_overlay.rs`: `open(cx, display, on_done: impl Fn(Option<Rect>, &mut App))`. `WindowBounds::Windowed(display.bounds())`, `display_id`, `focus: true`. Mouse down/move/up track start+current (window-local = display-local points) → canvas draws rect; commit/cancel → `on_done` → `window.remove_window()`.
  - SC display ↔ GPUI display: match id [U]; else match bounds (±1pt).

## Related code files
- Create: `src/region_overlay.rs`.
- Modify: `src/app.rs` (full panel), `src/bubble.rs` (final), `src/main.rs` (mod + quit handling).

## Implementation Steps
1. Read gpui-component 0.7.0 sources/examples for `Root`, `Button`, `Switch::on_click`, `Select`/`SelectState` delegate, progress widget; resolve [U]s (researcher-01-gpui-api.md:62-69).
2. `bubble.rs`: sources, 30fps pump, stale-image dropping, waveform canvas, drag, bounds getter.
3. `app.rs`: state, startup permission + device flow, selections (reopen Mic/Camera on change, update bubble source).
4. Record/Stop/Finishing flow + elapsed tick; disable inputs while busy.
5. `region_overlay.rs` + panel "Select region…" + display mapping.
6. Export + progress + Open folder + error line.
7. Quit/close-while-recording handling.
8. Manual walk-through of acceptance 1-4 on both displays; `cargo clippy`.

## Todo list
- [x] gpui-component APIs confirmed from source (Select/SelectEvent, RadioGroup, Switch, Progress 0-100)
- [x] Bubble: camera image, waveform (chord-bounded bars), drag, global frame via NSWindow
- [x] Panel: selections, permissions, banners
- [x] Record/Stop with elapsed + disabled inputs
- [x] Region overlay: drag, commit, Esc/right-click cancel (Esc delivery to PopUp unverified)
- [x] Export progress + Open folder + errors
- [x] Close-while-recording saves recording

- [x] From phase-1 review: start path async (SCShareableContent::get + start_capture off main thread, camera start back on main)
- [x] From phase-1 review: errors to UI; camera/mic grant callback re-opens device without relaunch
- [ ] `startRunning` off main thread (deferred: ~0.5s on camera switch only)

## Success Criteria
- Acceptance 1-4 pass by hand: fresh TCC (`tccutil reset All dev.taywind.record`) shows 3 prompts; lists populated; overlay selects region on each display; bubble drags and shows camera / waveform reacting to voice within ~100ms.
- Export button yields progress 0→100% and Open folder reveals `export.mp4`.
- Bubble RSS flat over 5 min recording (same method as spike b).

## Risk Assessment
| Risk | L×I | Signal it broke | Response |
|---|---|---|---|
| PopUp overlay gets no key events | M×M | Esc ignored | `focus: true` + `cx.activate(true)` + focus handle; else `macos` makeKeyWindow via ns_view; right-click always cancels (adjust) |
| Overlay not covering menu bar/notch | M×L | top strip not dimmed/selectable | accept, or raise level via objc2 (adjust) |
| GPUI global pts ≠ Quartz global pts | L×H | phase 5 bubble offset in export | convert via matched display origin (adjust) |
| `Select` API fights us | M×L | delegate trait errors | clickable row list in scroll div (adjust) |
| Debug build stutters (frame pump) | M×L | bubble < 30fps in debug | `[profile.dev.package."*"] opt-level = 3` (adjust) |
| `startRunning` blocks main on camera switch | M×L | ~0.5s freeze | move to dispatch queue if annoying (adjust) |

## Security Considerations
- Permission denied paths degrade gracefully (banner, track disabled), never crash.
- `open <dir>` via `Command` args, no shell.
- Window titles from other apps shown only locally.

## Implementation notes (2026-10-01)
- GPUI window bounds are display-local and display bounds report origin 0 → bubble frame read from NSWindow (`raw-window-handle` + `macos::window_frame`), flipped to Quartz global points.
- Defaults chosen while user away (review questions): bubble outside capture rect is clamped to the nearest edge (face stays visible); Dock Quit / logout mid-recording is not handled (only panel close finishes the recording first).
- Only the last recording of the session can be exported (no "open recording" UI in v1).

## Next steps
Phase 5 end-to-end matrix, sync calibration, docs.
