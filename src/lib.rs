//! Reusable OpenAI transcription and transcript-polishing API.

mod context;
mod ffmpeg;
mod formatter;
#[path = "engines/openai.rs"]
mod openai;

use std::path::Path;

use anyhow::Result;

pub use context::FormatContext;

/// Both stages of a transcription, allowing callers to retain or display either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw: String,
    pub text: String,
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
    let raw = openai::transcribe(input, vocabulary)?;
    let text = if polish {
        formatter::polish(&raw, context, dictionary_context, None)?
    } else {
        raw.clone()
    };
    Ok(Transcript { raw, text })
}

/// Polish an existing transcript with OpenAI.
pub fn polish(
    transcript: &str,
    context: Option<FormatContext>,
    dictionary_context: Option<&str>,
) -> Result<String> {
    formatter::polish(transcript, context, dictionary_context, None)
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
    let raw = openai::transcribe(input, vocabulary)?;
    let text = formatter::polish(&raw, context, dictionary_context, Some(instruction))?;
    Ok(Transcript { raw, text })
}

/// Polish an existing transcript with an additional caller-supplied
/// formatting instruction.
pub fn polish_with_instruction(
    transcript: &str,
    context: Option<FormatContext>,
    dictionary_context: Option<&str>,
    instruction: &str,
) -> Result<String> {
    formatter::polish(transcript, context, dictionary_context, Some(instruction))
}
