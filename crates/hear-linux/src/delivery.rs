use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use anyhow::{Context, Result, bail};

use crate::config::Config;

pub fn deliver(transcript: &str, paste: bool, config: &Config) -> Result<bool> {
    copy_to_clipboard(transcript)?;

    if paste && post_paste(config) {
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

fn post_paste(config: &Config) -> bool {
    if is_wayland() {
        Command::new("wtype")
            .args(["-M", "ctrl", "-P", "v", "-m", "ctrl", "-p", "v"])
            .status()
            .is_ok_and(|s| s.success())
    } else {
        let shortcut = config.paste_shortcut_for(active_x11_window_class().as_deref());
        Command::new("xdotool")
            .args(["key", "--clearmodifiers", shortcut])
            .status()
            .is_ok_and(|s| s.success())
    }
}

fn active_x11_window_class() -> Option<String> {
    let output = Command::new("xdotool")
        .args(["getwindowfocus", "getwindowclassname"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let class = String::from_utf8(output.stdout).ok()?;
    let class = class.trim();
    (!class.is_empty()).then(|| class.to_owned())
}

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
}
