use crate::{app::AppEvent, config::HearConfig, credentials};
use hear_core::{dictation::PendingRecording, process::Job};
use std::path::PathBuf;
use std::sync::mpsc;
pub fn transcribe_async(
    pending: PendingRecording,
    tx: mpsc::Sender<AppEvent>,
    hear: HearConfig,
) -> Job {
    Job::spawn(move |cancellation| {
        let result = (|| {
            let (transcript, recording) = pending.transcribe(
                &hear,
                helper_path(),
                credentials::stored_api_key,
                &cancellation,
            )?;
            Ok((transcript.text, recording))
        })()
        .map_err(|e: anyhow::Error| format!("{e:#}"));
        let _ = tx.send(AppEvent::TranscriptionFinished(result));
    })
}
pub(crate) fn helper_path() -> PathBuf {
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
