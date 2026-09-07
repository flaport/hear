use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use anyhow::{Context, Result, bail};

pub fn deliver(transcript: &str, paste: bool) -> Result<bool> {
    copy_to_clipboard(transcript)?;

    if paste && post_paste() {
        Ok(true)
    } else {
        Ok(false)
    }
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let (program, arguments): (&str, &[&str]) = if is_wayland() {
        ("wl-copy", &[])
    } else {
        ("xclip", &["-selection", "clipboard"])
    };
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not launch {program} to copy the transcript"))?;
    child
        .stdin
        .take()
        .context("clipboard process did not accept input")?
        .write_all(text.as_bytes())
        .context("could not send the transcript to the clipboard process")?;
    let status = child
        .wait()
        .context("could not wait for the clipboard process")?;
    if !status.success() {
        bail!("{program} failed with {status}");
    }
    Ok(())
}

fn post_paste() -> bool {
    if is_wayland() {
        Command::new("wtype")
            .args(["-M", "ctrl", "-P", "v", "-m", "ctrl", "-p", "v"])
            .status()
            .is_ok_and(|s| s.success())
    } else {
        Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+v"])
            .status()
            .is_ok_and(|s| s.success())
    }
}

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
}
