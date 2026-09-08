use crate::{app::AppEvent, config::HearConfig, credentials};
use hear_core::{capture::PendingRecording, helper::Recording, process::Job};
use std::path::PathBuf;
use std::sync::mpsc;
pub fn transcribe_async(
    pending: PendingRecording,
    tx: mpsc::Sender<AppEvent>,
    hear: HearConfig,
) -> Job {
    Job::spawn(move |cancellation| {
        let result = (|| {
            let mut recording = Recording::new(pending.finish()?);
            let text = run(&recording, &hear, &cancellation)?;
            recording.remember_transcript(&text);
            Ok((text, recording))
        })()
        .map_err(|e: anyhow::Error| format!("{e:#}"));
        let _ = tx.send(AppEvent::TranscriptionFinished(result));
    })
}
pub(crate) fn run(
    recording: &Recording,
    hear: &HearConfig,
    cancellation: &hear_core::process::Cancellation,
) -> anyhow::Result<String> {
    hear_core::helper::transcribe(
        recording,
        hear,
        helper_path(),
        credentials::stored_api_key,
        cancellation,
    )
}
fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HEAR_HELPER_PATH") {
        return path.into();
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let sibling = directory.join("hear");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("hear")
}
