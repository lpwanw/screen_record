---
title: "taywind_record bootstrap: macOS screen recorder (Rust + GPUI)"
description: "Record display/window/region with a floating camera-or-waveform bubble into separate tracks, then export one composited mp4 via ffmpeg."
status: in-progress
priority: P2
effort: 52h
branch: main
tags: [rust, gpui, macos, screen-recorder]
created: 2026-10-01
---

# taywind_record bootstrap

## Overview
macOS 15+ Rust app for product demos. GPUI control panel + always-on-top circle bubble (camera, else mic waveform). Record screen (+system audio track) / mic WAV / camera file separately + `project.json` into `~/Movies/Taywind/<datetime>/`; Export → one H.264+AAC mp4 via ffmpeg CLI. Stack: `docs/tech-stack.md`. Research: `./research/`, `../reports/researcher-261001-0009-*.md`.

## Phases
| # | Phase | Status | Effort | Progress |
|---|-------|--------|--------|----------|
| 1 | [Spike + foundation](./phase-01-spike-foundation.md) | Done | 12h | 100% |
| 2 | [Project model + export](./phase-02-project-model-export.md) | Done | 7h | 100% |
| 3 | [Capture backend](./phase-03-capture-backend.md) | Pending | 15h | 0% |
| 4 | [UI: panel, bubble, region overlay](./phase-04-ui-panel-bubble-overlay.md) | Pending | 12h | 0% |
| 5 | [Integration + verification](./phase-05-integration-verification.md) | Pending | 6h | 0% |

## Dependencies
- Strictly sequential 1 → 2 → 3 → 4 → 5 (shared `Cargo.toml`, `src/main.rs` mod lines; single dev).
- 1 blocks all: toolchain, exact pins, bundle+sign loop, 5 smoke verdicts that pick branches in 3/4. STOP & replan if smoke a (build), b (persistent atlas leak), c (exclusion) or d (corrupt file) fails.
- 2 hardware-free; 3's `recording.rs` consumes 2's `Project`; 4 consumes 3's APIs; 5 needs all.

## Layout (flat)
`src/{main,app,bubble,region_overlay}.rs` = only files calling GPUI. `src/{screen,mic,camera,recording,project,export,macos}.rs` = no GPUI. `scripts/bundle.sh` (sh). `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock` committed.

## Data flow
UI selections → `recording::start` → screen (SCRecordingOutput `screen.mp4`, sys audio inside) + mic (cpal→hound `mic.wav`) + camera (`camera.mov`), each stamps `start_host_ns` (CLOCK_UPTIME_RAW ns = CMSampleBuffer host clock) → stop → `project.json` (tracks, offsets, mode/rect, px dims, duration, bubble normalized). Export: `project.json` → `export::ffmpeg_args` (one fn) → ffmpeg child → `-progress pipe:1` → UI bar → `export.mp4`.

## Decisions verified locally 2026-10-01 (supersede research where they differ)
- Main display 5120x2880 px: `h264_videotoolbox` fails (-12903) >4096x2304; HEVC ok → raw screen track HEVC; export scales to fit 3840x2160.
- CLT only, no `metal` → `runtime_shaders` mandatory. 0 codesign identities → phase 1 creates "Taywind Dev".
- `launchctl getenv PATH` empty → `open`-launched app can't see Homebrew; resolve ffmpeg via PATH + `/opt/homebrew/bin` + `/usr/local/bin`.
- ffmpeg 8.0.1 graph run on synthetic 5K inputs: screen = timeline anchor; later track `-itsoffset d`, earlier track input `-ss d`; `apad` + output `-t <duration>` (overlay otherwise outlives screen); soft-edge `geq` circle ok; `fps=60` before overlay required (overlay emits only on main frames → VFR screen freezes bubble).

## Spike outcomes (2026-10-01, supersede phases 3-4 where they differ)
- Exclusion = `SCContentFilter::with_excluding_applications(own pid)` (hides panel + bubble + overlay); drop `macos::window_number` / raw-window-handle / objc2-app-kit.
- AVFoundation file-output callbacks arrive on main thread → stop on main, `wait()` off-thread (`ScreenRecording::stop/wait`, `CameraRecording::wait`).
- Start latency: screen first frame +0.2-0.35s, camera file start +1.0-1.3s after start → export offsets mandatory.
- Dev loop: `dist/TaywindRecord.app`, log `~/Library/Logs/TaywindRecord.log`, `build.rs` adds Swift runtime path; setup in `docs/development.md`.

## Acceptance criteria
1. Launches as signed .app; requests screen/camera/mic permissions.
2. Lists displays + windows; region drag-select overlay works (Esc cancels).
3. Bubble floats, draggable, shows camera or mic waveform.
4. Start/stop in display/window/region modes with system-audio toggle.
5. Bubble absent from raw `screen.mp4`.
6. Project folder: separate track files + `project.json`.
7. Export → playable mp4, circle bubble at stored position, A/V sync drift < 100ms (flash/beep measured).
8. `cargo test` passes: project.json roundtrip + ffmpeg args (camera, no-camera, offsets).

## Rollback
Greenfield. One commit per phase → `git revert`. No data migrations; `project.json` carries `version: 1`. Recordings in `~/Movies/Taywind` untouched by reverts. TCC reset: `tccutil reset All dev.taywind.record`.

## Decisions (user, 2026-10-01)
- Plan approved; start phase 1.
- Export: H.264 fit to 3840x2160 (not HEVC native 5K).
- Exclude control panel AND bubble from screen track (display/region modes).
- Screen capture 60fps.
- Defaults kept (not contested): bubble position stored at stop; dark disc + white waveform; 200pt bubble; camera preview unmirrored.
