use crate::{app::AppEvent, config::HearConfig, credentials};
use hear_core::{capture::PendingRecording, helper::Recording, process::Job};
use std::path::{Path, PathBuf};
use winit::event_loop::EventLoopProxy;
pub fn transcribe(
    pending: PendingRecording,
    proxy: EventLoopProxy<AppEvent>,
    hear: HearConfig,
) -> Job {
    Job::spawn(move |cancellation| {
        let result = (|| {
            let mut recording = Recording::new(pending.finish()?);
            let text = hear_core::helper::transcribe(
                &recording,
                &hear,
                helper_path(),
                credentials::stored_api_key,
                &cancellation,
            )?;
            recording.remember_transcript(&text);
            Ok((text, recording))
        })()
        .map_err(|e: anyhow::Error| format!("{e:#}"));
        let _ = proxy.send_event(AppEvent::TranscriptionFinished(result));
    })
}
fn helper_path() -> PathBuf {
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
