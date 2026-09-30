# Export compositing research (macOS Rust recorder)

Saved by orchestrator from researcher hand-back (agent did not write file).

## Verdict
v1 = **Option A: ffmpeg CLI + filter_complex, driven by project.json**. Rank A > C > D > B.
Local check: ffmpeg 8.0.1 (Homebrew, GPL, --enable-videotoolbox). `h264_videotoolbox`, `hevc_videotoolbox` present; filters geq, alphamerge, overlay, amix, showwaves, showfreqs, showvolume, avectorscope present.

## Verified commands (synthetic inputs, M4, 2560x1600)
Both ran OK. Camera variant 4s export ≈1.4s wall. `-progress pipe:1` emits `out_time=`, `progress=end`.

Camera variant (bubble 512px at x=0.75,y=0.7; cam +0.5s, sys +0.25s):
```
ffmpeg -y -progress pipe:1 -nostats \
 -i screen.mov -itsoffset 0.5 -i cam.mov -i mic.wav -itsoffset 0.25 -i sys.wav \
 -filter_complex "\
[1:v]scale=512:512:force_original_aspect_ratio=increase,crop=512:512,format=yuva420p,\
geq=lum='p(X,Y)':cb='p(X,Y)':cr='p(X,Y)':a='if(lt(hypot(X-256,Y-256),256),255,0)'[cam];\
[0:v][cam]overlay=x=W*0.75:y=H*0.7:format=auto[v];\
[2:a]aresample=48000[a1];[3:a]aresample=48000[a2];\
[a1][a2]amix=inputs=2:normalize=0:duration=longest[a]" \
 -map "[v]" -map "[a]" -c:v h264_videotoolbox -b:v 12M -pix_fmt yuv420p -tag:v avc1 \
 -c:a aac -b:a 192k -movflags +faststart out.mp4
```

No-camera variant (mic waveform masked to circle):
```
[1:a]asplit=2[m1][m2];
[m2]showwaves=s=512x512:mode=cline:rate=30:colors=white,format=yuva420p,
 geq=lum='p(X,Y)':cb='p(X,Y)':cr='p(X,Y)':a='if(lt(hypot(X-256,Y-256),256),255,0)'[vis];
[0:v][vis]overlay=x=W*0.75:y=H*0.7[v];
[m1][2:a]amix=inputs=2:normalize=0[a]
```
Caveat: showwaves on black = black disc; needs background color (`color` source + overlay/blend) for styled bubble. `avectorscope=mode=polar` also works (radial look).

## Option A details
- Mask: `format=yuva420p,geq(a=...)` works; aliased edge. Better: `alphamerge` with pre-generated antialiased circle mask PNG (gen once per export, `image` crate or lavfi). Soft edge expr `clip((R-hypot)*2*255,0,255)` (untested). Border/shadow: overlay pre-rendered ring PNG (~10 lines).
- Audio: `amix` divides by N unless `normalize=0` (ffmpeg ≥4.4). Add `alimiter` vs clipping. Use amix, NOT amerge (stacks channels).
- Alignment: `-itsoffset` per input works for audio+video. Offset video lengthens output → `apad` + `-shortest`/`-t`. Finer: `adelay`, `setpts=PTS+S/TB`. Late-starting screen → `-ss`/`atrim`.
- Retina: decode fine. `-hwaccel videotoolbox` frames need `hwdownload` before geq; benchmark sw decode first.
- VT flags: `-b:v`, `-q:v` (newer), `-realtime 0`, `-allow_sw 0`, `-profile:v high`, `-tag:v hvc1` for HEVC.
- Progress: parse `out_time_us=` / total duration from project.json; cancel = kill child.
- Distribution: Homebrew ffmpeg is GPL → dev-only. Release: bundle **LGPL** static build (`--enable-videotoolbox`, no gpl/x264; ~20-30MB) built in CI; separate process = aggregation; ship license + source offer; codesign/notarize in `Contents/Resources`. Legal read advised.

## Option B: ffmpeg-next / rsmpeg
Needs pkg-config + linkable libav; breaks across ffmpeg majors; ~400-800 lines vs ~60-100 for CLI arg builder; same license issue. Rejected.

## Option C: AVFoundation (cidre/objc2)
Built-in CIFilter composition handles single source only; overlaying 2nd track needs custom `AVVideoCompositing` ObjC class from Rust. ~600-1200 lines FFI; waveform +~300 (CG per frame). cidre = single maintainer, coverage of AVMutableVideoComposition unverified. Upgrade target only if ffmpeg licensing blocks.

## Option D: wgpu/Metal + VideoToolbox (Cap-like)
Decode, textures, WGSL mask/border/shadow shader, IOSurface → VT encode, AAC, mux, sync, mixer. ~2000-4000 lines. Best for future editor live preview. Overkill for v1.

## Cap (CapSoftware/Cap)
License mixed: mostly **AGPLv3**; `cap-camera*`, `scap-*` crates **MIT** (reusable, but capture not export). Tauri v2 + Rust 1.88+ + SolidStart. Export: wgpu renderer (`cap-rendering`, unverified) + ffmpeg/VT encode. Do not copy AGPL code; study architecture only.

## Trade-off matrix
| | A ffmpeg CLI | B ffmpeg-next | C AVFoundation | D wgpu+VT |
|---|---|---|---|---|
| Code (est.) | ~100 | 400-800 | 600-1200 | 2000-4000 |
| Speed M4 | high (VT) | high | high | highest |
| Waveform | easy (filter) | easy | hard | custom shader |
| Build/dist pain | bundle binary | high | none | medium |
| Editor-ready | params→graph | same | partial | best |
| License risk | LGPL build ok | LGPL/GPL | none | none |

## Recommendation / upgrade path
1. v1: Rust builds ffmpeg args from project.json (offsets→`-itsoffset`, bubble x/y/size→scale+overlay, cam present→camera branch else waveform branch). Progress via `-progress pipe:1`. Dev: Homebrew ffmpeg on PATH.
2. Editor v2: trim→`-ss/-t`/`trim`/`atrim`; bubble move/zoom→expression overlay, `zoompan`, `sendcmd`.
3. Upgrade trigger: live preview / complex animation → Option D, same project.json scene model.
4. Keep filtergraph generator behind one function.

## Limitations
Synthetic inputs only (no real Retina HEVC / camera); output not visually inspected; Cap internals, cidre coverage, static-build licenses not triple-verified; hwaccel+geq untested.

## Unresolved questions
- Bundle ~25MB LGPL ffmpeg for distribution? Who owns build CI?
- Can audio start before screen (negative offsets)? Affects itsoffset/apad logic.
- Bubble style (border, shadow, waveform look) — design decision.
- Export native Retina res or downscale to 1080p?
