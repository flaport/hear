use crate::{
    HearConfig,
    process::{self, Cancellation},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tempfile::TempPath;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Success {
        raw: String,
        text: String,
    },
    Failure {
        phase: String,
        message: String,
        raw: Option<String>,
    },
}
/// Owns a recording until delivery succeeds. Failed jobs keep a recoverable WAV.
#[derive(Debug)]
pub struct Recording {
    path: Option<TempPath>,
    transcript: Option<String>,
}
impl Recording {
    pub fn new(path: TempPath) -> Self {
        Self {
            path: Some(path),
            transcript: None,
        }
    }
    pub fn path(&self) -> &Path {
        self.path.as_deref().expect("recording is live")
    }
    pub fn remember_transcript(&mut self, text: &str) {
        self.transcript = Some(text.to_owned());
    }
    pub fn delivered(mut self) {
        self.path.take();
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            if let Some(text) = &self.transcript {
                use std::io::Write;
                let destination = path.with_extension("txt");
                if let Ok(mut file) = crate::files::create(&destination, false)
                    && file.write_all(text.as_bytes()).is_ok()
                {
                    eprintln!("Transcript retained for retry: {}", destination.display());
                }
            }

            match path.keep() {
                Ok(path) => eprintln!("Recording retained for retry: {}", path.display()),
                Err(e) => eprintln!("Could not retain recording: {e}"),
            }
        }
    }
}
pub fn transcribe(
    recording: &Recording,
    hear: &HearConfig,
    helper: PathBuf,
    key: impl FnOnce() -> Result<Option<String>>,
    cancellation: &Cancellation,
) -> Result<String> {
    hear.validate()?;
    if let Some(destination) = hear.save_recording_path() {
        crate::files::copy(recording.path(), destination, hear.force)?;
    }
    let mut command = Command::new(helper);
    command
        .args(hear.arguments())
        .arg("--json")
        .arg(recording.path());
    if hear.requires_openai()
        && std::env::var_os("OPENAI_API_KEY").is_none()
        && let Some(key) = key()?
    {
        command.env("OPENAI_API_KEY", key);
    }
    let output = process::run(
        &mut command,
        None,
        cancellation,
        Duration::from_secs(3600),
        false,
    )?;
    let response: Response = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "hear returned an invalid helper response: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    match response {
        Response::Success { text, .. } if output.status.success() && !text.trim().is_empty() => {
            Ok(text)
        }
        Response::Failure {
            phase,
            message,
            raw,
        } => {
            if let Some(raw) = raw {
                let path = recording.path().with_extension("raw.txt");
                if let Ok(mut file) = crate::files::create(&path, false) {
                    use std::io::Write;
                    if file.write_all(raw.as_bytes()).is_ok() {
                        eprintln!("Raw transcript retained: {}", path.display());
                    }
                }
            }
            bail!(
                "{phase} failed: {message}; recording retained at {}",
                recording.path().display()
            )
        }
        _ => bail!(
            "hear returned an empty or unsuccessful response; recording retained at {}",
            recording.path().display()
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn helper_failure_preserves_raw_result_and_audio() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("helper");
        std::fs::write(&script, "#!/bin/sh\nprintf '%s' '{\"status\":\"failure\",\"phase\":\"polishing\",\"message\":\"quota\",\"raw\":\"hello\"}'\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = tempfile::NamedTempFile::new_in(directory.path())
            .unwrap()
            .into_temp_path();
        let recording = Recording::new(path);
        let audio = recording.path().to_path_buf();
        let config = HearConfig {
            engine: Some(crate::Engine::Whisper),
            polish: false,
            ..HearConfig::default()
        };
        assert!(
            transcribe(
                &recording,
                &config,
                script,
                || panic!("local transcription must not access credentials"),
                &Cancellation::default()
            )
            .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(audio.with_extension("raw.txt")).unwrap(),
            "hello"
        );
        drop(recording);
        assert!(audio.exists());
    }
    #[test]
    fn retain_failed_recording_but_remove_delivered_one() {
        let p = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let path = p.to_path_buf();
        drop(Recording::new(p));
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
        let p = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let path = p.to_path_buf();
        Recording::new(p).delivered();
        assert!(!path.exists());
    }
}
