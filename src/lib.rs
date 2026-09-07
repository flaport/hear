//! Reusable OpenAI transcription and transcript-polishing API.

mod context;
mod ffmpeg;
mod formatter;
mod openai;
mod openai_transport;

use std::path::Path;

use anyhow::Result;

pub use context::FormatContext;

/// Both stages of a transcription, allowing callers to retain or display either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw: String,
    pub text: String,
}

/// Named options for transcript polishing.
///
/// Use the builder methods so new optional settings can be added compatibly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PolishOptions<'a> {
    model: Option<&'a str>,
    context: Option<FormatContext>,
    dictionary_context: Option<&'a str>,
    instruction: Option<&'a str>,
}

impl<'a> PolishOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Select the model used to format the transcript.
    pub fn model(mut self, model: &'a str) -> Self {
        self.model = Some(model);
        self
    }

    pub fn context(mut self, context: FormatContext) -> Self {
        self.context = Some(context);
        self
    }

    pub fn dictionary_context(mut self, dictionary_context: &'a str) -> Self {
        self.dictionary_context = Some(dictionary_context);
        self
    }

    pub fn instruction(mut self, instruction: &'a str) -> Self {
        self.instruction = Some(instruction);
        self
    }
}

/// Named options for OpenAI transcription and optional polishing.
///
/// Transcription is raw by default. Call [`Self::polish`] to enable the
/// formatter after transcription.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TranscriptionOptions<'a> {
    vocabulary: &'a [String],
    polishing: Option<PolishOptions<'a>>,
}

impl<'a> TranscriptionOptions<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vocabulary(mut self, vocabulary: &'a [String]) -> Self {
        self.vocabulary = vocabulary;
        self
    }

    pub fn polish(mut self, options: PolishOptions<'a>) -> Self {
        self.polishing = Some(options);
        self
    }
}

/// Transcribe audio with OpenAI using named options.
///
/// ```no_run
/// let vocabulary = vec!["Qdrant".to_owned()];
/// let options = hear::TranscriptionOptions::new()
///     .vocabulary(&vocabulary)
///     .polish(
///         hear::PolishOptions::new()
///             .context(hear::FormatContext::Notes)
///             .instruction("Use short headings."),
///     );
/// let transcript = hear::transcribe_openai_with_options(
///     std::path::Path::new("meeting.m4a"),
///     &options,
/// )?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn transcribe_openai_with_options(
    input: &Path,
    options: &TranscriptionOptions<'_>,
) -> Result<Transcript> {
    let raw = transcribe_openai_raw(input, options.vocabulary)?;
    let text = match options.polishing {
        Some(options) => polish_with_options(&raw, &options)?,
        None => raw.clone(),
    };
    Ok(Transcript { raw, text })
}

/// Transcribe audio with OpenAI without allocating a duplicate polished field.
pub fn transcribe_openai_raw(input: &Path, vocabulary: &[String]) -> Result<String> {
    openai::transcribe(input, vocabulary)
}

/// Polish an existing transcript using named options.
pub fn polish_with_options(transcript: &str, options: &PolishOptions<'_>) -> Result<String> {
    formatter::polish(
        transcript,
        options.model,
        options.context,
        options.dictionary_context,
        options.instruction,
    )
}

/// Polish an existing transcript locally with the `hear-local-polish` helper.
///
/// The recommended Qwen model is downloaded and cached on first use unless a
/// model name or GGUF path was supplied through [`PolishOptions::model`].
#[cfg(feature = "local-polish")]
pub fn polish_local_with_options(transcript: &str, options: &PolishOptions<'_>) -> Result<String> {
    formatter::polish_local(
        transcript,
        options.model,
        options.context,
        options.dictionary_context,
        options.instruction,
    )
}

/// Transcribe an audio file with OpenAI and optionally polish the result.
///
/// The API key is read from `OPENAI_API_KEY`. `vocabulary` supplies preferred
/// spellings to transcription. `dictionary_context` can describe canonical
/// spellings and aliases for the polishing stage.
pub fn transcribe_openai(
    input: &Path,
    vocabulary: &[String],
    polish: bool,
    context: Option<FormatContext>,
    dictionary_context: Option<&str>,
) -> Result<Transcript> {
    let mut options = TranscriptionOptions::new().vocabulary(vocabulary);
    if polish {
        let mut polishing = PolishOptions::new();
        if let Some(context) = context {
            polishing = polishing.context(context);
        }
        if let Some(dictionary_context) = dictionary_context {
            polishing = polishing.dictionary_context(dictionary_context);
        }
        options = options.polish(polishing);
    }
    transcribe_openai_with_options(input, &options)
}

/// Polish an existing transcript with OpenAI.
pub fn polish(
    transcript: &str,
    context: Option<FormatContext>,
    dictionary_context: Option<&str>,
) -> Result<String> {
    let mut options = PolishOptions::new();
    if let Some(context) = context {
        options = options.context(context);
    }
    if let Some(dictionary_context) = dictionary_context {
        options = options.dictionary_context(dictionary_context);
    }
    polish_with_options(transcript, &options)
}

/// Transcribe an audio file with OpenAI and polish it with an additional
/// caller-supplied formatting instruction.
pub fn transcribe_openai_with_instruction(
    input: &Path,
    vocabulary: &[String],
    context: Option<FormatContext>,
    dictionary_context: Option<&str>,
    instruction: &str,
) -> Result<Transcript> {
    let mut polishing = PolishOptions::new().instruction(instruction);
    if let Some(context) = context {
        polishing = polishing.context(context);
    }
    if let Some(dictionary_context) = dictionary_context {
        polishing = polishing.dictionary_context(dictionary_context);
    }
    let options = TranscriptionOptions::new()
        .vocabulary(vocabulary)
        .polish(polishing);
    transcribe_openai_with_options(input, &options)
}

/// Polish an existing transcript with an additional caller-supplied
/// formatting instruction.
pub fn polish_with_instruction(
    transcript: &str,
    context: Option<FormatContext>,
    dictionary_context: Option<&str>,
    instruction: &str,
) -> Result<String> {
    let mut options = PolishOptions::new().instruction(instruction);
    if let Some(context) = context {
        options = options.context(context);
    }
    if let Some(dictionary_context) = dictionary_context {
        options = options.dictionary_context(dictionary_context);
    }
    polish_with_options(transcript, &options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_named_transcription_options() {
        let vocabulary = vec!["Qdrant".to_owned()];
        let polishing = PolishOptions::new()
            .context(FormatContext::Notes)
            .dictionary_context("- Qdrant; aliases: quadrant")
            .instruction("Use short headings.");
        let options = TranscriptionOptions::new()
            .vocabulary(&vocabulary)
            .polish(polishing);

        assert_eq!(options.vocabulary, vocabulary);
        assert_eq!(options.polishing, Some(polishing));
    }

    #[test]
    fn named_polish_options_support_verbatim_without_network() {
        let options = PolishOptions::new()
            .context(FormatContext::Verbatim)
            .instruction("This is intentionally ignored in verbatim mode.");
        assert_eq!(
            polish_with_options("Keep this exactly.", &options).unwrap(),
            "Keep this exactly."
        );
    }
}
