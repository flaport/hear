mod audio;
mod cli;
mod dictionary_cli;

use std::io::{self, Write};

use clap::Parser;
use hear::{Stage, Transcript, Workflow, WorkflowError, WorkflowEvent, dictionary::Dictionary};
use hear_core::helper::{Recording, Response};

use crate::cli::{Cli, Command};

struct Completed {
    transcript: Transcript,
    recording: Option<Recording>,
}

fn main() {
    let cli = Cli::parse();
    let result =
        run(&cli).and_then(|completed| completed.map(|job| deliver(&cli, job)).transpose());
    if cli.json && !matches!(&result, Ok(Some(_))) {
        let response = match &result {
            Ok(Some(transcript)) => Response::Success {
                raw: transcript.raw.clone(),
                text: transcript.text.clone(),
            },
            Ok(None) => Response::Failure {
                phase: "recording".into(),
                message: "cancelled".into(),
                raw: None,
            },
            Err(error) => Response::Failure {
                phase: error.stage.as_str().into(),
                message: error.to_string(),
                raw: error.raw.clone(),
            },
        };
        println!(
            "{}",
            serde_json::to_string(&response).expect("serializable response")
        );
    }
    let exit = match result {
        Ok(Some(_)) => 0,
        Ok(None) => 130,
        Err(error) => {
            eprintln!("error: {error}");
            if !cli.json
                && let Some(raw) = &error.raw
            {
                eprintln!("Raw transcript retained below:\n{raw}");
            }
            1
        }
    };
    if exit != 0 {
        std::process::exit(exit);
    }
}

fn run(cli: &Cli) -> Result<Option<Completed>, WorkflowError> {
    let validation = |error| WorkflowError::new(Stage::Validation, error);
    cli.validate().map_err(validation)?;
    if let Some(Command::Dictionary { command }) = &cli.command {
        dictionary_cli::run(command).map_err(validation)?;
        return Ok(Some(Completed {
            transcript: Transcript {
                raw: String::new(),
                text: String::new(),
            },
            recording: None,
        }));
    }
    let config = cli.hear_config();
    let engine = config.resolved_engine();
    let workflow = Workflow::new(config)
        .dictionary(Dictionary::load().map_err(validation)?)
        .progress(move |event| match event {
            WorkflowEvent::Stage(Stage::Transcription) => {
                eprintln!("Transcribing with {engine}...")
            }
            WorkflowEvent::Stage(Stage::Polishing) => eprintln!("Polishing transcript..."),
            WorkflowEvent::Progress(hear::ProgressEvent::Uploading { part, total })
                if total > 1 =>
            {
                eprintln!("Uploading part {part} of {total}...");
            }
            WorkflowEvent::Progress(hear::ProgressEvent::Message(message)) => {
                eprintln!("{message}")
            }
            _ => {}
        });
    workflow.preflight(cli.input.as_deref())?;
    let mut recording = if cli.record {
        match audio::record().map_err(|error| WorkflowError::new(Stage::Recording, error))? {
            audio::RecordingOutcome::Completed(path) => Some(Recording::new(path)),
            audio::RecordingOutcome::Cancelled => return Ok(None),
        }
    } else {
        None
    };
    let input = recording
        .as_ref()
        .map(Recording::path)
        .or(cli.input.as_deref())
        .expect("validated audio input");
    match workflow.run(input) {
        Ok(transcript) => Ok(Some(Completed {
            transcript,
            recording,
        })),
        Err(error) => {
            if let (Some(recording), Some(raw)) = (&mut recording, &error.raw) {
                recording.remember_transcript(raw);
            }
            Err(error)
        }
    }
}

// Delivery belongs to the terminal adapter. Keep the recording until stdout
// accepts the result, including the JSON protocol used by subprocess callers.
fn deliver(cli: &Cli, mut completed: Completed) -> Result<Transcript, WorkflowError> {
    let transcript = completed.transcript;
    if let Some(recording) = &mut completed.recording {
        recording.remember_transcript(&transcript.raw);
    }
    let write = if cli.json {
        let response = Response::Success {
            raw: transcript.raw.clone(),
            text: transcript.text.clone(),
        };
        writeln!(
            io::stdout().lock(),
            "{}",
            serde_json::to_string(&response).expect("serializable response")
        )
    } else if cli.output.is_none() && cli.command.is_none() {
        writeln!(io::stdout().lock(), "{}", transcript.text.trim())
    } else {
        Ok(())
    };
    write.map_err(|source| {
        let mut error = WorkflowError::new(Stage::Output, source);
        error.raw = Some(transcript.raw.clone());
        error.text = Some(transcript.text.clone());
        error
    })?;
    if let Some(recording) = completed.recording {
        recording.delivered();
    }
    Ok(transcript)
}
