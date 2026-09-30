use std::{
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use anyhow::Context as _;
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Window, WindowHandle, div, prelude::FluentBuilder, rgb,
};
use gpui_component::{
    Disableable, IndexPath,
    button::{Button, ButtonVariants},
    progress::Progress,
    radio::RadioGroup,
    select::{Select, SelectEvent, SelectState},
    switch::Switch,
};

use crate::{
    bubble::{self, Bubble, Source},
    camera::{self, Camera, FrameSlot},
    export,
    macos::{self, Access, Media},
    mic::{self, Mic},
    project::{Mode, Rect},
    recording::{self, Session},
    region_overlay,
    screen::{self, Target},
};

const MODES: [Mode; 3] = [Mode::Display, Mode::Window, Mode::Region];
const NONE: &str = "None";

type Choices = Entity<SelectState<Vec<SharedString>>>;

enum State {
    Idle,
    Starting,
    Recording {
        session: Box<Session>,
        since: Instant,
    },
    Finishing,
    Exporting(f32),
}

pub struct Panel {
    bubble: WindowHandle<Bubble>,
    slot: FrameSlot,
    state: State,
    mode: Mode,
    displays: Vec<screen::Display>,
    windows: Vec<screen::Window>,
    cameras: Vec<camera::Device>,
    mics: Vec<String>,
    display_select: Choices,
    window_select: Choices,
    camera_select: Choices,
    mic_select: Choices,
    region: Option<Rect>,
    camera: Option<Camera>,
    mic: Option<Mic>,
    system_audio: bool,
    screen_granted: bool,
    last_dir: Option<PathBuf>,
    status: SharedString,
    error: Option<SharedString>,
    quit_when_idle: bool,
    overlay_open: bool,
}

fn choices(
    items: Vec<SharedString>,
    selected: Option<usize>,
    window: &mut Window,
    cx: &mut Context<Panel>,
) -> Choices {
    cx.new(|cx| SelectState::new(items, selected.map(IndexPath::new), window, cx))
}

fn chosen(select: &Choices, cx: &App) -> Option<usize> {
    select.read(cx).selected_index(cx).map(|index| index.row)
}

fn with_none(names: impl IntoIterator<Item = String>) -> Vec<SharedString> {
    std::iter::once(NONE.into())
        .chain(names.into_iter().map(SharedString::from))
        .collect()
}

fn display_names(displays: &[screen::Display]) -> Vec<SharedString> {
    displays
        .iter()
        .enumerate()
        .map(|(i, display)| {
            format!(
                "Display {} ({:.0} × {:.0})",
                i + 1,
                display.frame.w,
                display.frame.h
            )
            .into()
        })
        .collect()
}

impl Panel {
    pub fn new(bubble: WindowHandle<Bubble>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let screen_granted = macos::screen_access();
        let displays = if screen_granted {
            screen::displays().unwrap_or_default()
        } else {
            Vec::new()
        };
        let cameras = camera::devices();
        let mics = mic::input_devices();
        let default_mic = mic::default_input()
            .and_then(|name| mics.iter().position(|mic| *mic == name))
            .or((!mics.is_empty()).then_some(0));

        let display_select = choices(display_names(&displays), Some(0), window, cx);
        let window_select = choices(Vec::new(), None, window, cx);
        let camera_select = choices(
            with_none(cameras.iter().map(|camera| camera.name.clone())),
            Some(if cameras.is_empty() { 0 } else { 1 }),
            window,
            cx,
        );
        let mic_select = choices(
            with_none(mics.clone()),
            Some(default_mic.map_or(0, |i| i + 1)),
            window,
            cx,
        );
        cx.subscribe_in(
            &display_select,
            window,
            |this, _, _: &SelectEvent<_>, _, cx| {
                this.region = None;
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &camera_select,
            window,
            |this, _, _: &SelectEvent<_>, _, cx| this.open_camera(cx),
        )
        .detach();
        cx.subscribe_in(&mic_select, window, |this, _, _: &SelectEvent<_>, _, cx| {
            this.open_mic(cx)
        })
        .detach();

        let panel = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            panel
                .update(cx, |panel, cx| panel.close_requested(cx))
                .unwrap_or(true)
        });
        Self::tick(cx);

        let mut panel = Self {
            bubble,
            slot: FrameSlot::default(),
            state: State::Idle,
            mode: Mode::Display,
            displays,
            windows: Vec::new(),
            cameras,
            mics,
            display_select,
            window_select,
            camera_select,
            mic_select,
            region: None,
            camera: None,
            mic: None,
            system_audio: true,
            screen_granted,
            last_dir: None,
            status: "Ready".into(),
            error: None,
            quit_when_idle: false,
            overlay_open: false,
        };
        panel.open_camera(cx);
        panel.open_mic(cx);
        panel
    }

    fn tick(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let alive = this.update(cx, |this, cx| {
                    if matches!(this.state, State::Recording { .. }) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn busy(&self) -> bool {
        !matches!(self.state, State::Idle)
    }

    fn fail(&mut self, message: impl Into<SharedString>) {
        let message = message.into();
        eprintln!("{message}");
        self.error = Some(message);
    }

    fn open_camera(&mut self, cx: &mut Context<Self>) {
        self.camera = None;
        self.slot.lock().unwrap().take();
        let device = chosen(&self.camera_select, cx)
            .and_then(|row| row.checked_sub(1))
            .and_then(|i| self.cameras.get(i).cloned());
        if let Some(device) = device {
            match macos::av_access(Media::Camera) {
                Access::Granted => match camera::open(&device.id, self.slot.clone()) {
                    Ok(camera) => self.camera = Some(camera),
                    Err(error) => self.fail(format!("Camera: {error:#}")),
                },
                Access::Undetermined => self.request_access(Media::Camera, cx),
                Access::Denied => self
                    .fail("Camera access denied: allow it in System Settings › Privacy & Security"),
            }
        }
        self.update_bubble(cx);
    }

    fn open_mic(&mut self, cx: &mut Context<Self>) {
        self.mic = None;
        let name = chosen(&self.mic_select, cx)
            .and_then(|row| row.checked_sub(1))
            .and_then(|i| self.mics.get(i).cloned());
        if let Some(name) = name {
            match macos::av_access(Media::Mic) {
                Access::Granted => match mic::open(Some(&name)) {
                    Ok(mic) => self.mic = Some(mic),
                    Err(error) => self.fail(format!("Microphone: {error:#}")),
                },
                Access::Undetermined => self.request_access(Media::Mic, cx),
                Access::Denied => self.fail(
                    "Microphone access denied: allow it in System Settings › Privacy & Security",
                ),
            }
        }
        self.update_bubble(cx);
    }

    fn request_access(&mut self, media: Media, cx: &mut Context<Self>) {
        let (answer, answered) = mpsc::channel();
        macos::request_av(media, move |granted| {
            answer.send(granted).ok();
        });
        cx.spawn(async move |this, cx| {
            let granted = cx
                .background_executor()
                .spawn(async move { answered.recv().unwrap_or(false) })
                .await;
            this.update(cx, |this, cx| {
                match (media, granted) {
                    (Media::Camera, true) => this.open_camera(cx),
                    (Media::Mic, true) => this.open_mic(cx),
                    (Media::Camera, false) => this.fail("Camera access denied"),
                    (Media::Mic, false) => this.fail("Microphone access denied"),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn update_bubble(&mut self, cx: &mut Context<Self>) {
        let source = match (&self.camera, &self.mic) {
            (Some(_), _) => Source::Camera(self.slot.clone()),
            (None, Some(mic)) => Source::Mic(mic.handle.clone()),
            (None, None) => Source::Empty,
        };
        self.bubble
            .update(cx, |bubble, _, cx| bubble.set_source(source, cx))
            .ok();
        cx.notify();
    }

    fn refresh_windows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.windows = screen::windows().unwrap_or_else(|error| {
            self.fail(format!("Window list: {error:#}"));
            Vec::new()
        });
        let names: Vec<SharedString> = self
            .windows
            .iter()
            .map(|window| window.title.clone().into())
            .collect();
        let first = (!names.is_empty()).then(|| IndexPath::new(0));
        self.window_select.update(cx, |select, cx| {
            select.set_items(names, window, cx);
            select.set_selected_index(first, window, cx);
        });
    }

    fn set_mode(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = mode;
        if mode == Mode::Window {
            self.refresh_windows(window, cx);
        }
        cx.notify();
    }

    fn selected_display(&self, cx: &App) -> anyhow::Result<&screen::Display> {
        chosen(&self.display_select, cx)
            .and_then(|row| self.displays.get(row))
            .context("choose a display")
    }

    fn select_region(&mut self, cx: &mut Context<Self>) {
        let display = match self.selected_display(cx) {
            Ok(display) => display.id,
            Err(error) => return self.fail(format!("{error:#}")),
        };
        let panel = cx.entity().downgrade();
        let opened = region_overlay::open(display, cx, move |region, cx| {
            panel
                .update(cx, |this, cx| {
                    this.overlay_open = false;
                    if region.is_some() {
                        this.region = region;
                    }
                    cx.notify();
                })
                .ok();
        });
        match opened {
            Ok(()) => self.overlay_open = true,
            Err(error) => self.fail(format!("Region: {error:#}")),
        }
    }

    fn target(&self, cx: &App) -> anyhow::Result<Target> {
        Ok(match self.mode {
            Mode::Display => Target::Display(self.selected_display(cx)?.id),
            Mode::Window => Target::Window(
                chosen(&self.window_select, cx)
                    .and_then(|row| self.windows.get(row))
                    .context("choose a window")?
                    .id,
            ),
            Mode::Region => Target::Region {
                display: self.selected_display(cx)?.id,
                rect: self.region.context("select a region first")?,
            },
        })
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        match self.state {
            State::Idle => self.record(cx),
            State::Recording { .. } => self.stop(cx),
            State::Starting | State::Finishing | State::Exporting(_) => {}
        }
    }

    fn record(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let options = match self.target(cx) {
            Ok(target) => recording::Options {
                target,
                system_audio: self.system_audio,
            },
            Err(error) => {
                self.fail(format!("Record: {error:#}"));
                return cx.notify();
            }
        };
        self.state = State::Starting;
        self.status = "Starting…".into();
        let requested = Instant::now();
        cx.spawn(async move |this, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { recording::start_screen(&options) })
                .await;
            eprintln!("screen start took {:?}", requested.elapsed());
            this.update(cx, |this, cx| match started {
                Ok(started) => {
                    let session = started.attach(
                        this.mic.as_ref().map(|mic| &mic.handle),
                        this.camera.as_ref(),
                    );
                    this.status = format!("Recording to {}", session.dir().display()).into();
                    this.state = State::Recording {
                        session: Box::new(session),
                        since: Instant::now(),
                    };
                    if this.quit_when_idle {
                        this.stop(cx);
                    }
                    cx.notify();
                }
                Err(error) => {
                    this.fail(format!("Record: {error:#}"));
                    this.become_idle(cx);
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let State::Recording { session, .. } = std::mem::replace(&mut self.state, State::Finishing)
        else {
            return;
        };
        let started = Instant::now();
        let bubble = self
            .bubble
            .update(cx, |_, window, _| Bubble::frame(window))
            .ok()
            .flatten()
            .unwrap_or(Rect {
                x: 0.,
                y: 0.,
                w: bubble::SIZE.into(),
                h: bubble::SIZE.into(),
            });
        let dir = session.dir().to_path_buf();
        let finishing = session.stop(bubble, self.camera.as_ref());
        eprintln!("record stop took {:?}", started.elapsed());
        self.status = "Saving…".into();
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move { finishing.wait() })
                .await;
            this.update(cx, |this, cx| {
                match saved {
                    Ok(dir) => {
                        this.status = format!("Saved {}", dir.display()).into();
                        eprintln!("{}", this.status);
                        this.last_dir = Some(dir);
                    }
                    Err(error) => {
                        this.fail(format!("Recording failed ({}): {error:#}", dir.display()))
                    }
                }
                this.become_idle(cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self.last_dir.clone() else {
            return;
        };
        self.error = None;
        self.state = State::Exporting(0.);
        let progress = Arc::new(AtomicU32::new(0));
        let reported = progress.clone();
        let task = cx.background_executor().spawn(async move {
            export::run(&dir, |p| reported.store(p.to_bits(), Ordering::Relaxed))
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let exporting = this.update(cx, |this, cx| match &mut this.state {
                    State::Exporting(value) => {
                        *value = f32::from_bits(progress.load(Ordering::Relaxed));
                        cx.notify();
                        true
                    }
                    _ => false,
                });
                if !exporting.unwrap_or(false) {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let exported = task.await;
            this.update(cx, |this, cx| {
                match exported {
                    Ok(out) => {
                        this.status = format!("Exported {}", out.display()).into();
                        eprintln!("{}", this.status);
                    }
                    Err(error) => this.fail(format!("Export: {error:#}")),
                }
                this.become_idle(cx);
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn become_idle(&mut self, cx: &mut Context<Self>) {
        self.state = State::Idle;
        if self.quit_when_idle {
            cx.quit();
        }
        cx.notify();
    }

    fn open_folder(&mut self) {
        if let Some(dir) = &self.last_dir
            && let Err(error) = Command::new("open").arg(dir).spawn()
        {
            self.fail(format!("Open folder: {error}"));
        }
    }

    /// Recording and export finish before the app quits.
    /// @return whether the panel window may close now
    fn close_requested(&mut self, cx: &mut Context<Self>) -> bool {
        match self.state {
            State::Idle => true,
            State::Recording { .. } => {
                self.quit_when_idle = true;
                self.stop(cx);
                false
            }
            State::Starting | State::Finishing | State::Exporting(_) => {
                self.quit_when_idle = true;
                false
            }
        }
    }

    fn elapsed(&self) -> Option<String> {
        let State::Recording { since, .. } = &self.state else {
            return None;
        };
        let seconds = since.elapsed().as_secs();
        Some(format!("{:02}:{:02}", seconds / 60, seconds % 60))
    }
}

fn row() -> gpui::Div {
    div().flex().items_center().gap_2()
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy();
        let recording = matches!(self.state, State::Recording { .. });
        let mode_index = MODES.iter().position(|mode| *mode == self.mode);
        let region = self.region.map_or("No region selected".to_string(), |r| {
            format!("{:.0} × {:.0} at ({:.0}, {:.0})", r.w, r.h, r.x, r.y)
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .when(!self.screen_granted, |panel| {
                panel.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_2()
                        .border_1()
                        .border_color(rgb(0xd97706))
                        .rounded_md()
                        .child("Screen Recording permission is required. Grant it, then relaunch the app.")
                        .child(
                            Button::new("grant-screen")
                                .label("Grant access")
                                .on_click(|_, _, _| {
                                    macos::request_screen_access();
                                }),
                        ),
                )
            })
            .child(
                RadioGroup::horizontal("mode")
                    .children(["Display", "Window", "Region"])
                    .selected_index(mode_index)
                    .disabled(busy)
                    .on_click(cx.listener(|this, index: &usize, window, cx| {
                        this.set_mode(MODES[*index], window, cx)
                    })),
            )
            .when(self.mode != Mode::Window, |panel| {
                panel.child(Select::new(&self.display_select).disabled(busy))
            })
            .when(self.mode == Mode::Window, |panel| {
                panel.child(
                    row()
                        .child(
                            div()
                                .flex_1()
                                .child(Select::new(&self.window_select).disabled(busy)),
                        )
                        .child(
                            Button::new("refresh-windows")
                                .label("Refresh")
                                .disabled(busy)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.refresh_windows(window, cx)
                                })),
                        ),
                )
            })
            .when(self.mode == Mode::Region, |panel| {
                panel.child(
                    row()
                        .child(
                            Button::new("select-region")
                                .label("Select region…")
                                .disabled(busy || self.overlay_open)
                                .on_click(cx.listener(|this, _, _, cx| this.select_region(cx))),
                        )
                        .child(region),
                )
            })
            .child(row().child("Camera").child(
                div().flex_1().child(Select::new(&self.camera_select).disabled(busy)),
            ))
            .child(row().child("Microphone").child(
                div().flex_1().child(Select::new(&self.mic_select).disabled(busy)),
            ))
            .child(
                Switch::new("system-audio")
                    .label("System audio")
                    .checked(self.system_audio)
                    .disabled(busy)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.system_audio = *checked;
                        cx.notify();
                    })),
            )
            .child(
                row()
                    .child(
                        Button::new("record")
                            .primary()
                            .label(if recording { "Stop" } else { "Record" })
                            .disabled(!self.screen_granted || (busy && !recording))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_recording(cx))),
                    )
                    .children(self.elapsed()),
            )
            .child(
                row()
                    .child(
                        Button::new("export")
                            .label("Export MP4")
                            .disabled(busy || self.last_dir.is_none())
                            .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                    )
                    .child(
                        Button::new("open-folder")
                            .label("Open folder")
                            .disabled(self.last_dir.is_none())
                            .on_click(cx.listener(|this, _, _, _| this.open_folder())),
                    ),
            )
            .when_some(
                match self.state {
                    State::Exporting(progress) => Some(progress),
                    _ => None,
                },
                |panel, progress| panel.child(Progress::new("export-progress").value(progress * 100.)),
            )
            .child(div().text_sm().child(self.status.clone()))
            .when_some(self.error.clone(), |panel, error| {
                panel.child(div().text_sm().text_color(rgb(0xdc2626)).child(error))
            })
    }
}
