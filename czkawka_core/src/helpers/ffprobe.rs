//! Minimal `ffprobe` JSON reader, trimmed from ffprobe-rs (MIT) to the fields czkawka reads.

use std::path::Path;
use std::process::{Command, Output};

use serde::Deserialize;

use crate::common::process_utils::disable_windows_console_window;

/// Runs `ffprobe` on `path` and parses its format and stream info.
pub fn ffprobe(path: impl AsRef<Path>) -> Result<FfProbe, FfProbeError> {
    let mut cmd = Command::new("ffprobe");
    cmd.args(["-v", "error", "-show_format", "-show_streams", "-print_format", "json"]).arg(path.as_ref());
    disable_windows_console_window(&mut cmd);

    let out = cmd.output().map_err(FfProbeError::Io)?;
    if !out.status.success() {
        return Err(FfProbeError::Status(out));
    }
    serde_json::from_slice(&out.stdout).map_err(FfProbeError::Deserialize)
}

#[derive(Debug)]
pub enum FfProbeError {
    Io(std::io::Error),
    Status(Output),
    Deserialize(serde_json::Error),
}

impl std::fmt::Display for FfProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Status(o) => write!(f, "ffprobe exited with status code {}: {}", o.status, String::from_utf8_lossy(&o.stderr)),
            Self::Deserialize(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for FfProbeError {}

#[derive(Debug, Deserialize)]
pub struct FfProbe {
    pub streams: Vec<Stream>,
    pub format: Format,
}

#[derive(Debug, Deserialize)]
pub struct Stream {
    pub codec_name: Option<String>,
    pub codec_type: Option<String>,
    pub r_frame_rate: String,
    pub avg_frame_rate: String,
    pub bit_rate: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct Format {
    pub duration: Option<String>,
    pub bit_rate: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parses_ffprobe_json_ignoring_unknown_fields() {
        let json = r#"{
            "streams": [
                {"index": 0, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080,
                 "r_frame_rate": "30/1", "avg_frame_rate": "30000/1001", "bit_rate": "4000000",
                 "disposition": {"default": 1}, "tags": {"language": "und"}},
                {"index": 1, "codec_name": "aac", "codec_type": "audio", "r_frame_rate": "0/0", "avg_frame_rate": "0/0"}
            ],
            "format": {"filename": "a.mp4", "nb_streams": 2, "duration": "12.5", "bit_rate": "4100000", "probe_score": 100}
        }"#;
        let info: FfProbe = serde_json::from_str(json).unwrap();

        assert_eq!(info.format.duration.as_deref(), Some("12.5"));
        assert_eq!(info.format.bit_rate.as_deref(), Some("4100000"));
        let video = &info.streams[0];
        assert_eq!((video.codec_type.as_deref(), video.codec_name.as_deref()), (Some("video"), Some("h264")));
        assert_eq!((video.width, video.height), (Some(1920), Some(1080)));
        assert_eq!((video.avg_frame_rate.as_str(), video.r_frame_rate.as_str()), ("30000/1001", "30/1"));
        assert_eq!(video.bit_rate.as_deref(), Some("4000000"));
        assert_eq!(info.streams[1].width, None);
    }
}
