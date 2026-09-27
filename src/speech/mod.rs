//! Blocking speech synthesis, independent of capture and native inference.
//!
//! Audio sinks receive ordered mono PCM16 samples at 24 kHz. Run synthesis on a
//! worker thread; sinks and observers run synchronously there, never on an audio
//! callback. Completion means delivery to the sink, not audible playback.
//! Cancellation is checked between reads and sink calls. In-flight HTTP waits
//! are bounded by the engine's total request timeout (60 seconds by default);
//! sink/observer implementations must bound their own blocking operations.
mod openai;
pub use hear_core::process::Cancellation;
pub use openai::OpenAiSpeech;

use std::{
    io::{Seek, Write},
    path::Path,
    time::Duration,
};

pub const SAMPLE_RATE: u32 = 24_000;
pub const DEFAULT_MODEL: &str = "gpt-4o-mini-tts";
pub const DEFAULT_VOICE: &str = "cedar";

/// Complete input text. Audio output streams; incremental text input is not yet supported.
#[derive(Debug, Clone)]
pub struct SpeechRequest<'a> {
    pub text: &'a str,
    pub voice: &'a str,
    pub model: &'a str,
    pub speed: f32,
    pub instructions: Option<&'a str>,
}
impl<'a> SpeechRequest<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            voice: DEFAULT_VOICE,
            model: DEFAULT_MODEL,
            speed: 1.0,
            instructions: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SpeechSummary {
    pub engine: String,
    pub model: String,
    pub voice: String,
    pub samples: u64,
    pub sample_rate: u32,
    pub audio_duration: Duration,
    /// Request start to first decoded samples, before the first sink call.
    pub first_audio: Duration,
    /// Includes HTTP transfer, decoding, and synchronous sink/observer calls.
    pub request_duration: Duration,
}

#[derive(Debug, Clone)]
pub enum SpeechEvent {
    Started,
    FirstAudio(Duration),
    Completed(SpeechSummary),
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
#[non_exhaustive]
pub enum SpeechError {
    Configuration(String),
    OpenAi(crate::Error),
    Network(std::io::Error),
    Decode(String),
    Output(anyhow::Error),
    Cancelled,
}
impl std::fmt::Display for SpeechError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(s) => write!(f, "invalid speech request: {s}"),
            Self::OpenAi(e) => write!(f, "{e}"),
            Self::Network(e) => write!(f, "speech audio transfer failed: {e}"),
            Self::Decode(s) => write!(f, "invalid speech audio: {s}"),
            Self::Output(e) => write!(f, "speech output failed: {e:#}"),
            Self::Cancelled => write!(f, "speech cancelled"),
        }
    }
}
impl std::error::Error for SpeechError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::OpenAi(e) => Some(e),
            Self::Network(e) => Some(e),
            Self::Output(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}
pub type Result<T> = std::result::Result<T, SpeechError>;

/// Each slice contains new mono 24 kHz PCM16 samples; consume or copy before returning.
pub trait SpeechAudioSink {
    fn write(&mut self, samples: &[i16]) -> Result<()>;
}
impl<F: FnMut(&[i16]) -> Result<()>> SpeechAudioSink for F {
    fn write(&mut self, samples: &[i16]) -> Result<()> {
        self(samples)
    }
}

pub trait SpeechEngine {
    fn synthesize(
        &self,
        request: &SpeechRequest<'_>,
        sink: &mut dyn SpeechAudioSink,
        cancellation: &Cancellation,
    ) -> Result<SpeechSummary>;
}

pub(crate) fn check_cancelled(cancellation: &Cancellation) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(SpeechError::Cancelled)
    } else {
        Ok(())
    }
}

/// WAV sink for the SDK's mono PCM16/24 kHz audio contract.
pub struct WavSink<W: Write + Seek>(hound::WavWriter<W>);
impl<W: Write + Seek> WavSink<W> {
    pub fn new(writer: W) -> Result<Self> {
        hound::WavWriter::new(
            writer,
            hound::WavSpec {
                channels: 1,
                sample_rate: SAMPLE_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .map(Self)
        .map_err(|e| SpeechError::Output(e.into()))
    }
    /// Finalize the WAV header after successful synthesis.
    pub fn finish(self) -> Result<()> {
        self.0.finalize().map_err(|e| SpeechError::Output(e.into()))
    }
}
impl<W: Write + Seek> SpeechAudioSink for WavSink<W> {
    fn write(&mut self, samples: &[i16]) -> Result<()> {
        for &sample in samples {
            self.0
                .write_sample(sample)
                .map_err(|e| SpeechError::Output(e.into()))?;
        }
        Ok(())
    }
}

/// Publish a complete WAV atomically. Errors and cancellation leave existing files intact.
pub fn synthesize_to_wav(
    engine: &dyn SpeechEngine,
    request: &SpeechRequest<'_>,
    path: &Path,
    force: bool,
    cancellation: &Cancellation,
) -> Result<SpeechSummary> {
    check_cancelled(cancellation)?;
    hear_core::files::preflight(path, force).map_err(SpeechError::Output)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| SpeechError::Output(e.into()))?;
    let mut sink = WavSink::new(std::io::BufWriter::new(temporary.as_file_mut()))?;
    let summary = engine.synthesize(request, &mut sink, cancellation)?;
    sink.finish()?;
    check_cancelled(cancellation)?;
    if force {
        temporary.persist(path)
    } else {
        temporary.persist_noclobber(path)
    }
    .map_err(|e| SpeechError::Output(e.error.into()))?;
    Ok(summary)
}
