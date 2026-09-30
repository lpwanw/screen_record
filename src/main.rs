mod app;
mod bubble;
mod camera;
mod export;
mod macos;
mod mic;
mod project;
mod recording;
mod region_overlay;
mod screen;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use gpui_component::Root;

fn main() {
    gpui_platform::application().run(|cx: &mut App| {
        gpui_component::init(cx);
        let bubble = bubble::Bubble::open(cx).expect("open bubble window");
        let bounds = Bounds::centered(None, size(px(440.), px(620.)), cx);
        let panel = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Taywind Record".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    let panel = cx.new(|cx| app::Panel::new(bubble, window, cx));
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
        cx.activate(true);
    });
}
