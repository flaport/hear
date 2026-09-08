use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

pub(crate) fn require_ffmpeg(operation: &str) -> Result<()> {
    let available = Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !available {
        bail!("{operation} is not possible without FFmpeg; install ffmpeg and try again");
    }
    Ok(())
}

pub(crate) fn run_ffmpeg(
    input: &Path,
    arguments: &[&str],
    output: &Path,
    operation: &str,
) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(arguments)
        .arg(output)
        .output()
        .with_context(|| format!("could not launch FFmpeg to {operation}"))?;
    if !output.status.success() {
        bail!(
            "FFmpeg could not {operation}: {}: {}",
            input.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
