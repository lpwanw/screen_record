use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context, anyhow};
use screencapturekit::{
    cg::CGRect,
    cm::{CMSampleBuffer, CMSampleBufferSCExt, SCFrameStatus},
    prelude::*,
    recording_output::{
        RecordingCallbacks, SCRecordingOutput, SCRecordingOutputCodec,
        SCRecordingOutputConfiguration, SCRecordingOutputFileType,
    },
    shareable_content::SCShareableContentInfo,
};

use crate::{
    macos,
    project::{FPS, Mode, Rect, even},
};

const MIN_WINDOW_SIDE: f64 = 64.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    pub id: u32,
    /// Global points, top-left origin.
    pub frame: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: u32,
    pub title: String,
    pub frame: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Display(u32),
    Window(u32),
    /// `rect` is in points relative to the display's top-left corner.
    Region {
        display: u32,
        rect: Rect,
    },
}

impl Target {
    pub fn mode(&self) -> Mode {
        match self {
            Target::Display(_) => Mode::Display,
            Target::Window(_) => Mode::Window,
            Target::Region { .. } => Mode::Region,
        }
    }
}

pub struct ScreenRecording {
    stream: SCStream,
    _output: SCRecordingOutput,
    finished: mpsc::Receiver<Result<(), String>>,
    first_frame_host_ns: Arc<AtomicU64>,
    /// Captured area in global points.
    pub capture: Rect,
    pub width: u32,
    pub height: u32,
}

pub fn displays() -> anyhow::Result<Vec<Display>> {
    Ok(SCShareableContent::get()?
        .displays()
        .into_iter()
        .map(|display| Display {
            id: display.display_id(),
            frame: rect(display.frame()),
        })
        .collect())
}

/// @return titled, normal-level, on-screen windows of other apps, front to back
pub fn windows() -> anyhow::Result<Vec<Window>> {
    let own_pid = std::process::id() as i32;
    Ok(SCShareableContent::get()?
        .windows()
        .into_iter()
        .filter(|window| window.is_on_screen() && window.window_layer() == 0)
        .filter_map(|window| {
            let app = window.owning_application()?;
            let title = window.title().filter(|title| !title.is_empty())?;
            let frame = rect(window.frame());
            (app.process_id() != own_pid && frame.w.min(frame.h) >= MIN_WINDOW_SIDE).then(|| {
                Window {
                    id: window.window_id(),
                    title: format!("{}: {title}", app.application_name()),
                    frame,
                }
            })
        })
        .collect())
}

fn rect(frame: CGRect) -> Rect {
    Rect {
        x: frame.origin.x,
        y: frame.origin.y,
        w: frame.size.width,
        h: frame.size.height,
    }
}

fn display_filter(
    content: &SCShareableContent,
    id: u32,
) -> anyhow::Result<(SCContentFilter, Rect)> {
    let display = content
        .displays()
        .into_iter()
        .find(|display| display.display_id() == id)
        .context("display disconnected")?;
    let own_app: Vec<_> = content
        .applications()
        .into_iter()
        .filter(|app| app.process_id() == std::process::id() as i32)
        .collect();
    anyhow::ensure!(
        !own_app.is_empty(),
        "own app missing from shareable content; launch the .app bundle"
    );
    let filter = SCContentFilter::create()
        .with_display(&display)
        .with_excluding_applications(&own_app.iter().collect::<Vec<_>>(), &[])
        .build()?;
    Ok((filter, rect(display.frame())))
}

/// Own app windows (panel, bubble) are excluded from display and region captures.
pub fn start(target: &Target, system_audio: bool, path: &Path) -> anyhow::Result<ScreenRecording> {
    let content = SCShareableContent::get()?;
    let (filter, capture, source) = match target {
        Target::Display(id) => {
            let (filter, frame) = display_filter(&content, *id)?;
            (filter, frame, None)
        }
        Target::Window(id) => {
            let window = content
                .windows()
                .into_iter()
                .find(|window| window.window_id() == *id)
                .context("window closed")?;
            let frame = rect(window.frame());
            (
                SCContentFilter::create().with_window(&window).build()?,
                frame,
                None,
            )
        }
        Target::Region { display, rect } => {
            let (filter, frame) = display_filter(&content, *display)?;
            let capture = Rect {
                x: frame.x + rect.x,
                y: frame.y + rect.y,
                ..*rect
            };
            (filter, capture, Some(*rect))
        }
    };
    let scale = SCShareableContentInfo::for_filter(&filter)
        .map_or(2.0, |info| info.point_pixel_scale()) as f64;
    let (width, height) = (even(capture.w * scale), even(capture.h * scale));
    let mut config = SCStreamConfiguration::new()
        .with_width(width)
        .with_height(height)
        .with_fps(FPS)
        .with_shows_cursor(true)
        .with_captures_audio(system_audio);
    if let Some(source) = source {
        config = config.with_source_rect(CGRect::new(source.x, source.y, source.w, source.h));
    }

    let (done, finished) = mpsc::channel();
    let failed = done.clone();
    let callbacks = RecordingCallbacks::new()
        .on_finish(move || {
            done.send(Ok(())).ok();
        })
        .on_fail(move |error| {
            failed.send(Err(error)).ok();
        });
    let output_config = SCRecordingOutputConfiguration::new()?
        .with_output_url(path)
        .map_err(|_| anyhow!("invalid output path {}", path.display()))?
        .with_video_codec(SCRecordingOutputCodec::HEVC)
        .with_output_file_type(SCRecordingOutputFileType::MP4);
    let output = SCRecordingOutput::new_with_delegate(&output_config, callbacks)
        .context("recording output needs macOS 15")?;

    let first_frame_host_ns = Arc::new(AtomicU64::new(0));
    let mut stream = SCStream::new(&filter, &config)?;
    let first = first_frame_host_ns.clone();
    stream.add_output_handler(
        move |sample: CMSampleBuffer, _| {
            if sample.frame_status() != Some(SCFrameStatus::Complete) {
                return;
            }
            let pts = sample.presentation_timestamp();
            if let Some(ns) = macos::cmtime_ns(pts.value, pts.timescale) {
                first
                    .compare_exchange(0, ns, Ordering::AcqRel, Ordering::Acquire)
                    .ok();
            }
        },
        SCStreamOutputType::Screen,
    )?;
    stream.add_recording_output(&output)?;
    stream.start_capture()?;
    Ok(ScreenRecording {
        stream,
        _output: output,
        finished,
        first_frame_host_ns,
        capture,
        width,
        height,
    })
}

impl ScreenRecording {
    /// Stops capture and blocks until the file is finalized, so call it off the main thread.
    /// @return host time (ns) of the first captured frame
    pub fn finish(self) -> anyhow::Result<u64> {
        let stopped = self.stream.stop_capture();
        self.finished
            .recv_timeout(Duration::from_secs(10))
            .context("recording did not finish")?
            .map_err(|error| anyhow!("recording failed: {error}"))?;
        stopped?;
        Ok(self.first_frame_host_ns.load(Ordering::Acquire))
    }
}
