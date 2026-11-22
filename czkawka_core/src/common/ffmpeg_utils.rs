use std::process::{Command, Stdio};

use crate::common::process_utils::disable_windows_console_window;

pub fn check_if_ffprobe_ffmpeg_exists() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|program| {
        let mut command = Command::new(program);
        disable_windows_console_window(&mut command);
        command.arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    })
}

/// Returns the path of the first available DRI render node, or None if none exist.
pub fn find_vaapi_device() -> Option<String> {
    (128..=132u32)
        .map(|index| format!("/dev/dri/renderD{index}"))
        .find(|device| std::path::Path::new(device).exists())
}
