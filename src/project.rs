use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const FPS: u32 = 60;
const FILE: &str = "project.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub mode: Mode,
    /// Captured area in global display points, top-left origin.
    pub rect: Rect,
    /// Screen track size in pixels.
    pub width: u32,
    pub height: u32,
    pub duration_s: f64,
    pub screen: Track,
    pub system_audio: bool,
    pub mic: Option<Track>,
    pub camera: Option<Track>,
    pub bubble: Bubble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Display,
    Window,
    Region,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub file: String,
    /// Host clock time (ns) of the track's first sample.
    pub start_host_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Bubble placement relative to the captured frame: `x`, `y` are the top-left corner as
/// fractions of frame width and height, `d` is the diameter as a fraction of frame width.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bubble {
    pub x: f64,
    pub y: f64,
    pub d: f64,
}

impl Project {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        Ok(serde_json::from_str(&fs::read_to_string(dir.join(FILE))?)?)
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        fs::write(dir.join(FILE), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// @return seconds `track` starts after the screen track; negative when it starts earlier
    pub fn offset_s(&self, track: &Track) -> f64 {
        (track.start_host_ns as i128 - self.screen.start_host_ns as i128) as f64 / 1e9
    }
}

/// @param bubble bubble window bounds in global points
/// @param frame captured area in global points
/// @return bubble placement clamped inside the frame
pub fn normalize_bubble(bubble: Rect, frame: Rect) -> Bubble {
    let d = (bubble.w / frame.w).min(1.0).min(frame.h / frame.w);
    let d_y = d * frame.w / frame.h;
    Bubble {
        x: ((bubble.x - frame.x) / frame.w).clamp(0.0, 1.0 - d),
        y: ((bubble.y - frame.y) / frame.h).clamp(0.0, 1.0 - d_y),
        d,
    }
}

/// @return `pixels` rounded, then lowered to an even integer, as H.264/HEVC require
pub fn even(pixels: f64) -> u32 {
    (pixels.round() as u32) & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Project {
        Project {
            version: VERSION,
            mode: Mode::Region,
            rect: Rect {
                x: 100.0,
                y: 50.0,
                w: 1280.0,
                h: 720.0,
            },
            width: 2560,
            height: 1440,
            duration_s: 12.5,
            screen: Track {
                file: "screen.mp4".into(),
                start_host_ns: 1_000_000_000,
            },
            system_audio: true,
            mic: Some(Track {
                file: "mic.wav".into(),
                start_host_ns: 800_000_000,
            }),
            camera: Some(Track {
                file: "camera.mov".into(),
                start_host_ns: 2_250_000_000,
            }),
            bubble: Bubble {
                x: 0.8,
                y: 0.7,
                d: 0.15,
            },
        }
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("taywind-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn roundtrip_full_and_minimal() {
        let dir = temp_dir("project");
        let full = sample();
        full.save(&dir).unwrap();
        assert_eq!(Project::load(&dir).unwrap(), full);

        let minimal = Project {
            mic: None,
            camera: None,
            system_audio: false,
            ..sample()
        };
        minimal.save(&dir).unwrap();
        let json = fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(json.contains("\"mode\": \"region\""));
        assert_eq!(Project::load(&dir).unwrap(), minimal);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn offsets_are_relative_to_screen() {
        let project = sample();
        assert_eq!(project.offset_s(project.camera.as_ref().unwrap()), 1.25);
        assert!((project.offset_s(project.mic.as_ref().unwrap()) + 0.2).abs() < 1e-9);
    }

    #[test]
    fn normalize_inside_frame() {
        let frame = Rect {
            x: 100.0,
            y: 50.0,
            w: 1000.0,
            h: 500.0,
        };
        let bubble = normalize_bubble(
            Rect {
                x: 300.0,
                y: 150.0,
                w: 200.0,
                h: 200.0,
            },
            frame,
        );
        assert_eq!(
            bubble,
            Bubble {
                x: 0.2,
                y: 0.2,
                d: 0.2
            }
        );
    }

    #[test]
    fn normalize_clamps_overflow() {
        let frame = Rect {
            x: 0.0,
            y: 0.0,
            w: 1000.0,
            h: 500.0,
        };
        let bubble = normalize_bubble(
            Rect {
                x: 950.0,
                y: -80.0,
                w: 200.0,
                h: 200.0,
            },
            frame,
        );
        assert_eq!(
            bubble,
            Bubble {
                x: 0.8,
                y: 0.0,
                d: 0.2
            }
        );
    }

    #[test]
    fn normalize_bubble_larger_than_frame() {
        let frame = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 150.0,
        };
        let bubble = normalize_bubble(
            Rect {
                x: 10.0,
                y: 10.0,
                w: 200.0,
                h: 200.0,
            },
            frame,
        );
        assert_eq!(bubble.d, 0.5);
        assert_eq!((bubble.x, bubble.y), (1.0 / 30.0, 0.0));
    }

    #[test]
    fn even_rounds_down_to_even() {
        assert_eq!(even(1439.6), 1440);
        assert_eq!(even(1441.0), 1440);
        assert_eq!(even(3840.0), 3840);
    }
}
