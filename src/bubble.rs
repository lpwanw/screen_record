use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::Context as _;
use gpui::{
    App, AppContext, Bounds, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, RenderImage, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowHandle,
    WindowKind, WindowOptions, canvas, div, fill, img, point, px, rgb, size,
};
use image::{Frame as ImageFrame, RgbaImage};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::{camera::FrameSlot, macos, mic::MicHandle, project::Rect};

pub const SIZE: f32 = 200.;
const MARGIN: f32 = 40.;
const BARS: usize = 24;
const BACKGROUND: u32 = 0x1e1e1e;

pub enum Source {
    Camera(FrameSlot),
    Mic(MicHandle),
    Empty,
}

pub struct Bubble {
    source: Source,
    image: Option<Arc<RenderImage>>,
    stale: Vec<Arc<RenderImage>>,
    levels: VecDeque<f32>,
}

impl Bubble {
    /// Opens the always-on-top bubble at the bottom-right of the primary display.
    pub fn open(cx: &mut App) -> anyhow::Result<WindowHandle<Bubble>> {
        let display = cx.primary_display().context("no display")?.bounds();
        let origin = point(
            display.size.width - px(SIZE + MARGIN),
            display.size.height - px(SIZE + MARGIN),
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
                        source: Source::Empty,
                        image: None,
                        stale: Vec::new(),
                        levels: VecDeque::from(vec![0.; BARS]),
                    }
                })
            },
        )
    }

    pub fn set_source(&mut self, source: Source, cx: &mut Context<Self>) {
        self.stale.extend(self.image.take());
        self.levels = VecDeque::from(vec![0.; BARS]);
        self.source = source;
        cx.notify();
    }

    /// @return the bubble window bounds in global points, top-left origin
    pub fn frame(window: &Window) -> Option<Rect> {
        match HasWindowHandle::window_handle(window).ok()?.as_raw() {
            RawWindowHandle::AppKit(handle) => macos::window_frame(handle.ns_view),
            _ => None,
        }
    }

    fn pump(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                let updated = this.update(cx, |bubble, cx| {
                    let changed = match &bubble.source {
                        Source::Camera(slot) => slot.lock().unwrap().is_some(),
                        Source::Mic(mic) => {
                            bubble.levels.pop_front();
                            bubble.levels.push_back(mic.take_peak());
                            true
                        }
                        Source::Empty => false,
                    };
                    if changed {
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
        let Source::Camera(slot) = &self.source else {
            return;
        };
        let Some(frame) = slot.lock().unwrap().take() else {
            return;
        };
        let Some(buffer) = RgbaImage::from_raw(frame.width, frame.height, frame.bgra) else {
            return;
        };
        let image = Arc::new(RenderImage::new([ImageFrame::new(buffer)]));
        self.stale.extend(self.image.replace(image));
    }
}

/// Bars stay inside the circle: each bar is at most the chord height at its x.
fn waveform(levels: Vec<f32>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let diameter = bounds.size.width.as_f32();
            let radius = diameter / 2.;
            let slot = diameter * 0.8 / BARS as f32;
            for (i, level) in levels.iter().enumerate() {
                let x = diameter * 0.1 + slot * (i as f32 + 0.5);
                let chord = 2. * (radius * radius - (x - radius).powi(2)).max(0.).sqrt();
                let height = (level.clamp(0., 1.).sqrt() * chord * 0.8).max(4.);
                let bar = Bounds::new(
                    point(
                        bounds.origin.x + px(x - slot * 0.3),
                        bounds.origin.y + px(radius - height / 2.),
                    ),
                    size(px(slot * 0.6), px(height)),
                );
                window.paint_quad(fill(bar, rgb(0xffffff)).corner_radii(px(slot * 0.3)));
            }
        },
    )
    .size_full()
}

impl Render for Bubble {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        for old in self.stale.drain(..) {
            window.drop_image(old).ok();
        }
        self.take_frame();
        let content = match &self.source {
            Source::Camera(_) => self
                .image
                .clone()
                .map(|image| img(image).size_full().rounded_full().into_any_element()),
            Source::Mic(_) => {
                Some(waveform(self.levels.iter().copied().collect()).into_any_element())
            }
            Source::Empty => None,
        };
        div()
            .size_full()
            .rounded_full()
            .overflow_hidden()
            .bg(rgb(BACKGROUND))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            .children(content)
    }
}
