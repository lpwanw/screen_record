use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    camera::{Camera, CameraRecording},
    export, macos,
    mic::{MicHandle, MicRecording},
    project::{self, Mode, Project, Rect, Track},
    screen::{self, ScreenRecording, Target},
};

const SCREEN: &str = "screen.mp4";
const CAMERA: &str = "camera.mov";
const MIC: &str = "mic.wav";

pub struct Options {
    pub target: Target,
    pub system_audio: bool,
}

/// A running screen capture waiting for the other tracks; `Send`, so its blocking start
/// can run off the main thread.
pub struct Started {
    dir: PathBuf,
    mode: Mode,
    system_audio: bool,
    screen: ScreenRecording,
}

pub struct Session {
    dir: PathBuf,
    mode: Mode,
    system_audio: bool,
    screen: ScreenRecording,
    camera: Option<CameraRecording>,
    mic: Option<MicHandle>,
}

/// Stopped tracks still being finalized; `Send`, so `wait` can run off the main thread.
pub struct Finishing {
    dir: PathBuf,
    mode: Mode,
    system_audio: bool,
    screen: ScreenRecording,
    camera: Option<CameraRecording>,
    mic: Option<MicRecording>,
    stopped_host_ns: u64,
    bubble: Rect,
}

/// Creates the recording folder and starts the screen track, the timeline anchor.
/// Blocks until ScreenCaptureKit is capturing, so call it off the main thread.
pub fn start_screen(options: &Options) -> anyhow::Result<Started> {
    let dir = new_dir()?;
    let screen = screen::start(&options.target, options.system_audio, &dir.join(SCREEN))
        .inspect_err(|_| {
            fs::remove_dir_all(&dir).ok();
        })?;
    Ok(Started {
        dir,
        mode: options.target.mode(),
        system_audio: options.system_audio,
        screen,
    })
}

impl Started {
    /// Adds the camera and microphone tracks; a failing one only drops that track.
    pub fn attach(self, mic: Option<&MicHandle>, camera: Option<&Camera>) -> Session {
        let mic = mic.and_then(|mic| {
            log_failure("microphone", mic.start(&self.dir.join(MIC))).map(|()| mic.clone())
        });
        let camera = camera.map(|camera| camera.start_recording(&self.dir.join(CAMERA)));
        Session {
            dir: self.dir,
            mode: self.mode,
            system_audio: self.system_audio,
            screen: self.screen,
            camera,
            mic,
        }
    }
}

impl Session {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// @param bubble bubble window bounds in global points, stored relative to the capture
    /// @param camera the camera passed to `attach`
    pub fn stop(self, bubble: Rect, camera: Option<&Camera>) -> Finishing {
        let stopped_host_ns = macos::host_now_ns();
        if let (Some(_), Some(camera)) = (&self.camera, camera) {
            camera.stop_recording();
        }
        Finishing {
            dir: self.dir,
            mode: self.mode,
            system_audio: self.system_audio,
            screen: self.screen,
            camera: self.camera,
            mic: self.mic.map(|mic| mic.stop()),
            stopped_host_ns,
            bubble,
        }
    }
}

impl Finishing {
    /// Blocks until every track is finalized, then writes `project.json`.
    /// @return the recording folder
    pub fn wait(self) -> anyhow::Result<PathBuf> {
        let (capture, width, height) = (self.screen.capture, self.screen.width, self.screen.height);
        let screen_start = self.screen.finish()?;
        anyhow::ensure!(screen_start > 0, "no screen frames were captured");
        let track = |file: &str, start_host_ns: u64| Track {
            file: file.into(),
            start_host_ns,
        };
        let camera = self
            .camera
            .and_then(|camera| log_failure("camera", camera.wait()))
            .map(|start| track(CAMERA, start));
        let mic = self
            .mic
            .and_then(|mic| log_failure("microphone", mic.wait()))
            .map(|start| track(MIC, start));
        let system_audio =
            self.system_audio && export::has_audio(&self.dir.join(SCREEN)).unwrap_or(true);
        let project = Project {
            version: project::VERSION,
            mode: self.mode,
            rect: capture,
            width,
            height,
            duration_s: self.stopped_host_ns.saturating_sub(screen_start) as f64 / 1e9,
            screen: track(SCREEN, screen_start),
            system_audio,
            mic,
            camera,
            bubble: project::normalize_bubble(self.bubble, capture),
        };
        project.save(&self.dir)?;
        Ok(self.dir)
    }
}

fn log_failure<T>(track: &str, result: anyhow::Result<T>) -> Option<T> {
    result
        .inspect_err(|error| eprintln!("{track} track dropped: {error:#}"))
        .ok()
}

/// @return a new empty folder `~/Movies/Taywind/<local date time>`
fn new_dir() -> anyhow::Result<PathBuf> {
    let root = PathBuf::from(std::env::var("HOME")?).join("Movies/Taywind");
    fs::create_dir_all(&root)?;
    let stamp = local_timestamp();
    for attempt in 1.. {
        let name = match attempt {
            1 => stamp.clone(),
            n => format!("{stamp} ({n})"),
        };
        let dir = root.join(name);
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!()
}

fn local_timestamp() -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        libc::localtime_r(&now, &mut tm);
    }
    format!(
        "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}
