mod audio;
mod cli;
mod engines;
mod output;

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;
use tempfile::TempPath;

use crate::cli::{Cli, Engine};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    cli.validate()?;
    output::preflight(&cli)?;

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

        audio::record(&path)?;
        path
    } else {
        cli.input
            .clone()
            .expect("CLI validation guarantees an input path")
    };

    validate_input(&input)?;
    eprintln!("Transcribing with {}...", cli.engine);

    let transcript = match cli.engine {
        Engine::GptTranscribe => engines::openai::transcribe(&input)?,
        Engine::Codex => engines::codex::transcribe(&input, cli.model.as_deref())?,
        Engine::Whisper => {
            engines::whisper::transcribe(&input, cli.model.as_deref().unwrap_or("tiny.en"))?
        }
    };

    output::write_transcript(&transcript, cli.output.as_deref(), cli.force)?;
    drop(temporary_recording);
    Ok(())
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
