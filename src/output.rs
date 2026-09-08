use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::Cli;

pub fn preflight(cli: &Cli) -> Result<()> {
    if let Some(path) = &cli.output {
        ensure_writable_destination(path, cli.force, "transcript")?;
    }
    if let Some(path) = &cli.save_recording {
        ensure_writable_destination(path, cli.force, "recording")?;
    }
    if let Some(path) = &cli.raw_output {
        ensure_writable_destination(path, cli.force, "raw transcript")?;
    }
    Ok(())
}

fn ensure_writable_destination(path: &Path, force: bool, _description: &str) -> Result<()> {
    hear_core::files::preflight(path, force)
}

pub fn write_transcript(transcript: &str, destination: Option<&Path>, force: bool) -> Result<()> {
    let transcript = transcript.trim();
    match destination {
        Some(path) => {
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(force);
            if !force {
                options.create_new(true);
            }
            let mut file = options
                .open(path)
                .with_context(|| format!("could not create transcript file: {}", path.display()))?;
            writeln!(file, "{transcript}")
                .with_context(|| format!("could not write transcript: {}", path.display()))?;
        }
        None => {
            let stdout = io::stdout();
            let mut stdout = stdout.lock();
            writeln!(stdout, "{transcript}").context("could not write transcript to stdout")?;
        }
    }
    Ok(())
}
