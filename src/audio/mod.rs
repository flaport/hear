use anyhow::{Result, bail};
use std::{
    io::{self, IsTerminal},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
#[derive(Debug)]
pub enum RecordingOutcome {
    Completed(tempfile::TempPath),
    Cancelled,
}
pub fn record() -> Result<RecordingOutcome> {
    require_terminal()?;
    let recorder = hear::Recorder::start()?;
    if !wait(|| recorder.check())? {
        return Ok(RecordingOutcome::Cancelled);
    }
    Ok(RecordingOutcome::Completed(recorder.finish()?))
}

pub fn record_stream(
    config: &hear::HearConfig,
) -> Result<Option<(hear::Transcript, hear_core::helper::Recording)>> {
    require_terminal()?;
    let helper = std::env::current_exe()?;
    let cancellation = hear_core::process::Cancellation::default();
    let signal = cancellation.clone();
    ctrlc::set_handler(move || signal.cancel())?;
    let recorder = hear_core::dictation::Recorder::start(config, helper.clone(), || Ok(None))?;
    if !wait_for_stop(|| recorder.check(), || cancellation.is_cancelled())? {
        return Ok(None);
    }
    let result = recorder
        .stop()
        .transcribe(config, helper, || Ok(None), &cancellation);
    if cancellation.is_cancelled() {
        return Ok(None);
    }
    result.map(Some)
}

fn require_terminal() -> Result<()> {
    if !io::stdin().is_terminal() {
        bail!("recording requires an interactive terminal");
    }
    Ok(())
}
fn wait(check: impl Fn() -> Result<()>) -> Result<bool> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = cancelled.clone();
    ctrlc::set_handler(move || {
        if flag.swap(true, Ordering::SeqCst) {
            std::process::exit(130);
        }
    })?;
    let completed = wait_for_stop(check, || cancelled.load(Ordering::SeqCst))?;
    // The next Ctrl-C terminates the post-recording work.
    cancelled.store(true, Ordering::SeqCst);
    Ok(completed)
}
fn wait_for_stop(check: impl Fn() -> Result<()>, cancelled: impl Fn() -> bool) -> Result<bool> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = tx.send(io::stdin().read_line(&mut line));
    });
    eprintln!("Recording; press Return to finish or Ctrl-C to cancel...");
    loop {
        if cancelled() {
            return Ok(false);
        }
        check()?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Ok(0)) => bail!("standard input closed"),
            Ok(Ok(_)) => break,
            Ok(Err(e)) => return Err(e.into()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(true)
}
