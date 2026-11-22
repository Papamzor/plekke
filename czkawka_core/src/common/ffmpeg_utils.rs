use std::process::{Command, Stdio};

use crate::common::process_utils::disable_windows_console_window;
use crate::flc;
use crate::helpers::messages::Messages;

pub fn check_if_ffprobe_ffmpeg_exists() -> bool {
    ["ffmpeg", "ffprobe"].iter().all(|program| {
        let mut command = Command::new(program);
        disable_windows_console_window(&mut command);
        command.arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    })
}

/// Returns false and records a critical error (plus an install hint on Windows) when ffmpeg or ffprobe is missing.
pub(crate) fn ffmpeg_available(messages: &mut Messages) -> bool {
    if check_if_ffprobe_ffmpeg_exists() {
        return true;
    }
    messages.critical = Some(flc!("core_ffmpeg_not_found"));
    #[cfg(target_os = "windows")]
    messages.errors.push(flc!("core_ffmpeg_not_found_windows"));
    false
}

/// Returns the path of the first available DRI render node, or None if none exist.
pub fn find_vaapi_device() -> Option<String> {
    (128..=132u32)
        .map(|index| format!("/dev/dri/renderD{index}"))
        .find(|device| std::path::Path::new(device).exists())
}
