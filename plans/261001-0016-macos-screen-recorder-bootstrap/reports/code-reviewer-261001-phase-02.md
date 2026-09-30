# Phase 02 review: project model + ffmpeg export

Scope: src/project.rs, src/export.rs, screen.rs/main.rs/Cargo.toml diffs. Read-only.

## Medium
1. Mic-only audio that starts after screen (system_audio=false, mic later, N=1) is not gap-filled.
   `-itsoffset` + `aresample=48000` + single-input `amix` + `apad` only pads the tail.
   Verified (ffmpeg, itsoffset 1.0, -t 3, aac/mp4): audio stream start_time=0.978, duration=2.03; video start 0.
   Sync depends on the player honoring the stream start offset/edit list; some editors/players shift audio to 0.
   N=2 is fine (screen audio anchors the mix; output start_time=0, verified).
   Fix (verified): `aresample=48000:async=1:first_pts=0` in the per-source chain -> audio start_time=0, first 1.0s silent.
   Apply to all sources (harmless for N=2).
2. No test covers a track starting LATER than screen for audio, and no integration test for later camera.
   Unit test only asserts arg order; integration fixture uses mic earlier (9.8 vs 10.0) and camera later but never checks alignment.
   That's how #1 slipped. Suggest: integration case system_audio=false, camera=None, mic later; probe audio stream start_time ~0 via ffprobe.

## Low
3. `run`: `line?` on the stdout reader returns early on a read error (non-UTF8) and leaves ffmpeg running, un-waited, orphaned.
   Progress output is ASCII so unlikely; same for a panicking `on_progress`. Cheap fix: use `lines().map_while(Result::ok)` or kill/wait on error.
4. `even` doc says "rounded down" but it rounds to nearest then clears low bit (1439.6 -> 1440, 1443.0 -> 1442, 1441.4 -> 1440). Doc is misleading; behavior fine.
   Consequence: diameter can exceed d*width by <2px, x+diameter may overflow frame by 1-2px; overlay clips, invisible.
5. `normalize_bubble` panics on clamp (`min <= max` assert) if frame.w or frame.h is 0/NaN. Unrealistic input; ignore unless rects come from untrusted JSON.
   bubble.w=0 gives d=0 -> ffmpeg `scale=0:0` fails (error surfaces via log, OK).
6. `parse_progress`: duration_s=0 -> NaN -> `as f32` = 0, no panic. Fine. Negative/N/A out_time_us handled.
7. amix normalize=0 with two loud sources can clip (sum > 1.0) before AAC. Conscious decision; consider `alimiter` later if users report distortion.
8. Overlay: camera/waveform shorter than screen freezes on last frame (eof_action=repeat default) rather than hiding the bubble. By design, note for UX.

## Checked, no issue
- Filter labels: [s],[b],[v],[bg],[wave],[ma],[mw],[a0],[a1],[a] unique; each stream used once; indices correct for camera/mic/none x 0/1/2 audio (camera idx1, mic idx2 or 1).
- `-ss` before `-i` for earlier tracks, `-itsoffset` for later; 0.001 threshold and `{:.3}` formatting OK. Earlier mic N=1: duration exactly 3.0 verified.
- `-t` is an output option (after all -i), with apad gives exact duration. 
- Deadlock: stderr->file, stdout fully drained, stdin null. No risk.
- Paths with spaces: passed via Command::args, no paths inside filtergraph. Safe. `display()` is lossy only for non-UTF8 paths.
- normalize_bubble math: d bounded by width fraction and height; d_y = d*w/h consistent with export x/y/diameter derivation. Clamp range never inverted (d<=1).
- Float tests with exact assert_eq pass deterministically (same arithmetic); brittle if formula reordered, acceptable.
- Cargo: serde derive + serde_json only. `even`/`FPS` move is DRY.

## Metrics
cargo test 13/13 (per caller); clippy only dead-code; fmt clean. Independent ffmpeg probes run in /tmp/rv only.

## Unresolved
- Is audio start_time != 0 in mp4 acceptable for target players (QuickTime/browser)? If not, fix #1 is required; it is 1 line.

Status: DONE_WITH_CONCERNS
