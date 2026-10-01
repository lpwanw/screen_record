# Taywind Record

A macOS screen recorder for product demos, written in Rust with [GPUI](https://gpui.rs). It records a display, a window or a dragged region. A floating circle bubble shows your face, or a live mic waveform when there is no camera. Export produces one MP4.

## Features
- **Capture:** full display, a single window, or a drag-selected region, at 60 fps.
- **Bubble:** a draggable, always-on-top circle showing the camera, or the mic waveform without a camera. The bubble and the control panel never appear in the screen track.
- **Audio:** microphone, plus system audio as a toggle.
- **Separate tracks:** each recording keeps its tracks apart (`screen.mp4`, `camera.mov`, `mic.wav`) together with a `project.json`, so editing can be added later.
- **Export:** one H.264/AAC `export.mp4`, with the bubble composited where it sat when you pressed Stop. Tracks are aligned on the host clock, and the output is scaled to fit within 3840×2160.

## Requirements
- macOS 15 or later.
- Xcode Command Line Tools (`xcode-select --install`); full Xcode is not needed.
- [rustup](https://rustup.rs). The toolchain is pinned in `rust-toolchain.toml`.
- ffmpeg for export: `brew install ffmpeg`.
- A local code-signing identity named `Taywind Dev`, created once with the steps in [docs/development.md](docs/development.md#signing-identity). macOS ties Screen Recording, Camera and Microphone permissions to that signature.

## Quick start
```sh
./scripts/bundle.sh
```
This builds, signs and launches `dist/TaywindRecord.app`. Logs go to `~/Library/Logs/TaywindRecord.log`.

- Unlock the Mac before running it. The first signing shows a keychain prompt; choose **Always Allow**.
- On first launch, grant Screen Recording and then relaunch. Grant Camera and Microphone when asked.

To share the app, `./scripts/bundle.sh --dmg` writes `dist/TaywindRecord.dmg`; see [docs/development.md](docs/development.md#share-as-dmg) for what recipients need to do.

## Usage
1. Choose **Display**, **Window** or **Region**. For Region, click **Select region…**, then drag; right-click cancels.
2. Choose a camera and a microphone (or None), and set **System audio**. Drag the bubble to where it should appear in the video.
3. Press **Record**, then **Stop**.
4. **Export MP4** writes the final video, and **Open folder** shows it.

Recordings are saved in `~/Movies/Taywind/<YYYY-MM-DD HH-MM-SS>/`. [docs/development.md](docs/development.md#using-the-app) lists each file in the folder.

## Development
```sh
cargo test                  # project model, ffmpeg arguments, mic writer
cargo test -- --ignored     # end-to-end export on synthetic inputs (needs ffmpeg)
cargo clippy --all-targets
```
Always run the signed `.app` through `scripts/bundle.sh`, not `cargo run`, because the permissions belong to the bundle.

| Module | Role |
|---|---|
| `src/app.rs` | Control panel and the record/export state machine |
| `src/bubble.rs` | Floating camera / waveform bubble |
| `src/region_overlay.rs` | Drag-to-select region overlay |
| `src/recording.rs` | Starts, stops and finalizes the tracks, then writes `project.json` |
| `src/screen.rs`, `src/camera.rs`, `src/mic.rs` | ScreenCaptureKit, AVFoundation and cpal capture |
| `src/project.rs`, `src/export.rs` | `project.json` model and the ffmpeg compositor |
| `src/macos.rs` | Permissions, host clock, native window frame |

## Known limitations (v1)
- macOS only. Export uses the ffmpeg you have installed; it is not bundled.
- Only the most recent recording of the current session can be exported.
- Quitting from the Dock while recording loses that recording. Closing the panel saves it first.
- A bubble left outside the captured area is pinned to the nearest edge in the export.
- In Window mode, moving or resizing the window during a recording is not followed.
- Esc may not cancel region selection; right-click always does.

## Docs
- [docs/development.md](docs/development.md): setup, signing identity, usage, recording folder layout.
- [docs/tech-stack.md](docs/tech-stack.md): stack choices and alternatives considered.
- [Bootstrap plan](plans/261001-0016-macos-screen-recorder-bootstrap/plan.md), including the [manual verification checklist](plans/261001-0016-macos-screen-recorder-bootstrap/phase-05-integration-verification.md).
