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
    cm::CMSampleBuffer,
    prelude::*,
    recording_output::{
        RecordingCallbacks, SCRecordingOutput, SCRecordingOutputCodec,
        SCRecordingOutputConfiguration, SCRecordingOutputFileType,
    },
    shareable_content::SCShareableContentInfo,
};

use crate::project::{FPS, even};

pub struct ScreenRecording {
    stream: SCStream,
    _output: SCRecordingOutput,
    finished: mpsc::Receiver<Result<(), String>>,
    first_frame_host_ns: Arc<AtomicU64>,
}

pub fn start(system_audio: bool, path: &Path) -> anyhow::Result<ScreenRecording> {
    let content = SCShareableContent::get()?;
    let display = content
        .displays()
        .into_iter()
        .next()
        .context("no display")?;
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
    let scale = SCShareableContentInfo::for_filter(&filter)
        .map_or(2.0, |info| info.point_pixel_scale()) as f64;
    let frame = display.frame();
    let config = SCStreamConfiguration::new()
        .with_width(even(frame.size.width * scale))
        .with_height(even(frame.size.height * scale))
        .with_fps(FPS)
        .with_shows_cursor(true)
        .with_captures_audio(system_audio);

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
            let pts = sample.presentation_timestamp();
            if pts.timescale > 0 {
                let ns = (pts.value as i128 * 1_000_000_000 / pts.timescale as i128) as u64;
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
