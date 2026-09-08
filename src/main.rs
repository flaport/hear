mod audio;
mod cli;
mod dictionary;
mod engines;
mod ffmpeg;
mod output;

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Parser;
use hear_core::helper::{Recording, Response};

use crate::cli::{Cli, Command, Engine, PolishEngine};

enum RunOutcome {
    Completed,
    Cancelled,
}

#[derive(Default)]
struct Progress {
    phase: &'static str,
    raw: Option<String>,
    text: Option<String>,
    recording: Option<Recording>,
}
fn main() {
    let cli = Cli::parse();
    let mut progress = Progress {
        phase: "validation",
        ..Progress::default()
    };
    let result = run(&cli, &mut progress);
    if cli.json {
        let response = match &result {
            Ok(RunOutcome::Completed) => Response::Success {
                raw: progress.raw.clone().unwrap_or_default(),
                text: progress.text.clone().unwrap_or_default(),
            },
            Ok(RunOutcome::Cancelled) => Response::Failure {
                phase: "recording".into(),
                message: "cancelled".into(),
                raw: None,
            },
            Err(error) => Response::Failure {
                phase: progress.phase.into(),
                message: format!("{error:#}"),
                raw: progress.raw.clone(),
            },
        };
        println!(
            "{}",
            serde_json::to_string(&response).expect("serializable response")
        );
    }
    let exit = match result {
        Ok(RunOutcome::Completed) => 0,
        Ok(RunOutcome::Cancelled) => 130,
        Err(error) => {
            eprintln!("error: {error:#}");
            if !cli.json
                && let Some(raw) = &progress.raw
            {
                eprintln!("Raw transcript retained below:\n{raw}");
            }
            1
        }
    };
    if (exit == 0 || exit == 130)
        && let Some(recording) = progress.recording.take()
    {
        recording.delivered();
    }
    if exit == 1
        && let (Some(recording), Some(raw)) = (&mut progress.recording, &progress.raw)
    {
        recording.remember_transcript(raw);
    }
    // Drop before process::exit so failed recordings are preserved and reported.
    drop(progress);
    if exit != 0 {
        std::process::exit(exit);
    }
}
fn run(cli: &Cli, progress: &mut Progress) -> Result<RunOutcome> {
    cli.validate()?;
    if let Some(Command::Dictionary { command }) = &cli.command {
        dictionary::run(command)?;
        return Ok(RunOutcome::Completed);
    }
    output::preflight(cli)?;

    let dictionary = dictionary::Dictionary::load()?;
    let vocabulary = dictionary.canonical_terms();

    progress.phase = "recording";
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
                progress.recording = Some(Recording::new(tempfile.into_temp_path()));
                path
            }
        };

        if audio::record(&path, cli.force || progress.recording.is_some())?
            == audio::RecordingOutcome::Cancelled
        {
            return Ok(RunOutcome::Cancelled);
        }
        path
    } else {
        cli.input
            .clone()
            .expect("CLI validation guarantees an input path")
    };

    validate_input(&input)?;
    progress.phase = "transcription";
    let engine = cli.resolved_engine();
    eprintln!("Transcribing with {engine}...");

    let raw_transcript = match engine {
        Engine::GptTranscribe => hear::OpenAiClient::builder(
            std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?,
        )
        .progress(|event| match event {
            hear::ProgressEvent::Uploading { part, total } if total > 1 => {
                eprintln!("Uploading part {part} of {total}...")
            }
            hear::ProgressEvent::Message(message) => eprintln!("{message}"),
            _ => {}
        })
        .build()?
        .transcribe_raw(&input, &vocabulary)?,
        Engine::Codex => engines::codex::transcribe(&input, cli.model.as_deref(), &vocabulary)?,
        Engine::Whisper => engines::whisper::transcribe(
            &input,
            cli.model.as_deref().unwrap_or("tiny.en"),
            cli.language.as_deref().unwrap_or("en"),
            &vocabulary,
        )?,
    };
    let raw_transcript = dictionary.correct_aliases(&raw_transcript)?;

    progress.raw = Some(raw_transcript.clone());
    progress.phase = "output";
    if let Some(path) = cli.raw_output.as_deref() {
        output::write_transcript(&raw_transcript, Some(path), cli.force)?;
    }
    let transcript = if cli.should_polish() {
        progress.phase = "polishing";
        eprintln!("Polishing transcript...");
        let dictionary_context = dictionary.formatter_context();
        let mut options = hear::PolishOptions::new();
        if let Some(model) = cli.polish_model.as_deref() {
            options = options.model(model);
        }
        if let Some(context) = cli.format_context() {
            options = options.context(context);
        }
        if let Some(dictionary_context) = dictionary_context.as_deref() {
            options = options.dictionary_context(dictionary_context);
        }
        match cli.resolved_polish_engine() {
            PolishEngine::Openai => hear::polish_with_options(&raw_transcript, &options)?,
            PolishEngine::Local => hear::polish_local_with_options(&raw_transcript, &options)?,
        }
    } else {
        raw_transcript
    };
    progress.phase = "output";
    if !cli.json || cli.output.is_some() {
        output::write_transcript(&transcript, cli.output.as_deref(), cli.force)?;
    }
    progress.text = Some(transcript);
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
