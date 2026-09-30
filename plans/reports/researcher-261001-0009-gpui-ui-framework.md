# GPUI viability for macOS screen-recorder UI (2026-10-01)

## TL;DR
Recommend **Tauri v2** (rank 1) for shipping speed; **GPUI** (rank 2) viable but every hard requirement (circle video, exclusion-from-capture, click-through) needs raw AppKit/objc2 glue; **egui** rank 3. GPUI cannot clip `surface()` to a circle -> must use `img()` path (CPU copy per frame). Any choice needs rustc upgrade.

## 1. GPUI distribution (verified via crates.io API + zed main source)
- `gpui` on crates.io = **0.2.2, published 2025-10-22**, edition 2024, 314k dl. Stale vs zed main (same version string but main has new APIs: `gpui_platform` crate split, `AnchoredPopup`, `app_owns_titlebar_drag`). Examples on main use `gpui_platform::application()` - not compatible with 0.2.2.
- New channel: **`gpui-pre` 0.3.7 (2026-09-28)** + `gpui-pre-platform` 0.3.7, macro/web crates; "snapshot of zed@<sha>" releases, ~weekly (0.3.0 -> 0.3.7). Package is renamed (`gpui = { package = "gpui-pre", version = "=0.3.7" }`). Exact-pin required: gpui-component pins `=0.3.7` and notes "any snapshot may change GPUI's API".
- Alt: git dep on zed repo (works, heavy clone, repo pins toolchain **1.98.1**).
- **API stability: none guaranteed.** gpui.rs: "GPUI is tied to Zed, contributions made there". Expect breaking renames per snapshot. Pin exact, upgrade deliberately.
- gpui-component (longbridge): **0.7.0, 2026-09-28**, 157k dl, 60+ widgets (buttons, select/dropdown, switch, etc.), active; tracks gpui-pre snapshots. Useful for control panel (pickers, toggle). Adds dep weight.
- Official examples: `crates/gpui/examples/` (hello_world, window, window_positioning, window_movable, image_loading, painting, opacity, animation, ...). No official `create-gpui-app` confirmed in my search (unresolved; community scaffolds exist).
- Sources: https://crates.io/crates/gpui , https://crates.io/crates/gpui-pre , https://github.com/zed-industries/zed/tree/main/crates/gpui , https://raw.githubusercontent.com/longbridge/gpui-component/main/Cargo.toml , https://gpui.rs/

## 2. Window capabilities on macOS (source: crates/gpui/src/platform.rs, crates/gpui_macos/src/window.rs)
| Need | GPUI | Verdict |
|---|---|---|
| Multiple windows | `cx.open_window` repeatedly; example `move_entity_between_windows` | OK |
| Borderless | `WindowOptions.titlebar: None` | OK |
| Transparent bg | `window_background: WindowBackgroundAppearance::Transparent` (+ `setOpaque`) | OK |
| Always-on-top | `WindowKind::PopUp` -> NSPanel, nonactivating, level 101 (NSPopUpMenuWindowLevel), CanJoinAllSpaces+FullScreenAuxiliary. `Floating` = level 3 NSFloatingWindowLevel | OK; PopUp is best for bubble + overlay (above menu bar, over fullscreen apps) |
| Drag w/o titlebar | `Window::start_window_move()` on mouse_down; set `app_owns_titlebar_drag: true` (avoids macOS 27 titlebar click delay - comment in source mentions macOS 27) | OK |
| Overlay on specific display | `display_id: Some(id)` from `cx.displays()` + `WindowBounds::Windowed(display.bounds())`; `window_positioning` example does exactly this | OK; verify it covers menu bar/notch area (unverified) |
| Click-through | **No** `ignoresMouseEvents` in gpui_macos (grep empty) | needs objc2 call on NSWindow |
| Exclude from capture | **No** `sharingType` API (grep empty). `impl HasWindowHandle for Window` returns `AppKitWindowHandle` = **NSView ptr**; `[view window]` -> NSWindow -> `windowNumber` (= SCWindow.windowID) or `setSharingType:NSWindowSharingNone`. | Feasible, ~15 lines objc2 unsafe. Preferred: pass windowNumber to SCContentFilter `excludingWindows` |
- Overlay drag-select: fullscreen PopUp + `on_mouse_down/move/up` + `canvas()` rect. Fine.
- Caveat: PopUp = nonactivating panel; keyboard focus/text input in it is limited (fine for bubble/overlay, control panel should be `Normal`).

## 3. Live video in GPUI
- `surface(CVPixelBuffer)` (macOS only, core-video 0.5.2): zero-copy to Metal; ideal for AVFoundation/SCK frames. **But**: source has `// TODO: Add support for corner_radii` in surface.rs; `paint_surface` only takes bounds + rectangular content_mask -> **cannot clip to circle**. Workarounds: (a) overlay a masking quad/ring on top (only works if bubble bg is opaque/solid - bubble is transparent over desktop, so fails); (b) patch gpui fork to add rounded surface shader; (c) use `img()`.
- `img(Arc<RenderImage>)`: `paint_image(bounds, new_bounds, corner_radii, ...)` honors `rounded_full()` -> circle works. Per frame: BGRA->`RenderImage` (Arc<Frame<BGRA>>), CPU copy ~240x240x4 = 230KB x 30fps = 7MB/s, trivial on M4. Must call `window.drop_image(old)` for each replaced frame or atlas grows unboundedly (unique image_id per frame). Verify that cost of atlas alloc/remove per frame is fine (untested).
- Waveform: `canvas(prepaint, paint)` + `PathBuilder::stroke/fill` (`move_to/line_to/curve_to`) + `window.paint_path`, or many `paint_quad` bars. Easy; repaint via `cx.notify()` on timer/channel. Bars in a circle clip: draw with quads, rounded container.
- Threading: frames arrive on AVFoundation queue -> channel -> `cx.spawn`/`cx.notify()`. Entities are !Send; fine via async tasks.

## 4. Pain points
- Docs: API docs thin; learn from Zed source + examples. No stable guide; AI/LLM training data stale (API churn).
- Build: Metal shader compile at build needs full Xcode (`xcrun metal`); gpui-component uses `runtime_shaders` feature to avoid this. Big dependency tree, slow cold build.
- Permissions: camera/mic/screen-recording TCC attach to the **responsible process**. `cargo run` from Terminal: Terminal/iTerm gets the prompt; camera access from unbundled binary without `NSCameraUsageDescription` can crash/abort (AVCaptureDevice requires usage string in Info.plist of the process; general macOS rule, see Qt list https://lists.qt-project.org/pipermail/interest/2020-January/034380.html). Practical: use a `.app` bundle from day 1 (or embed plist via linker `-sectcreate __TEXT __info_plist`) + ad-hoc codesign with entitlements (camera, audio-input). Needed keys: NSCameraUsageDescription, NSMicrophoneUsageDescription; Screen Recording has no plist key (TCC prompt), re-grants needed after rebuild unless stable signing identity.
- Bundling: `cargo-bundle` / `cargo-packager` (CrabNebula) both work for arbitrary Rust binaries; Zed itself uses custom scripts. Not GPUI-specific. Not verified end-to-end here.

## 5. Comparison for THIS app
| Dim | GPUI | egui/eframe 0.36.2 | Tauri 2.12 |
|---|---|---|---|
| Circle transparent always-on-top window | PopUp + Transparent: native | `ViewportBuilder::with_transparent/with_always_on_top/with_decorations`; multi-viewport; macOS transparency works but glitchy history, frame-clear quirks (from memory, unverified) | `transparent:true` needs `macOSPrivateApi` (App Store incompat, fine here); CSS `border-radius:50%` circle trivial; always_on_top native |
| Live camera frames in circle | `img()` rounded_full OK; `surface()` no rounding | texture upload per frame, round via mesh/`Image::corner_radius`; OK | `<video>` + getUserMedia in WKWebView (needs plist+entitlement), or canvas fed from Rust; CSS circle free |
| Waveform | canvas/path | egui_plot/painter, trivial | canvas/SVG/WebAudio AnalyserNode, trivial |
| Fullscreen overlay drag-select | good | ok, multi-viewport transparent fullscreen fiddly | good (HTML/JS drag rect) |
| Exclude window from capture | objc2 glue (windowNumber) | `RawWindowHandle` NSView via winit -> same glue | `set_content_protected(true)` built-in (sets NSWindowSharingNone) + `ns_window()` handle |
| Control panel widgets | gpui-component (good) | built-in, basic look | HTML/CSS any UI lib (bun fits) |
| Future editor/timeline | Strong: custom rendering, perf (Zed-grade) but build all widgets | Weak-mid: immediate mode, custom timeline doable, styling limited | Strong via web ecosystem (canvas/WebCodecs), but video preview perf through webview meh |
| Maturity/MSRV | pre-1.0 snapshots, weekly breaking | very mature (19M dl), MSRV 1.95 | mature v2, MSRV 1.90 (2.12.0); v3 alpha exists |
| Stack fit | pure Rust, Mac-native feel | pure Rust | Rust core + TS frontend; bun allowed |
- Prior art: CapSoftware/Cap (open-source Tauri+Rust screen recorder with camera bubble) validates Tauri path. https://github.com/CapSoftware/Cap
- Tauri sources: https://v2.tauri.app/ , https://github.com/tauri-apps/tauri/issues/13415 (transparency lost after bundling on macOS - **risk to test early**).

## Ranked recommendation
1. **Tauri v2**: circle window, transparency, capture exclusion, overlay all built-in/CSS; least unsafe code. Risks: transparency-after-bundle bug (#13415), WKWebView camera permission handling, JS layer (user has bun).
2. **GPUI**: choose only if pure-Rust/native polish matters. Cost: pin `gpui-pre` exact, objc2 glue for exclusion/click-through, `img()` per-frame upload with `drop_image`, rebuild churn each upgrade.
3. **egui**: fastest to prototype but weakest polish for transparent circular windows + future editor.
If GPUI chosen: spike first (1 day): PopUp transparent circular `img()` bubble at 30fps + objc2 windowNumber -> verify no atlas leak, no frame hitch.

## 6. Rust version
Dev 1.88.0 **must upgrade**. zed main pins 1.98.1; gpui-pre has no declared `rust-version` (MSRV unverified; expect recent stable since it tracks zed). egui/eframe 0.36.2 need 1.95; tauri 2.12.0 needs 1.90 (2.11.6: 1.77.2). Run `rustup update stable` regardless of choice.

## Limitations
No code compiled/spiked; all GPUI API claims from source reading of zed main (not the 0.3.7 snapshot tag; minor drift possible). egui behaviors not source-verified. No benchmarks. Couldn't confirm create-gpui-app.

## Unresolved
- Exact MSRV of gpui-pre 0.3.7 (try build on stable).
- Does `surface()` get rounded corners in snapshot > main? (TODO still present on main.) Patch feasibility in a fork?
- PopUp overlay covering menu bar/notch on M4 display; click-through on bubble needed?
- Does `cx.displays()` bounds map 1:1 to ScreenCaptureKit display coordinates (Retina scale)?
- Tauri #13415 status in 2.12; WKWebView getUserMedia from bundled app.
- Is `gpui-pre` an officially sanctioned Zed channel (naming suggests so; no announcement found).
