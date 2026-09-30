---
title: "Phase 5: Integration + verification"
status: pending
effort: 6h
---

# Phase 5: Integration + verification

## Context links
- Parent: [plan.md](./plan.md). Deps: phases 1-4. Blocks: none.
- Docs: `docs/tech-stack.md`, `../reports/researcher-261001-0009-macos-capture-stack.md` (Q6 sync), phase 2 Key Insights.

## Overview
- Date: 2026-10-01. Priority: P2. Implementation: Pending. Review: Not reviewed.
- End-to-end manual matrix on real hardware, objective A/V sync measurement (flash/beep), bubble position/exclusion checks, calibration if needed, docs.

## Key Insights
- Sync measured objectively, verified 2026-10-01: synthetic clip with 0.1s white flash + 1kHz beep every 2s; `blackdetect` `black_end` = flash onset, `silencedetect` `silence_end` = beep onset. On the clip itself: 2.000/4.000 vs 2.005/4.011 → ~10ms measurement resolution.
- Main display 5K → export exercises the fit-to-3840x2160 path; second display 2560x1600 exercises the no-scale path.
- Calibration is a knob, not a guess: add per-track bias constants only if measured bias is constant across runs.

## Requirements
- Acceptance 1-8 (plan.md) demonstrated and recorded in the results table below.
- Docs: dev setup + run instructions; tech-stack deltas from this plan.

## Architecture
Verification only; no new modules. Measurement flow:
```
sync.mp4 (scratch, not in repo) → QuickTime on screen + speakers + camera aimed at screen
  → app records (mode/cam/sys per matrix) → project dir → Export → export.mp4
  → ffmpeg blackdetect (cropped to clip area) + silencedetect → |flash - beep| per pair, start vs end
```
Clip: `ffmpeg -f lavfi -i "color=black:s=1280x720:r=60:d=330,drawbox=c=white:t=fill:enable='lt(mod(t,2),0.1)'" -f lavfi -i "sine=f=1000:sample_rate=48000:d=330,volume=0:enable='gte(mod(t,2),0.1)'" -c:v h264_videotoolbox -b:v 5M -c:a aac -shortest sync.mp4`
Measure: `ffmpeg -i export.mp4 -an -vf "crop=W:H:X:Y,blackdetect=d=0.02:pix_th=0.1" -f null -` and `ffmpeg -i export.mp4 -vn -af "silencedetect=n=-30dB:d=0.05" -f null -`.
- screen↔system audio: mic None, sys on. screen↔mic: sys off, mic on (speaker → mic). camera↔screen: crop bubble area, `blackdetect pix_th=0.3`, or frame-step in ffplay.

## Related code files
- Create: `README.md` (setup, run, test).
- Modify: `docs/tech-stack.md` (HEVC raw screen track; export fit ≤3840x2160 + CFR `fps`; ffmpeg path resolution; 60fps). `src/recording.rs` only if calibration constants needed; `src/export.rs` only for real-file fixes found here.

## Implementation Steps
1. Quality gates: `cargo fmt --check`, `cargo clippy`, `cargo test`, `cargo test -- --ignored`.
2. Fresh permissions: `tccutil reset All dev.taywind.record`, `scripts/bundle.sh` → accept prompts (acceptance 1).
3. Matrix (below), ~20s each; per run: ffprobe tracks, `project.json` sanity, frame grab of `screen.mp4` at bubble spot, export + play in QuickTime.
4. Bubble placement: drag bubble to top-left, then bottom-right corner of captured area before Stop → export frame shows bubble at same corner.
5. Sync: 20s runs for 3 pairs; one 5-min run (window or region on clip, camera + mic + sys) → compare offsets first 10s vs last 10s.
6. Calibrate/fix per Risk table; re-run affected cells.
7. Edge runs: camera denied (waveform fallback), mic None + camera None (export has no overlay, `-an` if sys off), region on second display, window moved during recording.
8. Docs: README + tech-stack deltas. Commit (conventional, no AI refs).

## Test matrix (fill in)
| Mode | Camera | Sys audio | Tracks ok | Bubble absent in raw | Export plays + bubble pos | Notes |
|---|---|---|---|---|---|---|
| display (5K) | on | on | | | | |
| display (5K) | on | off | | | | |
| display (5K) | off (waveform) | on | | | | |
| display (5K) | off (waveform) | off | | | | |
| window | on | on | | n/a | | |
| window | on | off | | n/a | | |
| window | off | on | | n/a | | |
| window | off | off | | n/a | | |
| region (2nd display) | on | on | | | | |
| region (2nd display) | on | off | | | | |
| region (main) | off | on | | | | |
| region (main) | off | off | | | | |

| Sync pair | Start Δ (ms) | End Δ @5min (ms) | Pass < 100 |
|---|---|---|---|
| screen ↔ system audio | | | |
| screen ↔ mic | | | |
| screen ↔ camera | | | |

Unit/integration: `cargo test` (project roundtrip, normalize, args ×5, fit, progress) + `cargo test -- --ignored` (lavfi export). E2E: tables above.

## Todo list
- [ ] Quality gates green
- [ ] Permissions prompt on fresh TCC
- [ ] 12-cell matrix filled
- [ ] Bubble corner placement verified
- [ ] Sync table filled, all < 100ms
- [ ] Edge runs done
- [ ] README.md + docs/tech-stack.md updated

## Success Criteria
- All matrix cells pass; sync table all < 100ms start and end; `cargo test` + ignored test green; README lets a fresh clone reach a running .app.

## Risk Assessment
| Risk | L×I | Signal it broke | Response |
|---|---|---|---|
| Constant per-track start bias (file start ≠ stamped start) | M×M | same Δ sign/size (≥40ms) across runs | add `<TRACK>_START_BIAS_NS` const in `recording.rs` (adjust) |
| Mic clock drift vs host | L×M | end Δ − start Δ > 100ms over 5 min | store mic sample count + host span in project.json, `atempo` in export (+3h, adjust) |
| Random Δ > 500ms run-to-run | L×H | Δ varies wildly | STOP & replan stamping (e.g. SCStream→AVAssetWriter for exact PTS) |
| Real SCK file quirks break export | M×M | ffmpeg error in `export.log` | fix `ffmpeg_args` + add unit case (adjust) |
| Bubble offset in export | L×M | corner test misses by > few px | fix coordinate mapping (phase 4 risk) (adjust) |
| Export slower than realtime at 4K | M×L | wall time > duration | accept v1 / mask-once `alphamerge` (adjust) |

## Security Considerations
- Sync clip + test recordings stay in scratch / `~/Movies/Taywind`; none committed.
- README documents identity creation without shipping keys; notes Homebrew ffmpeg is GPL, dev-only.

## Next steps
Deferred per tech-stack: bundled LGPL ffmpeg for distribution, editor on same `project.json`.
