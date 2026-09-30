# gpui-pre 0.3.7 / gpui-component 0.7.0 API facts (saved by orchestrator from researcher hand-back)

Source: crate tarballs (static.crates.io) + crates.io API. Nothing compiled. [U] = unverified.

## 1. Cargo
- gpui-pre 0.3.7: edition 2024, NO `rust-version`. Defaults: font-kit, wayland, x11, windows-manifest. Other: `screen-capture` (scap), inspector, profiler, test-support.
- `runtime_shaders` lives on `gpui-pre-platform` 0.3.7 (lib `gpui_platform`, provides `application()`), defaults `[]`. Whether it avoids full Xcode `metal` [U].
- gpui-component 0.7.0: edition 2024, depends `gpui = { version = "=0.3.7", package = "gpui-pre" }`, gpui-base, gpui-kit-assets. Project rebranding to "gpui-kit" (crate `gpui-kit` also exists).
```toml
gpui = { package = "gpui-pre", version = "=0.3.7" }
gpui_platform = { package = "gpui-pre-platform", version = "=0.3.7", features = ["font-kit", "runtime_shaders"] }
gpui-component = "=0.7.0"
raw-window-handle = "0.6"
```

## 2. Entry
```rust
use gpui_platform::application;
application().run(|cx: &mut App| {
    gpui_component::init(cx);           // lib.rs:127, before any component
    let bounds = Bounds::centered(None, size(px(500.), px(500.)), cx);
    cx.open_window(
        WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
        |_, cx| cx.new(|_| View),
    ).unwrap();
    cx.activate(true);
});
```
- README of 0.7.0 shows `gpui_kit::application()/init/open_window` (auto-mounts `Root`). Whether gpui-component needs manual `Root::new(...)` wrapper [U].
- hello_world calls `example_support::load_fonts(cx)` — font loading may be needed [U].

## 3. WindowOptions (platform.rs:2174)
Fields: window_bounds, titlebar: Option<TitlebarOptions>, focus, show, kind: WindowKind, is_movable, app_owns_titlebar_drag, inactive_frame_interval, is_resizable, is_minimizable, display_id: Option<DisplayId>, window_background: WindowBackgroundAppearance, app_id, window_min_size.
- WindowKind: Normal, PopUp, AnchoredPopup(PopupOptions), Floating.
- WindowBackgroundAppearance: Opaque (+ Transparent/Blurred [U]).
- `Window::start_window_move(&self)`, `Window::bounds()`.
- `App::displays() -> Vec<Rc<dyn PlatformDisplay>>`, `primary_display()`, `find_display(id)`; `PlatformDisplay::id()`, `bounds()`.

## 4. Images
- `RenderImage::new(impl Into<SmallVec<[image::Frame; 1]>>)`; repo usage `RenderImage::new(SmallVec::from_const([Frame::new(buffer)]))` (svg_renderer.rs:216). BGRA vs RGBA order [U] — read svg_renderer before feeding frames.
- `img(impl Into<ImageSource>)`; `ImageSource::Render(Arc<RenderImage>)`.
- `Window::drop_image(&mut self, Arc<RenderImage>) -> Result<()>`.
- `rounded_full()` likely macro-generated [U].

## 5. Canvas / events / async
- `canvas(prepaint: FnOnce(Bounds, &mut Window, &mut App) -> T, paint: FnOnce(Bounds, T, &mut Window, &mut App))`.
- `PathBuilder::stroke(px)`, `move_to`, `line_to`, `build() -> Result<Path<Pixels>>`; `window.paint_path(path, color)` [U].
- Mouse: `on_mouse_down(MouseButton, Fn(&MouseDownEvent, &mut Window, &mut App))`, `on_mouse_up`, `on_mouse_move` [U].
- Async: `cx.spawn(async move |this, cx| { this.update(cx, |v, cx| { ..; cx.notify() }).ok(); })` (AsyncFnOnce(WeakEntity<T>, &mut AsyncApp)). `AsyncApp::update` NOT fallible in this snapshot.

## 6. Native handle
- `impl HasWindowHandle for Window` (window.rs:7360). Inherent `Window::window_handle()` returns AnyWindowHandle and SHADOWS trait → call `raw_window_handle::HasWindowHandle::window_handle(&*window)` explicitly. Expect `RawWindowHandle::AppKit(h)` → `h.ns_view` [U].

## 7. gpui-component widgets
- `Button::new(id).primary().label(..).on_click(|_, _, _| ..)`.
- `Switch::new(id).checked(bool).on_click(..)` (handler sig [U]); `Checkbox::new(id).checked(bool).on_click(Fn(&bool, &mut Window, &mut App))`.
- Select: `SelectState::new(delegate, Option<IndexPath>, window, cx)` + `Select::new(&Entity<SelectState<D>>)`, `D: SearchableListDelegate` [U trait shape]. Also Combobox, DropdownButton.

## Risk
Very new crates, gpui-component mid-rebrand. Exact pins mandatory; commit Cargo.lock; keep GPUI calls in UI module only.

## Unresolved
1. open_window vs Root::new wrapper in gpui-component 0.7.0?
2. runtime_shaders avoids full Xcode?
3. RenderImage byte order / premultiplied alpha?
4. Switch::on_click sig, SearchableListDelegate?
5. Transparent variant name, rounded_full, paint_path, on_mouse_move?
6. MSRV of transitive deps?
7. ns_view extraction?
