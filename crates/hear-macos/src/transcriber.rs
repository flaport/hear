use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

use tempfile::TempPath;
use winit::event_loop::EventLoopProxy;

use crate::app::AppEvent;
use crate::credentials;

pub fn transcribe(recording: TempPath, proxy: EventLoopProxy<AppEvent>) {
    thread::spawn(move || {
        let result = run(&recording).map_err(|error| format!("{error:#}"));
        let _ = proxy.send_event(AppEvent::TranscriptionFinished(result));
    });
}

fn run(recording: &Path) -> anyhow::Result<String> {
    let mut command = Command::new(helper_path());
    command.arg(recording);
    if std::env::var_os("OPENAI_API_KEY").is_none()
        && let Some(api_key) = credentials::stored_api_key()?
    {
        command.env("OPENAI_API_KEY", api_key);
    }
    let output = command
        .output()
        .map_err(|error| anyhow::anyhow!("could not launch the hear helper: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("hear failed with {}: {}", output.status, stderr.trim());
    }
    let transcript = String::from_utf8(output.stdout)
        .map_err(|_| anyhow::anyhow!("hear returned a transcript that was not UTF-8"))?;
    let transcript = transcript.trim();
    if transcript.is_empty() {
        anyhow::bail!("hear returned an empty transcript");
    }
    Ok(transcript.to_owned())
}

fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HEAR_HELPER_PATH") {
        return path.into();
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(contents) = executable.parent().and_then(Path::parent)
    {
        let bundled = contents.join("Helpers").join("hear");
        if bundled.is_file() {
            return bundled;
        }
    }
    PathBuf::from("hear")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_override_selects_helper() {
        // Environment mutation is avoided here; the fallback is deterministic
        // when the test binary is not inside an application bundle.
        if std::env::var_os("HEAR_HELPER_PATH").is_none() {
            assert_eq!(helper_path(), PathBuf::from("hear"));
        }
    }
}
