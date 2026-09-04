use std::fmt;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Engine {
    #[value(name = "gpt-transcribe", alias = "1")]
    GptTranscribe,
    #[value(alias = "2")]
    Codex,
    #[value(alias = "3")]
    Whisper,
}

impl fmt::Display for Engine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::GptTranscribe => "gpt-transcribe",
            Self::Codex => "codex",
            Self::Whisper => "whisper",
        })
    }
}

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Audio file to transcribe.
    #[arg(value_name = "AUDIO", conflicts_with = "record")]
    pub input: Option<PathBuf>,

    /// Record from the default microphone until Ctrl-C, then transcribe.
    #[arg(long, conflicts_with = "input")]
    pub record: bool,

    /// Keep a recording at this location instead of deleting it afterward.
    #[arg(long, value_name = "PATH", requires = "record")]
    pub save_recording: Option<PathBuf>,

    /// Transcription engine (names or aliases: 1, 2, 3).
    #[arg(long, value_enum, default_value_t = Engine::GptTranscribe)]
    pub engine: Engine,

    /// Model for the codex or whisper engine (whisper defaults to tiny.en).
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Write the transcript to a file instead of stdout.
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Allow overwriting output and saved-recording files.
    #[arg(long)]
    pub force: bool,
}

impl Cli {
    pub fn validate(&self) -> Result<()> {
        if !self.record && self.input.is_none() {
            bail!("provide an audio file or use --record");
        }
        if self.engine == Engine::GptTranscribe && self.model.is_some() {
            bail!("--model is only valid with --engine codex or --engine whisper");
        }
        if let (Some(output), Some(recording)) = (&self.output, &self.save_recording)
            && paths_refer_to_same_file(output, recording)
        {
            bail!("--output and --save-recording must refer to different files");
        }
        if let (Some(input), Some(output)) = (&self.input, &self.output)
            && paths_refer_to_same_file(input, output)
        {
            bail!("the transcript output must not overwrite the input audio file");
        }
        Ok(())
    }
}

fn paths_refer_to_same_file(left: &PathBuf, right: &PathBuf) -> bool {
    left == right
        || left
            .canonicalize()
            .and_then(|left| right.canonicalize().map(|right| left == right))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_gpt_transcribe() {
        let cli = Cli::try_parse_from(["hear", "message.mp3"]).unwrap();
        assert_eq!(cli.engine, Engine::GptTranscribe);
    }

    #[test]
    fn accepts_numeric_engine_aliases() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--engine", "3"]).unwrap();
        assert_eq!(cli.engine, Engine::Whisper);
    }

    #[test]
    fn rejects_model_for_gpt_transcribe() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--model", "anything"]).unwrap();
        assert!(cli.validate().is_err());
    }

    #[test]
    fn record_and_input_conflict() {
        assert!(Cli::try_parse_from(["hear", "message.wav", "--record"]).is_err());
    }

    #[test]
    fn output_cannot_overwrite_input() {
        let cli =
            Cli::try_parse_from(["hear", "message.wav", "--output", "message.wav", "--force"])
                .unwrap();
        assert!(cli.validate().is_err());
    }
}
