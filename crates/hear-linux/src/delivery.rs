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
        let shortcut = paste_shortcut_for_class(active_x11_window_class().as_deref());
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

fn paste_shortcut_for_class(class: Option<&str>) -> &'static str {
    if class.is_some_and(|class| class.eq_ignore_ascii_case("Alacritty")) {
        "alt+v"
    } else {
        "ctrl+v"
    }
}

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::paste_shortcut_for_class;

    #[test]
    fn uses_alt_v_for_alacritty() {
        assert_eq!(paste_shortcut_for_class(Some("Alacritty")), "alt+v");
        assert_eq!(paste_shortcut_for_class(Some("alacritty")), "alt+v");
    }

    #[test]
    fn uses_ctrl_v_for_other_or_unknown_apps() {
        assert_eq!(paste_shortcut_for_class(Some("firefox")), "ctrl+v");
        assert_eq!(paste_shortcut_for_class(None), "ctrl+v");
    }
}
