use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::recording::Recorder;

pub fn run(config: Config) -> Result<()> {
    let pid_file = pid_path();
    if signal_running_instance(&pid_file) {
        return Ok(());
    }

    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGUSR1,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&stop))
            .context("could not register signal handler")?;
    }
    write_pid_file(&pid_file)?;
    let _cleanup = PidCleanup(&pid_file);

    eprintln!("Recording… (run `hear-app oneshot` again to stop)");
    let recorder = Recorder::start()?;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }

    let recording = recorder.finish()?;
    eprintln!("Transcribing…");
    let transcript = crate::transcriber::run(&recording, &config.hear_options)?;
    if crate::delivery::deliver(&transcript, config.paste_automatically, &config)? {
        eprintln!("Pasted.");
    } else {
        eprintln!("Copied to clipboard.");
    }
    print!("{transcript}");
    Ok(())
}

fn pid_path() -> PathBuf {
    let directory = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    directory.join(format!(
        "hear-linux-oneshot-{}.pid",
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    ))
}

fn signal_running_instance(pid_file: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(pid_file) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<i32>() else {
        let _ = fs::remove_file(pid_file);
        return false;
    };

    // SAFETY: kill with signal 0 only checks whether the process exists. SIGUSR1
    // is sent only after that check succeeds.
    unsafe {
        if libc::kill(pid, 0) == 0 {
            libc::kill(pid, libc::SIGUSR1);
            return true;
        }
    }
    let _ = fs::remove_file(pid_file);
    false
}

fn write_pid_file(pid_file: &Path) -> Result<()> {
    let mut file = fs::File::create(pid_file).context("could not create the one-shot PID file")?;
    write!(file, "{}", std::process::id())?;
    Ok(())
}

struct PidCleanup<'a>(&'a Path);

impl Drop for PidCleanup<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}
