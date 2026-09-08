use std::{fmt, path::PathBuf};

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use hear::FormatContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Engine {
    #[value(name = "gpt-transcribe", alias = "1")]
    GptTranscribe,
    #[value(alias = "2")]
    Codex,
    #[value(alias = "3")]
    Whisper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PolishEngine {
    Openai,
    Local,
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

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage words and names that should be transcribed consistently.
    Dictionary {
        #[command(subcommand)]
        command: DictionaryCommand,
    },
}

#[derive(Debug, Subcommand)]
pub enum DictionaryCommand {
    /// Add a term or update its aliases and pronunciation.
    Add {
        /// Canonical spelling to use in transcripts.
        term: String,

        /// Alternate or commonly mistranscribed form; may be repeated.
        #[arg(long = "alias", value_name = "TEXT")]
        aliases: Vec<String>,

        /// A short pronunciation hint.
        #[arg(long, value_name = "TEXT")]
        sounds_like: Option<String>,
    },

    /// List saved dictionary entries.
    List,

    /// Remove an entry by its canonical spelling.
    Remove {
        /// Canonical spelling to remove.
        term: String,
    },
}

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Audio file to transcribe.
    #[arg(value_name = "AUDIO", conflicts_with = "record")]
    pub input: Option<PathBuf>,

    /// Record until Return; Ctrl-C cancels.
    #[arg(long, conflicts_with = "input")]
    pub record: bool,

    /// Keep a recording at this location instead of deleting it afterward.
    #[arg(long, value_name = "PATH", requires = "record")]
    pub save_recording: Option<PathBuf>,

    /// Engine; inferred from --model or --language, otherwise gpt-transcribe.
    #[arg(long, value_enum)]
    pub engine: Option<Engine>,

    /// Model for the selected transcription engine.
    ///
    /// Whisper models: tiny.en (default), base.en, small.en, medium.en, and
    /// large-v3-turbo. Codex accepts a model supported by `codex exec`.
    #[arg(long, value_name = "MODEL")]
    pub model: Option<String>,

    /// Polishing engine; inferred from --polish-model, otherwise OpenAI.
    #[arg(long, value_enum)]
    pub polish_engine: Option<PolishEngine>,

    /// Model for the selected polishing engine.
    ///
    /// OpenAI defaults to gpt-5.6-luna and accepts another OpenAI model ID.
    /// Local polishing supports qwen3.5-2b (the Q4_K_M default) and
    /// qwen3.5-0.8b. Their explicit aliases are qwen3.5-2b-q4_k_m and
    /// qwen3.5-0.8b-q4_k_m. A compatible GGUF file path is also accepted.
    #[arg(long, value_name = "MODEL_OR_GGUF_PATH")]
    pub polish_model: Option<String>,

    /// Spoken language for Whisper, or auto (defaults to en).
    #[arg(long, value_name = "LANGUAGE")]
    pub language: Option<String>,

    /// Write the transcript to a file instead of stdout.
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Explicitly enable automatic formatting (retained for compatibility).
    #[arg(long, conflicts_with = "no_polish", hide = true)]
    pub polish: bool,

    /// Formatting context instead of automatic inference.
    #[arg(long, value_enum, value_name = "CONTEXT", conflicts_with = "no_polish")]
    pub context: Option<FormatContext>,

    /// Skip LLM formatting and return the transcript directly.
    #[arg(long)]
    pub no_polish: bool,

    /// Save the unformatted transcript when polishing.
    #[arg(long, value_name = "PATH")]
    pub raw_output: Option<PathBuf>,

    /// Allow overwriting transcript and saved-recording files.
    #[arg(long)]
    pub force: bool,
}

impl Cli {
    pub fn validate(&self) -> Result<()> {
        if matches!(self.command, Some(Command::Dictionary { .. })) {
            if self.input.is_some()
                || self.record
                || self.save_recording.is_some()
                || self.engine.is_some()
                || self.model.is_some()
                || self.polish_engine.is_some()
                || self.polish_model.is_some()
                || self.language.is_some()
                || self.output.is_some()
                || self.polish
                || self.context.is_some()
                || self.no_polish
                || self.raw_output.is_some()
                || self.force
            {
                bail!("dictionary commands cannot be combined with transcription options");
            }
            return Ok(());
        }
        if !self.record && self.input.is_none() {
            bail!("provide an audio file or use --record");
        }
        if self.engine == Some(Engine::GptTranscribe) && self.model.is_some() {
            bail!("--model is only valid with --engine codex or --engine whisper");
        }
        if self.language.is_some() && self.engine.is_some_and(|engine| engine != Engine::Whisper) {
            bail!("--language is only valid with --engine whisper");
        }
        if self.raw_output.is_some() && !self.should_polish() {
            bail!("--raw-output cannot be used with --no-polish");
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
        let paths = [
            (self.input.as_ref(), "audio input"),
            (self.output.as_ref(), "transcript output"),
            (self.raw_output.as_ref(), "raw transcript output"),
            (self.save_recording.as_ref(), "saved recording"),
        ];
        for (index, (left, left_name)) in paths.iter().enumerate() {
            for (right, right_name) in paths.iter().skip(index + 1) {
                if let (Some(left), Some(right)) = (left, right)
                    && paths_refer_to_same_file(left, right)
                {
                    bail!("{left_name} and {right_name} must refer to different files");
                }
            }
        }
        Ok(())
    }

    pub fn should_polish(&self) -> bool {
        !self.no_polish
    }

    pub fn format_context(&self) -> Option<FormatContext> {
        self.context
            .filter(|context| *context != FormatContext::Auto)
    }

    pub fn resolved_engine(&self) -> Engine {
        self.engine.unwrap_or_else(|| {
            if self.language.is_some() || self.model.as_deref().is_some_and(is_whisper_model) {
                Engine::Whisper
            } else if self.model.is_some() {
                Engine::Codex
            } else {
                Engine::GptTranscribe
            }
        })
    }

    pub fn resolved_polish_engine(&self) -> PolishEngine {
        self.polish_engine.unwrap_or_else(|| {
            if self
                .polish_model
                .as_deref()
                .is_some_and(is_local_polish_model)
            {
                PolishEngine::Local
            } else {
                PolishEngine::Openai
            }
        })
    }
}

fn is_whisper_model(model: &str) -> bool {
    matches!(
        model,
        "tiny.en" | "base.en" | "small.en" | "medium.en" | "large-v3-turbo"
    )
}

fn is_local_polish_model(model: &str) -> bool {
    matches!(
        model,
        "qwen3.5-2b" | "qwen3.5-2b-q4_k_m" | "qwen3.5-0.8b" | "qwen3.5-0.8b-q4_k_m"
    ) || PathBuf::from(model)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gguf"))
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
        assert_eq!(cli.engine, None);
        assert_eq!(cli.resolved_engine(), Engine::GptTranscribe);
        assert_eq!(cli.polish_engine, None);
        assert_eq!(cli.resolved_polish_engine(), PolishEngine::Openai);
        assert_eq!(cli.polish_model, None);
        assert!(cli.should_polish());
    }

    #[test]
    fn accepts_local_polishing() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--polish-engine", "local"]).unwrap();
        assert_eq!(cli.polish_engine, Some(PolishEngine::Local));
        assert_eq!(cli.polish_model, None);
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn long_help_lists_transcription_and_polishing_models() {
        use clap::CommandFactory;

        let help = Cli::command().render_long_help().to_string();
        for model in [
            "tiny.en",
            "base.en",
            "small.en",
            "medium.en",
            "large-v3-turbo",
            "gpt-5.6-luna",
            "qwen3.5-2b",
            "qwen3.5-2b-q4_k_m",
            "qwen3.5-0.8b",
            "qwen3.5-0.8b-q4_k_m",
        ] {
            assert!(help.contains(model), "long help omitted {model}");
        }
        assert!(help.contains("compatible GGUF file"));
    }

    #[test]
    fn accepts_numeric_engine_aliases() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--engine", "3"]).unwrap();
        assert_eq!(cli.engine, Some(Engine::Whisper));
    }

    #[test]
    fn rejects_model_for_gpt_transcribe() {
        let cli = Cli::try_parse_from([
            "hear",
            "message.wav",
            "--engine",
            "gpt-transcribe",
            "--model",
            "anything",
        ])
        .unwrap();
        assert!(cli.validate().is_err());
    }

    #[test]
    fn infers_engines_from_models_and_language() {
        let whisper = Cli::try_parse_from(["hear", "message.wav", "--model", "small.en"]).unwrap();
        assert_eq!(whisper.resolved_engine(), Engine::Whisper);

        let multilingual =
            Cli::try_parse_from(["hear", "message.wav", "--language", "nl"]).unwrap();
        assert_eq!(multilingual.resolved_engine(), Engine::Whisper);

        let codex = Cli::try_parse_from(["hear", "message.wav", "--model", "gpt-5.4"]).unwrap();
        assert_eq!(codex.resolved_engine(), Engine::Codex);

        let local =
            Cli::try_parse_from(["hear", "message.wav", "--polish-model", "qwen3.5-0.8b"]).unwrap();
        assert_eq!(local.resolved_polish_engine(), PolishEngine::Local);

        let gguf = Cli::try_parse_from([
            "hear",
            "message.wav",
            "--polish-model",
            "/models/custom.GGUF",
        ])
        .unwrap();
        assert_eq!(gguf.resolved_polish_engine(), PolishEngine::Local);
    }

    #[test]
    fn explicit_engines_override_model_inference() {
        let cli = Cli::try_parse_from([
            "hear",
            "message.wav",
            "--engine",
            "codex",
            "--model",
            "tiny.en",
            "--polish-engine",
            "openai",
            "--polish-model",
            "qwen3.5-0.8b",
        ])
        .unwrap();
        assert_eq!(cli.resolved_engine(), Engine::Codex);
        assert_eq!(cli.resolved_polish_engine(), PolishEngine::Openai);
    }

    #[test]
    fn language_is_only_valid_for_whisper() {
        let openai = Cli::try_parse_from([
            "hear",
            "message.wav",
            "--engine",
            "gpt-transcribe",
            "--language",
            "de",
        ])
        .unwrap();
        assert!(openai.validate().is_err());

        let whisper = Cli::try_parse_from(["hear", "message.wav", "--language", "de"]).unwrap();
        assert!(whisper.validate().is_ok());
        assert_eq!(whisper.resolved_engine(), Engine::Whisper);
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

    #[test]
    fn context_enables_polishing() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--context", "email"]).unwrap();
        assert!(cli.should_polish());
        assert_eq!(cli.context, Some(FormatContext::Email));
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn accepts_context_aliases() {
        let text = Cli::try_parse_from(["hear", "message.wav", "--context", "text"]).unwrap();
        let tasks = Cli::try_parse_from(["hear", "message.wav", "--context", "tasks"]).unwrap();
        assert_eq!(text.context, Some(FormatContext::Message));
        assert_eq!(tasks.context, Some(FormatContext::Todo));
    }

    #[test]
    fn raw_output_requires_polishing() {
        let cli = Cli::try_parse_from([
            "hear",
            "message.wav",
            "--no-polish",
            "--raw-output",
            "raw.txt",
        ])
        .unwrap();
        assert!(cli.validate().is_err());
    }

    #[test]
    fn no_polish_disables_default_polishing() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--no-polish"]).unwrap();
        assert!(!cli.should_polish());
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn legacy_polish_flag_is_still_accepted() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--polish"]).unwrap();
        assert!(cli.should_polish());
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn explicit_auto_context_uses_automatic_directive_detection() {
        let cli = Cli::try_parse_from(["hear", "message.wav", "--context", "auto"]).unwrap();
        assert_eq!(cli.format_context(), None);
    }

    #[test]
    fn no_polish_conflicts_with_context() {
        assert!(
            Cli::try_parse_from(["hear", "message.wav", "--no-polish", "--context", "email",])
                .is_err()
        );
    }

    #[test]
    fn raw_and_formatted_outputs_must_differ() {
        let cli = Cli::try_parse_from([
            "hear",
            "message.wav",
            "--polish",
            "--output",
            "result.txt",
            "--raw-output",
            "result.txt",
        ])
        .unwrap();
        assert!(cli.validate().is_err());
    }

    #[test]
    fn parses_dictionary_add_command() {
        let cli = Cli::try_parse_from([
            "hear",
            "dictionary",
            "add",
            "Flaport",
            "--alias",
            "flap port",
            "--sounds-like",
            "flah-port",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Dictionary {
                command: DictionaryCommand::Add { .. }
            })
        ));
        assert!(cli.validate().is_ok());
    }

    #[test]
    fn dictionary_command_rejects_transcription_options() {
        let cli = Cli::try_parse_from(["hear", "--no-polish", "dictionary", "list"]).unwrap();
        assert!(cli.validate().is_err());
    }
}
