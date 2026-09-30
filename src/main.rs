mod app;
mod bubble;
mod camera;
mod export;
mod macos;
mod project;
mod screen;

use gpui::{App, AppContext, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);
        let slot = camera::FrameSlot::default();
        let bounds = Bounds::centered(None, size(px(420.), px(560.)), cx);
        let panel = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    let panel = cx.new(|_| app::Panel::new(slot.clone()));
                    cx.new(|cx| Root::new(panel, window, cx))
                },
            )
            .expect("open control panel window");
        let panel_id = panel.window_id();
        cx.on_window_closed(move |cx, closed| {
            if closed == panel_id {
                cx.quit();
            }
        })
        .detach();
        bubble::Bubble::open(slot, cx).expect("open bubble window");
        cx.activate(true);
    });
}
