---
title: "Phase 1: Spike + foundation"
status: done
effort: 12h
---

# Phase 1: Spike + foundation

## Context links
- Parent: [plan.md](./plan.md). Deps: none. Blocks: phases 2-5.
- Docs: `docs/tech-stack.md`; `research/researcher-01-gpui-api.md`; `research/researcher-02-capture-api.md`; `../reports/researcher-261001-0009-gpui-ui-framework.md` (§2-4), `../reports/researcher-261001-0009-macos-capture-stack.md` (Q2, Q4, Q7, Q8).

## Overview
- Date: 2026-10-01. Priority: P1 (blocks everything). Implementation: Done (2026-10-01). Review: Done — fixed re-record-while-finishing guard, AVErrorRecordingSuccessfullyFinishedKey, stop_capture off main thread, own-app guard, CVReturn check, camera teardown, quit on panel close, idle pump, build.rs rerun, bundle.sh order/timeout, .gitignore. Kept 1-frame-delayed drop_image (verified flat memory; avoids freeing an atlas tile an in-flight frame may still use). Deferred start-path async + UI error handling to phase 4.
- Pin toolchain + deps, build signed .app dev loop, run 5 smoke checks on real hardware. Each verdict picks a branch for phases 3/4. Spike code is the first cut of the real modules (no throwaway files, no `--smoke` flags): smokes are driven by panel buttons that later become real controls.

## Key Insights
- Env checked 2026-10-01: rustc 1.88 only (`rustup` 1.28.2 present); CLT only (`xcrun --find metal` fails, `swift` present → SCK Swift bridge builds); `security find-identity -v -p codesigning` = 0; main display 5120x2880 px, second 2560x1600; ffmpeg/ffprobe/ffplay in `/opt/homebrew/bin`.
- `h264_videotoolbox` fails at 5120x2880 (-12903), `hevc_videotoolbox` ok → SCRecordingOutput codec = HEVC (confirm in smoke d).
- `runtime_shaders` is on `gpui-pre-platform`, not `gpui-pre` (researcher-01-gpui-api.md:7). No `metal` here → mandatory.
- Inherent `Window::window_handle()` shadows raw-window-handle trait → call `raw_window_handle::HasWindowHandle::window_handle(&*window)` (researcher-01-gpui-api.md:52).
- `img()` + `rounded_full()` clips to circle; `surface()` cannot (researcher-261001-0009-gpui-ui-framework.md:30-31). Must `window.drop_image(old)` per replaced frame or atlas grows.
- TCC: run only as signed .app via `open`; ad-hoc signature resets grants every build (researcher-02-capture-api.md:73-76). No hardened runtime → no entitlements needed.
- `RenderImage` takes `image::Frame`; use the exact `image`/`smallvec` versions already in Cargo.lock (via gpui) or types mismatch.

## Requirements
- Functional: panel window (Normal) opens from .app; bubble window (PopUp, transparent, circle) shows 30fps frames; panel buttons: "Screen start/stop" (display mode, bubble excluded, sys-audio switch), "Camera start/stop".
- Non-functional: exact pins `gpui-pre =0.3.7`, `gpui-pre-platform =0.3.7` (features `font-kit`, `runtime_shaders`), `gpui-component =0.7.0`; Cargo.lock committed; builds with CLT only; bundle script in sh (no node).

## Architecture
- Dev loop: `scripts/bundle.sh` → `cargo build` → `target/TaywindRecord.app/Contents/{MacOS/taywind_record, Info.plist}` → `codesign --force --sign "Taywind Dev"` → `pkill -x taywind_record` (stale instance, else `open` just refocuses old build) → `open --stdout target/taywind.log --stderr target/taywind.log target/TaywindRecord.app`.
- Info.plist (heredoc in script): CFBundleIdentifier `dev.taywind.record`, CFBundleName "Taywind Record", CFBundleExecutable `taywind_record`, CFBundlePackageType APPL, CFBundleShortVersionString 0.1.0, CFBundleVersion 1, LSMinimumSystemVersion 15.0, NSHighResolutionCapable, NSCameraUsageDescription, NSMicrophoneUsageDescription, NSScreenCaptureUsageDescription.
- Frame path: BGRA source (synthetic in b, camera in e) → `Arc<Mutex<Option<camera::Frame>>>` latest-frame slot (overwrite, no backlog) → bubble `cx.spawn` 33ms timer → `RenderImage` → `img(..).rounded_full()`; previous image → `window.drop_image`.
- Exclusion path: bubble window → raw handle `ns_view` → `macos::window_number` (main thread) → `screen::start(exclude=[n])` → `SCContentFilter::create().with_display(d).with_excluding_windows(&[scwindow where window_id==n])` → SCRecordingOutput (HEVC, MP4).
- Camera: AVCaptureSession(DeviceInput) + AVCaptureMovieFileOutput (`camera.mov`) + AVCaptureVideoDataOutput (32BGRA, delegate on `dispatch2` serial queue, copies centered square into slot).

## Related code files
- Create: `rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`, `.gitignore` (`/target`, `*.p12`, `*.pem`), `scripts/bundle.sh`, `src/main.rs`, `src/app.rs`, `src/bubble.rs`, `src/screen.rs`, `src/camera.rs`, `src/macos.rs`.
- Modify: none (greenfield).

## Implementation Steps
1. `rust-toolchain.toml`: `channel = "1.98.1"` (Zed's pin; gpui-pre snapshots built with it) + `components = ["rustfmt","clippy"]`. `rustup show` installs. If a dep needs newer, bump to that stable.
2. `Cargo.toml` (package `taywind_record`, edition 2024): gpui trio per researcher-01-gpui-api.md:9-14, `raw-window-handle = "0.6"`, `screencapturekit = { version = "11", features = ["macos_15_0"] }`, `objc2`, `objc2-foundation`, `objc2-app-kit` (NSView, NSWindow), `objc2-av-foundation = "0.3"` (`features=["all"]` for spike, trim at end of phase 3), `objc2-core-media`, `objc2-core-video`, `block2`, `dispatch2`, `anyhow`, `libc`, `image`/`smallvec` (lock-matched).
3. Signing identity (one-time, user): Keychain Access → Certificate Assistant → Create a Certificate: name "Taywind Dev", Self Signed Root, Code Signing. CLI alt: `openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=Taywind Dev" -addext extendedKeyUsage=codeSigning` → `openssl pkcs12 -export` → `security import x.p12 -T /usr/bin/codesign` → `security add-trusted-cert -p codeSign`. Keys in scratch dir, deleted after import. Verify `security find-identity -p codesigning` lists it.
4. `scripts/bundle.sh` (`set -eu`, ~25 lines) per Architecture. `chmod +x`.
5. `main.rs`: `gpui_platform::application().run`, `gpui_component::init`, open panel (wrap in `gpui_component::Root` if its examples do). Smoke (a).
6. `camera.rs` starts with `pub struct Frame { width, height, bgra: Vec<u8> }`. `bubble.rs`: PopUp, `titlebar: None`, `WindowBackgroundAppearance::Transparent`, 200pt, synthetic moving BGRA gradient (red left edge) through the slot. Smoke (b).
7. `macos.rs`: `window_number(ns_view) -> Option<u32>`, `host_now_ns()` (`libc::clock_gettime_nsec_np(CLOCK_UPTIME_RAW)`), `screen_access()/request_screen_access()` (CGPreflight/CGRequestScreenCaptureAccess extern). `screen.rs`: display-mode `start(exclude, system_audio, path)`/`stop()`. Panel button + switch. Smokes (c), (d).
8. `camera.rs`: first device from DiscoverySession (BuiltInWideAngle, External, ContinuityCamera), session + both outputs, `start_recording(path)`/`stop_recording()`; slot feeds bubble instead of synthetic. Smoke (e).
9. Rebuild twice without code change → smoke (f). Fill Spike results table; commit.

## Spike results (fill in)
| Check | Pass criterion | Result | Branch |
|---|---|---|---|
| a build | `cargo build` ok with CLT only, .app launches, panel renders | pass: CLT-only build ok (runtime_shaders); needed `build.rs` for Swift runtime search path + `/usr/lib/swift` rpath (apple-metal assumes full Xcode) | none |
| b bubble | 30fps smooth, clean circle, transparent corners, red shows red, RSS growth < 20MB over 2 min (`ps -o rss= -p $(pgrep -x taywind_record)` every 10s) | pass: RSS 85-100MB flat over 2 min (1-frame-delayed drop_image); circle + drag ok | none |
| c exclude | frame from `screen.mp4` (`ffmpeg -ss 2 -i screen.mp4 -frames:v 1`) has no bubble | pass: bubble + panel absent from screen.mp4 frames | used `with_excluding_applications(own pid)` instead of windowNumber → no objc2-app-kit / raw-window-handle glue; hides every own window |
| d SCRecordingOutput | ffprobe: hevc video (5K) + exactly 1 aac audio; plays in QuickTime; finish signal known (delegate or file-size-stable poll); first-frame PTS via screen output handler; silent system → audio track present? | pass: hevc 5120x2880 + exactly 1 aac 48k stereo (sys audio, -34dB mean); `RecordingCallbacks::on_finish` fires; first-frame PTS via Screen output handler (+207..337ms after start) | none |
| e camera | preview frames flow while `camera.mov` records; mov duration ≈ wall time; `didStartRecordingTo:startPTS:` available? | pass: MovieFileOutput + VideoDataOutput coexist; camera.mov h264 1280x720 ~30fps; `didStartRecording…startPTS` works (+1.0-1.3s after start). didFinish delivered on MAIN thread → never block main thread waiting (stop/wait split, wait off-thread) | no AVAssetWriter fallback needed (-4h) |
| f TCC | no re-prompt after rebuild + re-sign | pass: no re-prompt across 3 rebuild+re-sign cycles (DR = certificate leaf hash) | none |

## Todo list
- [x] rust-toolchain.toml + Cargo.toml pins, Cargo.lock committed
- [x] "Taywind Dev" identity created and listed
- [x] scripts/bundle.sh builds, signs, relaunches (dist/TaywindRecord.app), logs to ~/Library/Logs/TaywindRecord.log
- [x] Smoke a: hello panel as .app
- [x] Smoke b: circle bubble 30fps, memory flat 2 min
- [x] Smoke c: bubble excluded from screen.mp4
- [x] Smoke d: SCRecordingOutput HEVC + 1 audio track, finish signal
- [x] Smoke e: camera file + preview coexist
- [x] Smoke f: TCC grant survives rebuild
- [x] Spike results table filled, branches recorded

## Success Criteria
- `scripts/bundle.sh` from clean clone → running app, log file populated.
- Spike table has a Result + Branch for a-f; no check left "unknown".
- `git ls-files` includes Cargo.lock; no key/cert files tracked.

## Risk Assessment
| Risk | L×I | Signal it broke | Response |
|---|---|---|---|
| gpui-pre/transitive MSRV > pinned rustc | M×L | "requires rustc 1.xx" | bump pin (adjust) |
| `runtime_shaders` still calls `metal` | L×H | build error "unable to find utility metal" | STOP: user decides full Xcode install |
| SCK Swift bridge fails on CLT/macOS 27 SDK | M×H | build.rs swift errors | try newest 11.x; else STOP & replan (objc2-screen-capture-kit) |
| gpui-component needs `Root`/font setup | M×L | panic "no Root" / blank text | follow crate examples (adjust) |
| Atlas leak from per-frame images | M×H | RSS climbs linearly | see branch b |
| Self-signed identity unusable by codesign | M×M | "no identity found" / TCC re-prompt | trust cert for codeSign; GUI creation path (adjust) |
| SCK exclusion ignored on macOS 27 | L×H | bubble in frame | branch c |

## Security Considerations
- Private key only in login keychain; `.p12`/`.pem` never in repo (`.gitignore`), scratch copies deleted.
- No hardened runtime/entitlements in dev; unsandboxed app, local files only, no network.
- `window_number` unsafe cast: main thread only, window alive.

## Next steps
Phase 2 (hardware-free). Carry branch decisions b-e into phases 3/4.
