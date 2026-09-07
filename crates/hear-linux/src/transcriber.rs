use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;

use tempfile::TempPath;

use crate::app::AppEvent;
use crate::config::HearConfig;
use crate::credentials;

pub fn transcribe_async(recording: TempPath, tx: mpsc::Sender<AppEvent>, hear: HearConfig) {
    thread::spawn(move || {
        let result = run(&recording, &hear).map_err(|error| format!("{error:#}"));
        let _ = tx.send(AppEvent::TranscriptionFinished(result));
    });
}

pub(crate) fn run(recording: &Path, hear: &HearConfig) -> anyhow::Result<String> {
    save_recording(recording, hear)?;
    let mut command = helper_command(recording, hear);
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
    let transcript = match hear.output_path() {
        Some(path) => fs::read_to_string(path)
            .map_err(|error| anyhow::anyhow!("could not read transcript output: {error}"))?,
        None => String::from_utf8(output.stdout)
            .map_err(|_| anyhow::anyhow!("hear returned a transcript that was not UTF-8"))?,
    };
    let transcript = transcript.trim();
    if transcript.is_empty() {
        anyhow::bail!("hear returned an empty transcript");
    }
    Ok(transcript.to_owned())
}

fn helper_command(recording: &Path, hear: &HearConfig) -> Command {
    let mut command = Command::new(helper_path());
    command.args(hear.arguments()).arg(recording);
    command
}

fn save_recording(recording: &Path, hear: &HearConfig) -> anyhow::Result<()> {
    let Some(destination) = hear.save_recording_path() else {
        return Ok(());
    };
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(hear.force);
    if !hear.force {
        options.create_new(true);
    }
    let mut source = fs::File::open(recording)?;
    let mut destination_file = options.open(destination).map_err(|error| {
        anyhow::anyhow!(
            "could not save recording to {}: {error}",
            destination.display()
        )
    })?;
    io::copy(&mut source, &mut destination_file)?;
    Ok(())
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
        let hear = HearConfig {
            engine: "whisper".to_owned(),
            ..HearConfig::default()
        };
        let command = helper_command(Path::new("recording.wav"), &hear);
        let arguments: Vec<_> = command.get_args().collect();

        assert_eq!(
            arguments,
            [
                "--engine",
                "whisper",
                "--polish-engine",
                "openai",
                "--context",
                "auto",
                "recording.wav"
            ]
        );
    }
}
