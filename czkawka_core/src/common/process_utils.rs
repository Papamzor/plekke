use std::io::{self, Read};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use log::error;

use crate::flc;
use crate::helpers::long_operation_watcher::LongOperationWatcher;

#[cfg_attr(not(target_os = "windows"), expect(clippy::needless_pass_by_ref_mut))]
pub fn disable_windows_console_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
    }
}

pub struct CommandOutput {
    pub status: std::process::ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

// Remember - Ok returned by this function does not necessarily mean that the command executed successfully
// it only means that the command was executed and its output was captured.
// The actual success of the command should be determined by checking the `status` field of the returned `CommandOutput`.
pub fn run_command_interruptible(mut command: Command, stop_flag: &Arc<AtomicBool>) -> Option<Result<CommandOutput, String>> {
    if stop_flag.load(Ordering::Relaxed) {
        return None;
    }

    disable_windows_console_window(&mut command);

    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => return Some(Err(flc!("core_failed_to_spawn_command", reason = e.to_string()))),
    };

    let Some(mut stdout) = child.stdout.take() else {
        error!("Failed to take stdout from child process");
        return Some(Err("Failed to take stdout from child process".to_string()));
    };
    let Some(mut stderr) = child.stderr.take() else {
        error!("Failed to take stderr from child process");
        return Some(Err("Failed to take stderr from child process".to_string()));
    };

    let out_handle = thread::spawn(move || read_to_end(&mut stdout));
    let err_handle = thread::spawn(move || read_to_end(&mut stderr));

    // pid is unique only while the child is alive, which is exactly the watcher's lifetime here.
    let _watch = LongOperationWatcher::watch(&format!("command {command:?}"), &child.id().to_string());

    loop {
        if stop_flag.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }

        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => thread::sleep(Duration::from_millis(100)),
            Err(e) => return Some(Err(flc!("core_failed_to_check_process_status", reason = e.to_string()))),
        }
    }

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return Some(Err(flc!("core_failed_to_wait_for_process", reason = e.to_string()))),
    };

    let stdout = out_handle.join().unwrap_or_default();
    let stderr = err_handle.join().unwrap_or_default();

    if stop_flag.load(Ordering::Relaxed) {
        return None;
    }

    Some(Ok(CommandOutput {
        status,
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    }))
}

fn read_to_end(reader: &mut impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    // Partial output is still useful; read errors only truncate it.
    let _ = io::copy(reader, &mut buf);
    buf
}
