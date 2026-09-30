use std::{sync::Arc, time::Duration};

use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, RenderImage, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, div, img, point, px, rgb, size,
};
use image::{Frame as ImageFrame, RgbaImage};

use crate::camera::FrameSlot;

pub const SIZE: f32 = 200.;
const MARGIN: f32 = 40.;

pub struct Bubble {
    slot: FrameSlot,
    image: Option<Arc<RenderImage>>,
    stale: Option<Arc<RenderImage>>,
}

impl Bubble {
    pub fn open(slot: FrameSlot, cx: &mut App) -> anyhow::Result<WindowHandle<Bubble>> {
        let display = cx
            .primary_display()
            .expect("a display is connected")
            .bounds();
        let origin = point(
            display.origin.x + px(MARGIN),
            display.origin.y + display.size.height - px(SIZE + MARGIN),
        );
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    origin,
                    size(px(SIZE), px(SIZE)),
                ))),
                titlebar: None,
                kind: WindowKind::PopUp,
                window_background: WindowBackgroundAppearance::Transparent,
                focus: false,
                is_resizable: false,
                is_minimizable: false,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    Self::pump(cx);
                    Self {
                        slot,
                        image: None,
                        stale: None,
                    }
                })
            },
        )
    }

    fn pump(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let updated = this.update(cx, |bubble, cx| {
                    if bubble.slot.lock().unwrap().is_some() {
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn take_frame(&mut self) {
        let Some(frame) = self.slot.lock().unwrap().take() else {
            return;
        };
        let Some(buffer) = RgbaImage::from_raw(frame.width, frame.height, frame.bgra) else {
            return;
        };
        let image = Arc::new(RenderImage::new([ImageFrame::new(buffer)]));
        self.stale = self.image.replace(image);
    }
}

impl Render for Bubble {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(old) = self.stale.take() {
            window.drop_image(old).ok();
        }
        self.take_frame();
        div()
            .size_full()
            .rounded_full()
            .overflow_hidden()
            .bg(rgb(0x202020))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .children(
                self.image
                    .clone()
                    .map(|image| img(image).size_full().rounded_full()),
            )
    }
}
