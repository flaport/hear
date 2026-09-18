use crate::{app::AppEvent, config::HearConfig, credentials};
use hear_core::{dictation::PendingRecording, process::Job};
use std::path::{Path, PathBuf};
use winit::event_loop::EventLoopProxy;
pub fn transcribe(
    pending: PendingRecording,
    proxy: EventLoopProxy<AppEvent>,
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
        let _ = proxy.send_event(AppEvent::TranscriptionFinished(result));
    })
}
pub(crate) fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HEAR_HELPER_PATH") {
        return path.into();
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(contents) = executable.parent().and_then(Path::parent)
    {
        let bundled = contents.join("Helpers").join("hear");
        if bundled.is_file() {
            return bundled;
        }
    }
    PathBuf::from("hear")
}
