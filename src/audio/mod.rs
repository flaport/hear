mod capture;
pub(crate) mod processing;
pub(crate) mod wav;

use std::path::Path;

use anyhow::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingOutcome {
    Completed,
    Cancelled,
}

pub fn record(destination: &Path) -> Result<RecordingOutcome> {
    let Some(recording) = capture::capture()? else {
        return Ok(RecordingOutcome::Cancelled);
    };
    let samples = processing::resample(&recording.samples, recording.sample_rate, 16_000);
    wav::write(destination, &samples)?;
    eprintln!("Recording complete ({}).", destination.display());
    Ok(RecordingOutcome::Completed)
}
