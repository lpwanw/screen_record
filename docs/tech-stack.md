# Tech Stack

macOS-only Rust screen recorder for product demos. Approved 2026-10-01.

| Concern | Choice | Notes |
|---|---|---|
| Platform | macOS 15.0+ | SCRecordingOutput + SCK mic APIs need 15.0 |
| Language | Rust (stable, pinned via `rust-toolchain.toml`) | rustc 1.88 too old for deps |
| UI | GPUI (`gpui-pre` pinned exact, e.g. `=0.3.7`) + `gpui-component` | Weekly snapshots, no API stability; upgrade deliberately |
| Screen + system audio | `screencapturekit` 11 (`macos_15_0`) → SCRecordingOutput | HEVC mp4 at 60 fps (VFR), native pixels; system audio = AAC track in the screen file; own app excluded via `SCContentFilter` |
| Mic | `cpal` 0.18 input → `hound` WAV (32-bit float, native rate) | Writer thread; same callback feeds the bubble waveform level |
| Camera | `objc2-av-foundation` (AVCaptureSession) | MovieFileOutput (`camera.mov`) + VideoDataOutput preview coexist (verified) |
| Native glue | `objc2`, `raw-window-handle` | Bubble NSWindow frame → global points for `project.json` |
| Export | `ffmpeg` CLI (PATH, `/opt/homebrew/bin`, `/usr/local/bin`), `h264_videotoolbox` | `filter_complex` built from `project.json`; output fit to ≤3840×2160 (encoder limit), CFR 60 fps |
| Packaging | `.app` bundle + stable codesign identity | Required for TCC (screen/camera/mic) prompts |
| Sync | Host clock (`CLOCK_UPTIME_RAW` ns) stamps per track | Export aligns tracks with `-itsoffset` / `-ss`; screen is the timeline anchor |

## Rejected
- Tauri v2 (two languages, transparency-after-bundle bug), egui (weak transparent-window polish).
- `scap` (stale), `nokhwa` (no file recording), ffmpeg-next (build pain), AVFoundation/wgpu compositing (too much code for v1).
- Copying Cap code (AGPLv3); architecture reference only.

## Deferred
- Bundled LGPL ffmpeg (needed before outside distribution).
- Editor (trim, move bubble, zoom) → driven by same `project.json`; wgpu renderer only if live preview needed.

## Research
- `plans/reports/researcher-261001-0009-gpui-ui-framework.md`
- `plans/reports/researcher-261001-0009-macos-capture-stack.md`
- `plans/reports/researcher-261001-0009-export-compositing.md`
