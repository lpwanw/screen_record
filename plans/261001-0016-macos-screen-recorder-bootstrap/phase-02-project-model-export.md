---
title: "Phase 2: Project model + export"
status: done
effort: 7h
---

# Phase 2: Project model + export

## Context links
- Parent: [plan.md](./plan.md). Deps: phase 1 (toolchain, Cargo.toml). Blocks: phase 3 (`recording.rs` builds `Project`), phase 4 (export button).
- Docs: `../reports/researcher-261001-0009-export-compositing.md` (verified cmds lines 12-33, alignment line 39, progress line 42).

## Overview
- Date: 2026-10-01. Priority: P2. Implementation: Done. Review: Done (`reports/code-reviewer-261001-phase-02.md`, 0 critical; late-start single audio source fixed with `aresample=48000:async=1:first_pts=0`, covered by ignored test).
- Pure logic, no hardware: `project.json` model + bubble normalization, ffmpeg arg builder (one fn), progress parser, ffmpeg locator + runner. `cargo test` covers roundtrip + args; `#[ignore]` lavfi test proves the graph runs.

## Key Insights (verified with ffmpeg 8.0.1 on synthetic 5K inputs, 2026-10-01)
- `h264_videotoolbox` can't open >4096x2304 → fit output into 3840x2160 box (orientation-aware), even dims.
- `overlay` emits only when main (screen) has a frame: sparse input gave 6 output frames vs 90 with `fps=30` first → `fps=FPS` on screen before overlay, else bubble freezes on static screen (SCK is VFR).
- `overlay` output outlives screen when bubble input is longer (3.48s vs 3.0s) → output `-t duration_s` + `apad` on audio. Drop `-shortest`.
- `-itsoffset 0.5` on camera: frames before 0.5s pass screen through untouched (checked visually).
- Soft circle: `geq=lum='p(X,Y)':cb='p(X,Y)':cr='p(X,Y)':a='clip(255*(R-hypot(X+0.5-R,Y+0.5-R)),0,255)'` (R=D/2) → clean AA edge.
- Waveform disc: `color=c=0x1e1e1e:s=DxD:r=30[bg];[mw]showwaves=s=DxD:mode=cline:rate=30:colors=white[w];[bg][w]overlay=shortest=1,format=yuva420p,geq=…` works.
- Args passed via `Command::args` (no shell) → filtergraph single quotes stay literal and correct. Shell-testing graphs in zsh: brace vars (`${A}[b]`), `$A[b]` is array subscripting.
- `-progress pipe:1` emits `out_time_us=` and `progress=end`.
- GUI app PATH lacks Homebrew → locate ffmpeg explicitly.

## Requirements
- `project.json` holds: version, mode, captured rect (global points), screen px dims, duration, per-track file + `start_host_ns`, system_audio flag, bubble normalized to captured frame.
- Export: circle camera (or circle mic waveform when no camera) at stored position; mic + system audio via `amix normalize=0`; tracks aligned; H.264 VT + AAC mp4; progress 0..1.
- Tests: roundtrip, normalize clamp, args (camera, no-camera, offsets ±, no audio), fit, progress; ignored integration.

## Architecture
`project.rs` (serde, no GPUI):
```rust
pub const FPS: u32 = 60;
pub struct Project { version: u32, mode: Mode, rect: Rect, width: u32, height: u32, duration_s: f64,
    screen: Track, system_audio: bool, mic: Option<Track>, camera: Option<Track>, bubble: Bubble }
pub enum Mode { Display, Window, Region }        // serde lowercase
pub struct Track { file: String, start_host_ns: u64 }
pub struct Rect { x: f64, y: f64, w: f64, h: f64 } // global points, top-left origin
pub struct Bubble { x: f64, y: f64, d: f64 }     // x,y = top-left / frame w,h; d = diameter / frame w
Project::load(dir) / save(dir)                     // dir/project.json, pretty JSON
Project::offset_s(&self, t: &Track) -> f64        // (t.start - screen.start) / 1e9, i128 math
normalize_bubble(bubble: Rect, frame: Rect) -> Bubble // clamp d ≤ min(1, h/w); x,y into [0, 1-size]
```
`export.rs` (no GPUI). `ffmpeg_args(p, dir, out) -> Vec<String>`:
1. Inputs: `0` = screen (anchor). Camera, mic: offset d = `offset_s`; d > 0.001 → `-itsoffset d`; d < -0.001 → `-ss -d`; `{:.3}`.
2. `(W,H) = fit_even(width,height)`; `[0:v]fps=FPS,scale=W:H[s]`.
3. D = even(d·W), X = round(x·W), Y = round(y·H). Camera → `[c:v]scale=D:D:force_original_aspect_ratio=increase,crop=D:D,format=yuva420p,geq=…[b]`. Else mic → `[m:a]asplit=2[ma][mw]` + waveform disc `[b]`. Then `[s][b]overlay=x=X:y=Y:format=auto[v]`; neither → `[s]` is `[v]`.
4. Audio sources: `[0:a]` if system_audio, mic (`[ma]` or `[m:a]`). 0 → `-an`; else each `aresample=48000:async=1:first_pts=0` (pads late starts) → `amix=inputs=N:normalize=0:duration=longest,apad[a]`.
5. Output: `-map [v] [-map [a]] -t duration_s -c:v h264_videotoolbox -b:v 20M -pix_fmt yuv420p -tag:v avc1 [-c:a aac -b:a 192k] -movflags +faststart -progress pipe:1 -nostats -y out`.
- `fit_even(w,h)`: box (3840,2160) if w≥h else (2160,3840); s = min(1, bw/w, bh/h); round each to even.
- `parse_progress(line, duration_s) -> Option<f32>`: `out_time_us=N` → (N/1e6/duration).clamp(0,1); `progress=end` → 1.0; else None.
- `tool(name) -> Option<PathBuf>`: PATH dirs, then `/opt/homebrew/bin`, `/usr/local/bin`; first existing file.
- `run(dir, on_progress) -> Result<PathBuf>`: load project → args → spawn (stdout piped, stderr → `dir/export.log`) → read lines → `on_progress` → wait; non-zero → Err with log path; Ok(`dir/export.mp4`). Blocking; caller runs it off the UI thread.

## Related code files
- Create: `src/project.rs`, `src/export.rs`.
- Modify: `Cargo.toml` (+`serde` derive, `serde_json`), `src/main.rs` (`mod project; mod export;`).

## Implementation Steps
1. Add serde deps; `project.rs` structs + load/save + `offset_s` + `normalize_bubble`.
2. `export.rs`: `fit_even`, circle/waveform snippet helpers (private fns returning `String`, shared by both branches), `ffmpeg_args`.
3. `parse_progress`, `tool`, `run`.
4. Unit tests (in-file `#[cfg(test)]`, synthetic `Project` values, substring asserts on args):
   - project: roundtrip full + minimal (mic/camera None); normalize inside, overflow clamped, tiny region (bubble > frame).
   - args: camera variant (cam +0.5 → `-itsoffset 0.500` before its `-i`; mic −0.2 → `-ss 0.200`; `geq`, `amix=inputs=2:normalize=0`, `-t`); no-camera variant (`showwaves`, `asplit`); no mic/cam/sys → no `overlay`, `-an`; sys-only → `[0:a]`, no `amix`; 5K → `scale=3840:2160` and bubble px from scaled dims.
   - `fit_even`: 5120x2880→3840x2160, 2560x1600 unchanged, odd→even, portrait.
   - `parse_progress`: value, end, garbage.
5. `#[ignore] export_runs_on_synthetic_inputs`: temp dir (`std::env::temp_dir()` + pid); ffmpeg lavfi makes sparse VFR `screen.mp4` (testsrc2 r=2 + sine, hevc_videotoolbox), `camera.mov`, `mic.wav`; write project.json with offsets; `run`; ffprobe: 1 video + 1 audio, duration within 0.1s of `duration_s`, frames ≈ duration·FPS.

## Todo list
- [x] project.rs model + load/save + offset_s + normalize_bubble
- [x] export.rs ffmpeg_args + fit_even
- [x] parse_progress + tool + run
- [x] unit tests green (`cargo test`)
- [x] ignored integration test green (`cargo test -- --ignored`)

## Success Criteria
- `cargo test` passes with no ffmpeg dependency; `cargo test -- --ignored` passes on dev box.
- Every branch of `ffmpeg_args` (camera / waveform / none × 0/1/2 audio × ± offsets) hit by ≥1 test.

## Risk Assessment
| Risk | L×I | Signal it broke | Response |
|---|---|---|---|
| Real SCK files differ from synthetic (10-bit HEVC, audio start_time ≠ 0, edit lists) | M×M | phase 5 export fails/misaligned while ignored test passes | fix in `ffmpeg_args` during phase 5 (adjust) |
| `geq` too slow for long 4K exports | L×M | export ≫ realtime | precompute mask once + `alphamerge` (adjust) |
| amix normalize=0 clipping | L×L | audible distortion in phase 5 | append `alimiter` (adjust) |
| `-t` from `duration_s` disagrees with file | L×L | last frame repeated/cut | use ffprobe duration in `run` (adjust) |

## Security Considerations
- No shell: `Command::new(path).args(vec)`; filtergraph built only from numbers/fixed literals, file paths passed as separate args → no injection.
- `tool()` searches fixed dirs + PATH; dev-only Homebrew ffmpeg (GPL) — not redistributed (bundling deferred).

## Next steps
Phase 3 uses `Project`, `normalize_bubble`, `FPS`, `tool("ffprobe")`.
