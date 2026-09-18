//! Recording lifecycle shared by desktop companions and the interactive CLI.
use crate::{
    HearConfig, capture,
    helper::{self, Recording, Streaming, Transcript},
    process::Cancellation,
};
use anyhow::Result;
use std::path::PathBuf;

pub struct Recorder {
    capture: capture::Recorder,
    streaming: Option<Streaming>,
}
pub struct PendingRecording {
    capture: capture::PendingRecording,
    streaming: Option<Streaming>,
}
impl Recorder {
    pub fn start(
        config: &HearConfig,
        helper: PathBuf,
        key: impl FnOnce() -> Result<Option<String>>,
    ) -> Result<Self> {
        config.preflight()?;
        let (streaming, sink) = if config.stream {
            let (streaming, sink) = Streaming::start(config, helper, key)?;
            (Some(streaming), Some(sink))
        } else {
            (None, None)
        };
        Ok(Self {
            capture: capture::Recorder::start_with_sink(sink)?,
            streaming,
        })
    }
    pub fn check(&self) -> Result<()> {
        self.capture.check()
    }
    pub fn stop(self) -> PendingRecording {
        PendingRecording {
            capture: self.capture.stop(),
            streaming: self.streaming,
        }
    }
}
impl PendingRecording {
    pub fn transcribe(
        self,
        config: &HearConfig,
        helper: PathBuf,
        key: impl FnOnce() -> Result<Option<String>>,
        cancellation: &Cancellation,
    ) -> Result<(Transcript, Recording)> {
        let mut recording = Recording::new(self.capture.finish()?);
        let text = if let Some(streaming) = self.streaming {
            if let Some(destination) = config.save_recording_path() {
                crate::files::copy(recording.path(), destination, config.force)?;
            }
            streaming.finish(&recording, cancellation)?
        } else {
            helper::transcribe_full(&recording, config, helper, key, cancellation)?
        };
        recording.remember_transcript(&text.raw);
        Ok((text, recording))
    }
}
