#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{process::Command, time::Duration};

use regex::Regex;
use serde::Deserialize;

fn default_interval() -> u64 {
    0
}

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_interval")]
    interval: u64,
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
struct Task {
    pattern: String,
    command: String,
}

// TODO: Support Linux, macOS
#[cfg(windows)]
fn get_foreground_pid() -> Result<Option<u32>, String> {
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    let mut _thread_id = 0;
    let mut process_id = 0;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        return Ok(None);
    }

    _thread_id = unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };

    if process_id != 0 {
        Ok(Some(process_id))
    } else {
        Err(unsafe { format!("0x{:x}", GetLastError()) })
    }
}

// TODO: Refactor, maybe into an Iterator of exe paths
// - Separate command-launching from watching
// TODO: Watch window title or other attributes as well
async fn watch_window(config: Config) {
    let mut old_process_id = None;

    loop {
        match get_foreground_pid() {
            Ok(Some(process_id)) => {
                log::trace!("{}", process_id);

                if Some(process_id) == old_process_id {
                    log::trace!("Found same foreground process `{}`", process_id);
                    continue;
                }

                match heim::process::get(process_id).await {
                    Ok(process) => {
                        let exe_raw = process.exe().await.unwrap();

                        // TODO: Support UTF-16 executable names?
                        let exe = exe_raw.to_string_lossy();

                        log::debug!("Found foreground process `{}`", exe);

                        for task in &config.tasks {
                            if let Ok(regex) = Regex::new(&task.pattern) {
                                if regex.is_match(&exe) {
                                    log::debug!("Process `{}` matches {}", exe, regex);
                                    for cmd in task.command.lines() {
                                        log::debug!("Executing {}", cmd);

                                        // TODO: Support running as parent process? Or kill all spawned processes on exit
                                        if let Some((cmd, args)) = cmd.split_once(' ') {
                                            Command::new(cmd).args(args.split(' ')).spawn().ok();
                                        } else {
                                            Command::new(cmd).spawn().ok();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        log::warn!(
                            "Unable to retrieve information of foreground process {}: {}",
                            process_id,
                            err
                        );
                    }
                }

                old_process_id = Some(process_id);
            }
            result => {
                log::info!("Unable to retrieve PID of foreground window: {:?}", result);
            }
        }

        // I know this is blocking, but it doesn't matter here.
        std::thread::sleep(Duration::from_micros(config.interval));
    }
}

fn main() {
    #[cfg(debug_assertions)]
    const DEFAULT_LOG: &'static str = "DEBUG";

    #[cfg(not(debug_assertions))]
    const DEFAULT_LOG: &'static str = "WARN";

    kaf::from_str(&std::env::var("WUST_LOG").unwrap_or(DEFAULT_LOG.into()));

    let toml_path = std::env::args()
        .skip(1) // TODO: Switch to a proper crate like clap. This is unreliable.
        .next()
        .unwrap_or("~/.config/wust/config.toml".to_string());

    let toml = std::fs::read_to_string(std::fs::canonicalize(toml_path).unwrap()).unwrap();
    let config = toml::from_str(&toml).unwrap();

    futures::executor::block_on(watch_window(config));
}
