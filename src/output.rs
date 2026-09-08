use anyhow::{Context, Result};
use std::{io::Write, path::Path};

/// Write a transcript followed by a newline, respecting overwrite policy.
pub fn write_transcript(transcript: &str, destination: &Path, force: bool) -> Result<()> {
    let mut file = hear_core::files::create(destination, force)?;
    writeln!(file, "{}", transcript.trim())
        .with_context(|| format!("could not write transcript: {}", destination.display()))
}
