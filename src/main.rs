mod audio;
mod cli;
mod dictionary;
mod engines;
mod formatter;
mod output;

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tempfile::TempPath;

use crate::cli::{Cli, Command, Engine};

enum RunOutcome {
    Completed,
    Cancelled,
}

fn main() {
    match run() {
        Ok(RunOutcome::Completed) => {}
        Ok(RunOutcome::Cancelled) => std::process::exit(130),
        Err(error) => {
            eprintln!("error: {error:#}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<RunOutcome> {
    let cli = Cli::parse();
    cli.validate()?;
    if let Some(Command::Dictionary { command }) = &cli.command {
        dictionary::run(command)?;
        return Ok(RunOutcome::Completed);
    }
    output::preflight(&cli)?;

    let dictionary = dictionary::Dictionary::load()?;
    let vocabulary = dictionary.canonical_terms();

    let mut temporary_recording: Option<TempPath> = None;
    let input = if cli.record {
        let path = match &cli.save_recording {
            Some(path) => path.clone(),
            None => {
                let tempfile = tempfile::Builder::new()
                    .prefix("hear-recording-")
                    .suffix(".wav")
                    .tempfile()
                    .context("could not create a temporary recording file")?;
                let path = tempfile.path().to_path_buf();
                temporary_recording = Some(tempfile.into_temp_path());
                path
            }
        };

        if audio::record(&path)? == audio::RecordingOutcome::Cancelled {
            return Ok(RunOutcome::Cancelled);
        }
        path
    } else {
        cli.input
            .clone()
            .expect("CLI validation guarantees an input path")
    };

    validate_input(&input)?;
    eprintln!("Transcribing with {}...", cli.engine);

    let raw_transcript = match cli.engine {
        Engine::GptTranscribe => engines::openai::transcribe(&input, &vocabulary)?,
        Engine::Codex => engines::codex::transcribe(&input, cli.model.as_deref(), &vocabulary)?,
        Engine::Whisper => engines::whisper::transcribe(
            &input,
            cli.model.as_deref().unwrap_or("tiny.en"),
            &vocabulary,
        )?,
    };
    let raw_transcript = dictionary.correct_aliases(&raw_transcript)?;

    if let Some(path) = cli.raw_output.as_deref() {
        output::write_transcript(&raw_transcript, Some(path), cli.force)?;
    }
    let transcript = if cli.should_polish() {
        eprintln!("Polishing transcript...");
        formatter::polish(&raw_transcript, cli.format_context(), &dictionary)?
    } else {
        raw_transcript
    };
    output::write_transcript(&transcript, cli.output.as_deref(), cli.force)?;
    drop(temporary_recording);
    Ok(RunOutcome::Completed)
}

fn validate_input(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("audio file does not exist: {}", path.display());
    }
    if !path.is_file() {
        bail!("audio input is not a file: {}", path.display());
    }
    Ok(())
}
