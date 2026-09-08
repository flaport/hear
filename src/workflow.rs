//! The transcription workflow shared by the CLI and library callers.
use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};

use crate::{
    Engine, FormatContext, HearConfig, OpenAiClient, PolishEngine, PolishOptions, ProgressEvent,
    Transcript, dictionary::Dictionary, engines, write_transcript,
};

/// Stage at which work is running or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Validation,
    Recording,
    Transcription,
    Polishing,
    Output,
}
impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "validation",
            Self::Recording => "recording",
            Self::Transcription => "transcription",
            Self::Polishing => "polishing",
            Self::Output => "output",
        }
    }
}

#[derive(Debug, Clone)]
pub enum WorkflowEvent {
    Stage(Stage),
    Progress(ProgressEvent),
}

/// A failed workflow, including any text already produced for recovery.
#[derive(Debug)]
pub struct WorkflowError {
    pub stage: Stage,
    pub raw: Option<String>,
    pub text: Option<String>,
    source: anyhow::Error,
}
impl WorkflowError {
    pub fn new(stage: Stage, source: impl Into<anyhow::Error>) -> Self {
        Self {
            stage,
            raw: None,
            text: None,
            source: source.into(),
        }
    }
}
impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#}", self.source)
    }
}
impl std::error::Error for WorkflowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

type Observer = Arc<dyn Fn(WorkflowEvent) + Send + Sync>;

/// Blocking workflow for all transcription and polishing engines.
///
/// Dictionary loading is explicit: the default dictionary is empty. Supply an
/// OpenAI client for explicit credentials and transport settings; otherwise an
/// OpenAI operation reads `OPENAI_API_KEY` when needed. No terminal input or
/// process-wide signal handlers are installed. Native engines may emit diagnostics
/// to stderr. Hosts should run this on a worker and own cancellation/lifetime;
/// the desktop apps use a subprocess for a hard deadline and crash isolation.
///
/// ```no_run
/// use hear::{Engine, HearConfig, Workflow};
/// let workflow = Workflow::new(HearConfig {
///     engine: Some(Engine::Whisper),
///     polish: false,
///     ..HearConfig::default()
/// });
/// let transcript = workflow.run(std::path::Path::new("speech.wav"))?;
/// assert_eq!(transcript.raw, transcript.text);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone)]
pub struct Workflow {
    config: HearConfig,
    dictionary: Dictionary,
    client: Option<OpenAiClient>,
    observer: Option<Observer>,
}
impl Workflow {
    pub fn new(config: HearConfig) -> Self {
        Self {
            config,
            dictionary: Dictionary::default(),
            client: None,
            observer: None,
        }
    }
    pub fn dictionary(mut self, dictionary: Dictionary) -> Self {
        self.dictionary = dictionary;
        self
    }
    pub fn openai_client(mut self, client: OpenAiClient) -> Self {
        self.client = Some(client);
        self
    }
    pub fn progress(mut self, observer: impl Fn(WorkflowEvent) + Send + Sync + 'static) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }
    fn report(&self, stage: Stage) {
        if let Some(observer) = &self.observer {
            observer(WorkflowEvent::Stage(stage));
        }
    }
    fn openai(&self) -> Result<OpenAiClient> {
        if let Some(client) = &self.client {
            return Ok(client.clone());
        }
        let mut builder = OpenAiClient::builder(
            std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?,
        );
        if let Some(observer) = self.observer.clone() {
            builder = builder.progress(move |event| observer(WorkflowEvent::Progress(event)));
        }
        Ok(builder.build()?)
    }

    /// Validate configuration, file aliases and destinations without changing files.
    /// Pass `None` before starting microphone capture, then `run` the completed WAV.
    pub fn preflight(&self, input: Option<&Path>) -> std::result::Result<(), WorkflowError> {
        let validate = || -> Result<()> {
            self.config.validate()?;
            self.dictionary.validate()?;
            hear_core::files::ensure_distinct(&[
                input,
                self.config.output.as_deref(),
                self.config.raw_output.as_deref(),
                self.config.save_recording.as_deref(),
            ])?;
            if let Some(input) = input {
                if !input.exists() {
                    bail!("audio file does not exist: {}", input.display());
                }
                if !input.is_file() {
                    bail!("audio input is not a file: {}", input.display());
                }
            }
            self.config.preflight()
        };
        validate().map_err(|error| WorkflowError::new(Stage::Validation, error))
    }

    /// Transcribe a file or a WAV returned by [`crate::Recorder::finish`].
    ///
    /// Applies dictionary corrections, optionally polishes, and writes configured
    /// raw/final output files. `save_recording` copies the input before inference.
    /// Always returns both texts; never writes the transcript to stdout.
    pub fn run(&self, input: &Path) -> std::result::Result<Transcript, WorkflowError> {
        self.report(Stage::Validation);
        self.preflight(Some(input))?;
        let mut stage = Stage::Recording;
        let mut raw = None;
        let mut text = None;
        let result = (|| -> Result<()> {
            if let Some(destination) = &self.config.save_recording {
                hear_core::files::copy(input, destination, self.config.force)?;
            }
            stage = Stage::Transcription;
            self.report(stage);
            let vocabulary = self.dictionary.canonical_terms();
            let mut client = self.client.clone();
            let transcript = match self.config.resolved_engine() {
                Engine::GptTranscribe => {
                    let openai = self.openai()?;
                    let transcript = openai.transcribe_raw(input, &vocabulary)?;
                    client = Some(openai);
                    transcript
                }
                Engine::Codex => {
                    engines::codex::transcribe(input, self.config.model.as_deref(), &vocabulary)?
                }
                Engine::Whisper => engines::whisper::transcribe(
                    input,
                    self.config.model.as_deref().unwrap_or("tiny.en"),
                    self.config.language.as_deref().unwrap_or("en"),
                    &vocabulary,
                )?,
            };
            raw = Some(transcript);
            let corrected = self.dictionary.correct_aliases(raw.as_deref().unwrap())?;
            raw = Some(corrected.clone());
            stage = Stage::Output;
            if let Some(path) = &self.config.raw_output {
                self.report(stage);
                write_transcript(&corrected, path, self.config.force)?;
            }
            let formatted = if self.config.polish {
                stage = Stage::Polishing;
                self.report(stage);
                let dictionary_context = self.dictionary.formatter_context();
                let mut options = PolishOptions::new();
                if let Some(model) = &self.config.polish_model {
                    options = options.model(model);
                }
                if self.config.context != FormatContext::Auto {
                    options = options.context(self.config.context);
                }
                if let Some(context) = &dictionary_context {
                    options = options.dictionary_context(context);
                }
                match self.config.resolved_polish_engine() {
                    PolishEngine::Local => crate::polish_local_with_options(&corrected, &options)?,
                    // Keep client creation lazy: spoken Verbatim directives do not
                    // require a key even when OpenAI polishing is the default.
                    PolishEngine::Openai => match &client {
                        Some(client) => client.polish(&corrected, &options)?,
                        None => crate::polish_with_options(&corrected, &options)?,
                    },
                }
            } else {
                corrected
            };
            text = Some(formatted);
            stage = Stage::Output;
            self.report(stage);
            if let Some(path) = &self.config.output {
                write_transcript(text.as_deref().unwrap(), path, self.config.force)?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(Transcript {
                raw: raw.unwrap(),
                text: text.unwrap(),
            }),
            Err(source) => Err(WorkflowError {
                stage,
                raw,
                text,
                source,
            }),
        }
    }
}
