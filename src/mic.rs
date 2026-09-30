use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

enum Command {
    Samples(Vec<f32>, u64),
    Start(PathBuf, mpsc::Sender<anyhow::Result<()>>),
    Stop(mpsc::Sender<anyhow::Result<u64>>),
}

/// Owns the input stream (not `Send`); share `handle` with other threads.
pub struct Mic {
    _stream: cpal::Stream,
    pub handle: MicHandle,
}

#[derive(Clone)]
pub struct MicHandle {
    commands: mpsc::Sender<Command>,
    peak: Arc<AtomicU32>,
}

/// A WAV file being finalized; `wait` blocks, so call it off the main thread.
pub struct MicRecording {
    finished: mpsc::Receiver<anyhow::Result<u64>>,
}

struct Take {
    writer: hound::WavWriter<BufWriter<File>>,
    start_host_ns: Option<u64>,
    error: Option<hound::Error>,
}

pub fn input_devices() -> Vec<String> {
    cpal::default_host()
        .input_devices()
        .map(|devices| devices.filter_map(|device| name(&device)).collect())
        .unwrap_or_default()
}

pub fn default_input() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .as_ref()
        .and_then(name)
}

fn name(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().to_string())
}

/// @param device input device name; the system default input when `None`
pub fn open(device: Option<&str>) -> anyhow::Result<Mic> {
    let host = cpal::default_host();
    let device = match device {
        Some(wanted) => host
            .input_devices()?
            .find(|device| name(device).as_deref() == Some(wanted)),
        None => host.default_input_device(),
    }
    .context("microphone not found")?;
    let config = device.default_input_config()?.config();
    let spec = hound::WavSpec {
        channels: config.channels,
        sample_rate: config.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let (commands, received) = mpsc::channel();
    thread::Builder::new()
        .name("mic-writer".into())
        .spawn(move || write_loop(received, spec))?;
    let peak = Arc::new(AtomicU32::new(0));
    let samples = commands.clone();
    let level = peak.clone();
    let stream = device.build_input_stream::<f32, _, _>(
        config,
        move |data, info| {
            let loudest = data.iter().fold(0f32, |max, sample| max.max(sample.abs()));
            level.fetch_max(loudest.to_bits(), Ordering::Relaxed);
            let capture_ns = info.timestamp().capture.as_nanos() as u64;
            samples
                .send(Command::Samples(data.to_vec(), capture_ns))
                .ok();
        },
        |error| eprintln!("microphone: {error}"),
        None,
    )?;
    stream.play()?;
    Ok(Mic {
        _stream: stream,
        handle: MicHandle { commands, peak },
    })
}

impl MicHandle {
    /// @return loudest absolute sample (0..=1) since the previous call
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, Ordering::Relaxed))
    }

    pub fn start(&self, path: &Path) -> anyhow::Result<()> {
        let (reply, started) = mpsc::channel();
        self.commands
            .send(Command::Start(path.to_path_buf(), reply))
            .map_err(|_| anyhow!("microphone writer stopped"))?;
        started
            .recv_timeout(Duration::from_secs(2))
            .context("microphone writer not responding")?
    }

    pub fn stop(&self) -> MicRecording {
        let (reply, finished) = mpsc::channel();
        self.commands.send(Command::Stop(reply)).ok();
        MicRecording { finished }
    }
}

impl MicRecording {
    /// @return host time (ns) of the first sample in the file
    pub fn wait(self) -> anyhow::Result<u64> {
        self.finished
            .recv_timeout(Duration::from_secs(10))
            .context("microphone recording did not finish")?
    }
}

fn write_loop(commands: mpsc::Receiver<Command>, spec: hound::WavSpec) {
    let mut take: Option<Take> = None;
    for command in commands {
        match command {
            Command::Samples(samples, capture_ns) => {
                let Some(take) = take.as_mut().filter(|take| take.error.is_none()) else {
                    continue;
                };
                take.start_host_ns.get_or_insert(capture_ns);
                if let Err(error) = samples
                    .into_iter()
                    .try_for_each(|sample| take.writer.write_sample(sample))
                {
                    take.error = Some(error);
                }
            }
            Command::Start(path, reply) => {
                let started = hound::WavWriter::create(path, spec).map(|writer| {
                    take = Some(Take {
                        writer,
                        start_host_ns: None,
                        error: None,
                    });
                });
                reply.send(started.map_err(Into::into)).ok();
            }
            Command::Stop(reply) => {
                reply.send(finish(take.take())).ok();
            }
        }
    }
}

fn finish(take: Option<Take>) -> anyhow::Result<u64> {
    let take = take.context("microphone was not recording")?;
    let finalized = take.writer.finalize();
    if let Some(error) = take.error {
        return Err(anyhow!("microphone write failed: {error}"));
    }
    finalized?;
    take.start_host_ns.context("no microphone samples captured")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> (MicHandle, thread::JoinHandle<()>) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let (commands, received) = mpsc::channel();
        let thread = thread::spawn(move || write_loop(received, spec));
        let handle = MicHandle {
            commands,
            peak: Arc::default(),
        };
        (handle, thread)
    }

    fn samples(handle: &MicHandle, values: &[f32], capture_ns: u64) {
        handle
            .commands
            .send(Command::Samples(values.to_vec(), capture_ns))
            .unwrap();
    }

    #[test]
    fn writes_only_between_start_and_stop() {
        let path = std::env::temp_dir().join(format!("taywind-mic-{}.wav", std::process::id()));
        let (handle, thread) = writer();
        samples(&handle, &[0.9, 0.9], 100);
        handle.start(&path).unwrap();
        samples(&handle, &[0.1, 0.2], 500);
        samples(&handle, &[0.3], 900);
        assert_eq!(handle.stop().wait().unwrap(), 500);
        samples(&handle, &[0.9], 1_000);
        drop(handle);
        thread.join().unwrap();

        let written: Vec<f32> = hound::WavReader::open(&path)
            .unwrap()
            .into_samples()
            .map(Result::unwrap)
            .collect();
        assert_eq!(written, [0.1, 0.2, 0.3]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn stop_without_samples_or_start_fails() {
        let path =
            std::env::temp_dir().join(format!("taywind-mic-empty-{}.wav", std::process::id()));
        let (handle, _thread) = writer();
        assert!(handle.stop().wait().is_err());
        handle.start(&path).unwrap();
        assert!(handle.stop().wait().is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn peak_is_taken_once() {
        let (handle, _thread) = writer();
        handle.peak.fetch_max(0.25f32.to_bits(), Ordering::Relaxed);
        handle.peak.fetch_max(0.5f32.to_bits(), Ordering::Relaxed);
        handle.peak.fetch_max(0.1f32.to_bits(), Ordering::Relaxed);
        assert_eq!(handle.take_peak(), 0.5);
        assert_eq!(handle.take_peak(), 0.0);
    }
}
