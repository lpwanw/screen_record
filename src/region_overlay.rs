use anyhow::Context as _;
use gpui::{
    App, AppContext, Bounds, Context, DisplayId, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, Styled, Window, WindowBackgroundAppearance, WindowBounds, WindowKind,
    WindowOptions, div, hsla, point, px,
};

use crate::project::Rect;

const MIN_SIDE: f32 = 16.;

type OnDone = Box<dyn FnOnce(Option<Rect>, &mut App)>;

pub struct RegionOverlay {
    start: Option<Point<Pixels>>,
    current: Option<Point<Pixels>>,
    focus: FocusHandle,
    on_done: Option<OnDone>,
}

/// Covers `display` with a dimmed layer; drag selects, Esc or right-click cancels.
/// @param on_done receives the selection in points relative to the display's top-left corner
pub fn open(
    display: u32,
    cx: &mut App,
    on_done: impl FnOnce(Option<Rect>, &mut App) + 'static,
) -> anyhow::Result<()> {
    let id = DisplayId::new(display as u64);
    let size = cx
        .find_display(id)
        .context("display disconnected")?
        .bounds()
        .size;
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                size,
            ))),
            display_id: Some(id),
            titlebar: None,
            kind: WindowKind::PopUp,
            window_background: WindowBackgroundAppearance::Transparent,
            focus: true,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let focus = cx.focus_handle();
                window.focus(&focus, cx);
                RegionOverlay {
                    start: None,
                    current: None,
                    focus,
                    on_done: Some(Box::new(on_done)),
                }
            })
        },
    )?;
    cx.activate(true);
    Ok(())
}

impl RegionOverlay {
    fn selection(&self) -> Option<Bounds<Pixels>> {
        let (a, b) = (self.start?, self.current?);
        Some(Bounds::from_corners(
            point(a.x.min(b.x), a.y.min(b.y)),
            point(a.x.max(b.x), a.y.max(b.y)),
        ))
    }

    fn finish(&mut self, selection: Option<Rect>, window: &mut Window, cx: &mut App) {
        if let Some(on_done) = self.on_done.take() {
            on_done(selection, cx);
        }
        window.remove_window();
    }

    fn release(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.start.is_none() {
            return;
        }
        self.current = Some(position);
        let selection = self
            .selection()
            .filter(|b| b.size.width.as_f32().min(b.size.height.as_f32()) >= MIN_SIDE)
            .map(|b| Rect {
                x: b.origin.x.as_f32().into(),
                y: b.origin.y.as_f32().into(),
                w: b.size.width.as_f32().into(),
                h: b.size.height.as_f32().into(),
            });
        match selection {
            Some(selection) => self.finish(Some(selection), window, cx),
            None => {
                self.start = None;
                self.current = None;
                cx.notify();
            }
        }
    }
}

impl Render for RegionOverlay {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.selection().map(|b| {
            div()
                .absolute()
                .left(b.origin.x)
                .top(b.origin.y)
                .w(b.size.width)
                .h(b.size.height)
                .border_2()
                .border_color(hsla(0., 0., 1., 0.9))
                .bg(hsla(0., 0., 1., 0.08))
                .text_color(hsla(0., 0., 1., 1.))
                .p_1()
                .child(format!(
                    "{:.0} × {:.0}",
                    b.size.width.as_f32(),
                    b.size.height.as_f32()
                ))
        });
        div()
            .size_full()
            .track_focus(&self.focus)
            .cursor_crosshair()
            .bg(hsla(0., 0., 0., 0.35))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.start = Some(event.position);
                    this.current = Some(event.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.start.is_some() {
                    this.current = Some(event.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.release(event.position, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, window, cx| this.finish(None, window, cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.finish(None, window, cx);
                }
            }))
            .children(selection)
    }
}
