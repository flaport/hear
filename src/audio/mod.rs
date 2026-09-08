use anyhow::{Result, bail};
use std::{
    io::{self, IsTerminal},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingOutcome {
    Completed,
    Cancelled,
}
pub fn record(destination: &Path, force: bool) -> Result<RecordingOutcome> {
    if !io::stdin().is_terminal() {
        bail!("recording requires an interactive terminal");
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    ctrlc::set_handler(move || {
        if flag.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    })?;
    let recorder = hear_core::capture::Recorder::start()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = tx.send(io::stdin().read_line(&mut line));
    });
    eprintln!("Recording; press Return to finish or Ctrl-C to cancel...");
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Ok(RecordingOutcome::Cancelled);
        }
        recorder.check()?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(0)) => bail!("standard input closed"),
            Ok(Ok(_)) => break,
            Ok(Err(e)) => return Err(e.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => return Err(e.into()),
        }
    }
    let recording = recorder.finish()?;
    hear_core::files::copy(&recording, destination, force)?;
    // Subsequent Ctrl-C must terminate transcription instead of setting an unused flag.
    cancelled.store(true, Ordering::SeqCst);
    Ok(RecordingOutcome::Completed)
}
