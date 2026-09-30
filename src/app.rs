use std::path::PathBuf;

use gpui::{Context, IntoElement, ParentElement, Render, SharedString, Styled, Window, div};
use gpui_component::{
    Disableable,
    button::{Button, ButtonVariants},
    switch::Switch,
};

use crate::{camera, macos, screen};

pub struct Panel {
    camera: Option<camera::Camera>,
    system_audio: bool,
    recording: Option<Recording>,
    finishing: bool,
    status: SharedString,
}

struct Recording {
    screen: screen::ScreenRecording,
    camera: Option<camera::CameraRecording>,
    started_ns: u64,
}

impl Panel {
    pub fn new(slot: camera::FrameSlot) -> Self {
        let (camera, status) = match camera::open(slot) {
            Ok(Some(camera)) => (Some(camera), "Camera ready".into()),
            Ok(None) => (None, "No camera connected".into()),
            Err(error) => (None, format!("Camera: {error:#}").into()),
        };
        Self {
            camera,
            system_audio: false,
            recording: None,
            finishing: false,
            status,
        }
    }

    fn toggle_recording(&mut self, cx: &mut Context<Self>) {
        if self.finishing {
            return;
        }
        match self.recording.take() {
            Some(recording) => self.stop(recording, cx),
            None => {
                self.status = match self.start() {
                    Ok((recording, dir)) => {
                        self.recording = Some(recording);
                        format!("Recording to {}", dir.display()).into()
                    }
                    Err(error) => format!("Start failed: {error:#}").into(),
                };
            }
        }
        eprintln!("{}", self.status);
        cx.notify();
    }

    fn start(&self) -> anyhow::Result<(Recording, PathBuf)> {
        anyhow::ensure!(
            macos::ensure_screen_access(),
            "screen recording permission not granted"
        );
        let dir = PathBuf::from(std::env::var("HOME")?)
            .join("Movies/Taywind")
            .join(format!("spike-{}", macos::host_now_ns()));
        std::fs::create_dir_all(&dir)?;
        let started_ns = macos::host_now_ns();
        let screen = screen::start(self.system_audio, &dir.join("screen.mp4"))?;
        let camera = self
            .camera
            .as_ref()
            .map(|camera| camera.start_recording(&dir.join("camera.mov")));
        let recording = Recording {
            screen,
            camera,
            started_ns,
        };
        Ok((recording, dir))
    }

    fn stop(&mut self, recording: Recording, cx: &mut Context<Self>) {
        if let Some(camera) = &self.camera {
            camera.stop_recording();
        }
        self.finishing = true;
        self.status = "Finishing…".into();
        cx.spawn(async move |this, cx| {
            let summary = cx
                .background_executor()
                .spawn(async move { summarize(recording) })
                .await;
            eprintln!("{summary}");
            this.update(cx, |this, cx| {
                this.finishing = false;
                this.status = summary.into();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

fn summarize(recording: Recording) -> String {
    let offset = |host_ns: u64| (host_ns as f64 - recording.started_ns as f64) / 1e6;
    let screen = recording
        .screen
        .finish()
        .map(|ns| format!("screen +{:.0}ms", offset(ns)));
    let camera = recording.camera.map(|camera| {
        camera
            .wait()
            .map(|ns| format!("camera +{:.0}ms", offset(ns)))
    });
    let describe =
        |result: anyhow::Result<String>| result.unwrap_or_else(|error| format!("{error:#}"));
    format!(
        "Saved: {}; {}",
        describe(screen),
        camera.map_or("no camera".into(), describe)
    )
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let recording = self.recording.is_some();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child("Taywind Record")
            .child(
                Switch::new("system-audio")
                    .label("System audio")
                    .checked(self.system_audio)
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.system_audio = *checked;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("record")
                    .primary()
                    .label(if recording { "Stop" } else { "Record" })
                    .disabled(self.finishing)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_recording(cx))),
            )
            .child(self.status.clone())
    }
}
