use std::process::Command;

use anyhow::{Context, Result};

pub fn deliver(transcript: &str, paste: bool) -> Result<bool> {
    arboard::Clipboard::new()
        .context("could not access the clipboard")?
        .set_text(transcript)
        .context("could not copy the transcript")?;

    if paste && post_paste() {
        Ok(true)
    } else {
        Ok(false)
    }
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
