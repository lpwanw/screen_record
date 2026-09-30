use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::Context;

use crate::project::{FPS, Project, even};

const LANDSCAPE_MAX: (f64, f64) = (3840.0, 2160.0);
const BUBBLE_BACKGROUND: &str = "0x1e1e1e";

/// Reads `dir/project.json` and writes `dir/export.mp4`, reporting progress in 0..=1.
/// Blocks until ffmpeg exits, so call it off the UI thread.
/// @return path of the exported video
pub fn run(dir: &Path, mut on_progress: impl FnMut(f32)) -> anyhow::Result<PathBuf> {
    let project = Project::load(dir)?;
    let out = dir.join("export.mp4");
    let log = dir.join("export.log");
    let ffmpeg =
        tool("ffmpeg").context("ffmpeg not found; install it with `brew install ffmpeg`")?;
    let mut child = Command::new(ffmpeg)
        .args(ffmpeg_args(&project, dir, &out))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(&log)?)
        .spawn()?;
    let stdout = child.stdout.take().context("ffmpeg stdout")?;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else {
            child.kill().ok();
            break;
        };
        if let Some(progress) = parse_progress(&line, project.duration_s) {
            on_progress(progress);
        }
    }
    let status = child.wait()?;
    anyhow::ensure!(
        status.success(),
        "ffmpeg failed ({status}); see {}",
        log.display()
    );
    Ok(out)
}

/// @return full ffmpeg argument list compositing the project's tracks into `out`
pub fn ffmpeg_args(project: &Project, dir: &Path, out: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec!["-hide_banner".into(), "-nostdin".into()];
    let mut inputs = 0;
    let mut input = |args: &mut Vec<String>, file: &str, offset_s: f64| {
        if offset_s > 0.001 {
            args.extend(["-itsoffset".into(), format!("{offset_s:.3}")]);
        } else if offset_s < -0.001 {
            args.extend(["-ss".into(), format!("{:.3}", -offset_s)]);
        }
        args.extend(["-i".into(), dir.join(file).display().to_string()]);
        inputs += 1;
        inputs - 1
    };
    input(&mut args, &project.screen.file, 0.0);
    let camera = project
        .camera
        .as_ref()
        .map(|track| input(&mut args, &track.file, project.offset_s(track)));
    let mic = project
        .mic
        .as_ref()
        .map(|track| input(&mut args, &track.file, project.offset_s(track)));

    let (width, height) = fit_even(project.width, project.height);
    let diameter = even(project.bubble.d * width as f64);
    let x = (project.bubble.x * width as f64).round();
    let y = (project.bubble.y * height as f64).round();
    let mut graph = vec![format!("[0:v]fps={FPS},scale={width}:{height}[s]")];
    let mut mic_audio = mic.map(|index| format!("[{index}:a]"));

    let bubble = match (camera, mic) {
        (Some(camera), _) => Some(format!(
            "[{camera}:v]scale={diameter}:{diameter}:force_original_aspect_ratio=increase,crop={diameter}:{diameter},{}",
            circle(diameter)
        )),
        (None, Some(mic)) => {
            graph.push(format!("[{mic}:a]asplit=2[ma][mw]"));
            mic_audio = Some("[ma]".into());
            Some(format!(
                "color=c={BUBBLE_BACKGROUND}:s={diameter}x{diameter}:r=30[bg];\
                 [mw]showwaves=s={diameter}x{diameter}:mode=cline:rate=30:colors=white[wave];\
                 [bg][wave]overlay=shortest=1,{}",
                circle(diameter)
            ))
        }
        (None, None) => None,
    };
    let video = match bubble {
        Some(bubble) => {
            graph.push(format!("{bubble}[b]"));
            graph.push(format!("[s][b]overlay=x={x}:y={y}:format=auto[v]"));
            "[v]"
        }
        None => "[s]",
    };

    let sources: Vec<String> = project
        .system_audio
        .then(|| "[0:a]".to_string())
        .into_iter()
        .chain(mic_audio)
        .collect();
    if !sources.is_empty() {
        let mut labels = String::new();
        for (i, source) in sources.iter().enumerate() {
            graph.push(format!("{source}aresample=48000:async=1:first_pts=0[a{i}]"));
            labels.push_str(&format!("[a{i}]"));
        }
        graph.push(format!(
            "{labels}amix=inputs={}:normalize=0:duration=longest,apad[a]",
            sources.len()
        ));
    }

    args.extend(["-filter_complex".into(), graph.join(";")]);
    args.extend(["-map".into(), video.into()]);
    if sources.is_empty() {
        args.push("-an".into());
    } else {
        args.extend(["-map", "[a]", "-c:a", "aac", "-b:a", "192k"].map(String::from));
    }
    args.extend(["-t".into(), format!("{:.3}", project.duration_s)]);
    args.extend(
        [
            "-c:v",
            "h264_videotoolbox",
            "-b:v",
            "20M",
            "-pix_fmt",
            "yuv420p",
            "-tag:v",
            "avc1",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            "-y",
        ]
        .map(String::from),
    );
    args.push(out.display().to_string());
    args
}

fn circle(diameter: u32) -> String {
    let r = diameter as f64 / 2.0;
    format!(
        "format=yuva420p,geq=lum='p(X,Y)':cb='p(X,Y)':cr='p(X,Y)':a='clip(255*({r}-hypot(X+0.5-{r},Y+0.5-{r})),0,255)'"
    )
}

/// @return output size fitting a 4K box (H.264 hardware encoder limit), even dimensions
pub fn fit_even(width: u32, height: u32) -> (u32, u32) {
    let (box_w, box_h) = if width >= height {
        LANDSCAPE_MAX
    } else {
        (LANDSCAPE_MAX.1, LANDSCAPE_MAX.0)
    };
    let scale = (box_w / width as f64).min(box_h / height as f64).min(1.0);
    (even(width as f64 * scale), even(height as f64 * scale))
}

/// @return progress in 0..=1 for an ffmpeg `-progress` line, `None` for other lines
pub fn parse_progress(line: &str, duration_s: f64) -> Option<f32> {
    if line.trim() == "progress=end" {
        return Some(1.0);
    }
    let micros: f64 = line.strip_prefix("out_time_us=")?.trim().parse().ok()?;
    Some((micros / 1e6 / duration_s).clamp(0.0, 1.0) as f32)
}

/// Looks in PATH, then Homebrew locations, since apps launched from Finder get a minimal PATH.
/// @return absolute path of the command-line tool, if installed
pub fn tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from))
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{Bubble, Mode, Rect, Track, VERSION};

    fn project() -> Project {
        Project {
            version: VERSION,
            mode: Mode::Display,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 2560.0,
                h: 1440.0,
            },
            width: 5120,
            height: 2880,
            duration_s: 3.0,
            screen: Track {
                file: "screen.mp4".into(),
                start_host_ns: 10_000_000_000,
            },
            system_audio: true,
            mic: Some(Track {
                file: "mic.wav".into(),
                start_host_ns: 9_800_000_000,
            }),
            camera: Some(Track {
                file: "camera.mov".into(),
                start_host_ns: 10_500_000_000,
            }),
            bubble: Bubble {
                x: 0.75,
                y: 0.7,
                d: 0.1,
            },
        }
    }

    fn args_of(project: &Project) -> Vec<String> {
        ffmpeg_args(project, Path::new("/rec"), Path::new("/rec/export.mp4"))
    }

    fn graph(args: &[String]) -> &str {
        let at = args.iter().position(|a| a == "-filter_complex").unwrap();
        &args[at + 1]
    }

    fn before_input(args: &[String], file: &str) -> Vec<String> {
        let at = args
            .iter()
            .position(|a| a == &format!("/rec/{file}"))
            .unwrap();
        args[at - 3..at].to_vec()
    }

    #[test]
    fn camera_variant_aligns_and_masks() {
        let args = args_of(&project());
        assert_eq!(
            before_input(&args, "camera.mov"),
            ["-itsoffset", "0.500", "-i"]
        );
        assert_eq!(before_input(&args, "mic.wav"), ["-ss", "0.200", "-i"]);
        let graph = graph(&args);
        assert!(graph.starts_with("[0:v]fps=60,scale=3840:2160[s]"));
        assert!(graph.contains("[1:v]scale=384:384:force_original_aspect_ratio=increase,crop=384:384,format=yuva420p,geq="));
        assert!(graph.contains("[s][b]overlay=x=2880:y=1512:format=auto[v]"));
        assert!(graph.contains("[0:a]aresample=48000:async=1:first_pts=0[a0]"));
        assert!(graph.contains("[2:a]aresample=48000:async=1:first_pts=0[a1]"));
        assert!(graph.contains("[a0][a1]amix=inputs=2:normalize=0:duration=longest,apad[a]"));
        assert!(!graph.contains("showwaves"));
        let joined = args.join(" ");
        assert!(joined.contains("-map [v] -map [a] -c:a aac"));
        assert!(joined.contains("-t 3.000 -c:v h264_videotoolbox"));
        assert!(joined.ends_with("-progress pipe:1 -nostats -y /rec/export.mp4"));
    }

    #[test]
    fn no_camera_draws_mic_waveform() {
        let args = args_of(&Project {
            camera: None,
            ..project()
        });
        let graph = graph(&args);
        assert!(graph.contains("[1:a]asplit=2[ma][mw]"));
        assert!(graph.contains("color=c=0x1e1e1e:s=384x384:r=30[bg]"));
        assert!(graph.contains("[mw]showwaves=s=384x384:mode=cline"));
        assert!(graph.contains("[bg][wave]overlay=shortest=1,format=yuva420p,geq="));
        assert!(graph.contains("[ma]aresample=48000:async=1:first_pts=0[a1]"));
        assert!(graph.contains("[s][b]overlay="));
    }

    #[test]
    fn nothing_but_screen_is_silent_and_bare() {
        let args = args_of(&Project {
            camera: None,
            mic: None,
            system_audio: false,
            ..project()
        });
        assert_eq!(graph(&args), "[0:v]fps=60,scale=3840:2160[s]");
        let joined = args.join(" ");
        assert!(joined.contains("-map [s] -an -t 3.000"));
        assert!(!joined.contains("[a]"));
    }

    #[test]
    fn system_audio_only() {
        let args = args_of(&Project {
            camera: None,
            mic: None,
            ..project()
        });
        let graph = graph(&args);
        assert!(graph.contains(
            "[0:a]aresample=48000:async=1:first_pts=0[a0];[a0]amix=inputs=1:normalize=0"
        ));
        assert!(!graph.contains("overlay"));
    }

    #[test]
    fn small_screen_keeps_native_size() {
        let args = args_of(&Project {
            width: 2560,
            height: 1600,
            ..project()
        });
        let graph = graph(&args);
        assert!(graph.starts_with("[0:v]fps=60,scale=2560:1600[s]"));
        assert!(graph.contains("scale=256:256"));
        assert!(graph.contains("overlay=x=1920:y=1120"));
    }

    #[test]
    fn fit_even_boxes() {
        assert_eq!(fit_even(5120, 2880), (3840, 2160));
        assert_eq!(fit_even(2560, 1600), (2560, 1600));
        assert_eq!(fit_even(1281, 721), (1280, 720));
        assert_eq!(fit_even(2880, 5120), (2160, 3840));
        assert_eq!(fit_even(6016, 3384), (3840, 2160));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("out_time_us=1500000", 3.0), Some(0.5));
        assert_eq!(parse_progress("out_time_us=9000000", 3.0), Some(1.0));
        assert_eq!(parse_progress("out_time_us=N/A", 3.0), None);
        assert_eq!(parse_progress("progress=end", 3.0), Some(1.0));
        assert_eq!(parse_progress("progress=continue", 3.0), None);
        assert_eq!(parse_progress("frame=12", 3.0), None);
    }

    fn synthesize(dir: &Path, args: &str) {
        let status = Command::new(tool("ffmpeg").unwrap())
            .current_dir(dir)
            .args(["-hide_banner", "-loglevel", "error", "-y"])
            .args(args.split_whitespace())
            .status()
            .unwrap();
        assert!(status.success(), "ffmpeg {args}");
    }

    fn probe(file: &Path) -> Vec<(String, String)> {
        let output = Command::new(tool("ffprobe").unwrap())
            .args(["-v", "error", "-count_frames", "-show_entries"])
            .args(["stream=codec_type,start_time,nb_read_frames:format=duration"])
            .args(["-of", "default=noprint_wrappers=1"])
            .arg(file)
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    #[ignore = "needs ffmpeg + VideoToolbox"]
    fn export_runs_on_synthetic_inputs() {
        let dir = std::env::temp_dir().join(format!("taywind-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        synthesize(
            &dir,
            "-f lavfi -i testsrc2=s=1280x720:r=2:d=3 -f lavfi -i sine=f=440:d=3:sample_rate=48000 \
             -c:v hevc_videotoolbox -tag:v hvc1 -c:a aac screen.mp4",
        );
        synthesize(
            &dir,
            "-f lavfi -i testsrc=s=1280x720:r=30:d=3 -c:v h264_videotoolbox camera.mov",
        );
        synthesize(
            &dir,
            "-f lavfi -i sine=f=880:d=4:sample_rate=48000 -ac 1 mic.wav",
        );

        let with_camera = Project {
            width: 1280,
            height: 720,
            ..project()
        };
        let late_mic_only = Project {
            camera: None,
            system_audio: false,
            mic: Some(Track {
                file: "mic.wav".into(),
                start_host_ns: with_camera.screen.start_host_ns + 1_000_000_000,
            }),
            ..with_camera.clone()
        };
        for project in [with_camera, late_mic_only] {
            project.save(&dir).unwrap();
            let mut progress = Vec::new();
            let out = run(&dir, |p| progress.push(p)).unwrap();
            assert_eq!(progress.last(), Some(&1.0));

            let info = probe(&out);
            let value = |key: &str| {
                info.iter()
                    .filter(|(k, _)| k == key)
                    .map(|(_, v)| v.as_str())
                    .collect::<Vec<_>>()
            };
            assert_eq!(value("codec_type"), ["video", "audio"]);
            for start in value("start_time") {
                assert!(
                    start.parse::<f64>().unwrap().abs() < 0.05,
                    "starts at {start}"
                );
            }
            let duration: f64 = value("duration")[0].parse().unwrap();
            assert!((duration - project.duration_s).abs() < 0.1, "{duration}");
            let frames: f64 = value("nb_read_frames")[0].parse().unwrap();
            let expected = project.duration_s * FPS as f64;
            assert!((frames - expected).abs() <= 2.0, "{frames} frames");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
