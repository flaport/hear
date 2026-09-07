use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::delivery;
use crate::recording::Recorder;
use crate::transcriber;

fn pid_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
    PathBuf::from(dir).join("hear-linux-oneshot.pid")
}

pub fn run() -> Result<()> {
    let pid_file = pid_path();

    if signal_running_instance(&pid_file) {
        return Ok(());
    }

    write_pid_file(&pid_file)?;
    let _cleanup = PidCleanup(&pid_file);

    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGUSR1,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&stop))
            .context("could not register signal handler")?;
    }

    eprintln!("Recording… (run `hear-linux oneshot` again to stop)");
    let recorder = Recorder::start().context("could not start recording")?;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }

    eprintln!("Transcribing…");
    let recording = recorder.finish().context("could not finish recording")?;
    let transcript = transcriber::run(&recording)?;

    match delivery::deliver(&transcript, true) {
        Ok(true) => eprintln!("Pasted."),
        Ok(false) => eprintln!("Copied to clipboard."),
        Err(error) => eprintln!("Could not paste: {error:#}. Copied to clipboard."),
    }

    print!("{transcript}");
    Ok(())
}

fn signal_running_instance(pid_file: &PathBuf) -> bool {
    let Ok(contents) = fs::read_to_string(pid_file) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<i32>() else {
        return false;
    };
    unsafe {
        if libc::kill(pid, 0) == 0 {
            libc::kill(pid, libc::SIGUSR1);
            return true;
        }
    }
    let _ = fs::remove_file(pid_file);
    false
}

fn write_pid_file(pid_file: &PathBuf) -> Result<()> {
    let mut file = fs::File::create(pid_file).context("could not create PID file")?;
    write!(file, "{}", std::process::id())?;
    Ok(())
}

struct PidCleanup<'a>(&'a PathBuf);

impl Drop for PidCleanup<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}
