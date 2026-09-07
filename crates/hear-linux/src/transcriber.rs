use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use tempfile::TempPath;

use crate::app::AppEvent;
use crate::credentials;

pub fn transcribe_async(
    recording: TempPath,
    tx: mpsc::Sender<AppEvent>,
    hear_options: Vec<String>,
) {
    thread::spawn(move || {
        let result = run(&recording, &hear_options).map_err(|error| format!("{error:#}"));
        let _ = tx.send(AppEvent::TranscriptionFinished(result));
    });
}

pub(crate) fn run(recording: &Path, hear_options: &[String]) -> anyhow::Result<String> {
    let mut command = helper_command(recording, hear_options);
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

fn helper_command(recording: &Path, hear_options: &[String]) -> Command {
    let mut command = Command::new(helper_path());
    command.args(hear_options).arg(recording);
    command
}

fn helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HEAR_HELPER_PATH") {
        return path.into();
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let sibling = directory.join("hear");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("hear")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_override_selects_helper() {
        if std::env::var_os("HEAR_HELPER_PATH").is_none() {
            assert_eq!(helper_path(), PathBuf::from("hear"));
        }
    }

    #[test]
    fn configured_options_precede_the_recording_path() {
        let options = vec!["--engine".to_owned(), "whisper".to_owned()];
        let command = helper_command(Path::new("recording.wav"), &options);
        let arguments: Vec<_> = command.get_args().collect();

        assert_eq!(arguments, ["--engine", "whisper", "recording.wav"]);
    }
}
