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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw: String,
    pub text: String,
}

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
    transcribe_full(recording, hear, helper, key, cancellation).map(|transcript| transcript.text)
}

pub fn transcribe_full(
    recording: &Recording,
    hear: &HearConfig,
    helper: PathBuf,
    key: impl FnOnce() -> Result<Option<String>>,
    cancellation: &Cancellation,
) -> Result<Transcript> {
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
    decode(recording, output)
}

fn decode(recording: &Recording, output: std::process::Output) -> Result<Transcript> {
    let response: Response = serde_json::from_slice(&output.stdout).with_context(|| {
        format!(
            "hear returned an invalid helper response: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })?;
    match response {
        Response::Success { raw, text } if output.status.success() && !text.trim().is_empty() => {
            Ok(Transcript { raw, text })
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

/// Streaming runs in the same isolated, cancellable helper as file transcription.
pub struct Streaming {
    job: process::Job,
    result: std::sync::mpsc::Receiver<Result<std::process::Output>>,
    health: crate::audio_stream::StreamHealth,
}
impl Streaming {
    pub fn start(
        hear: &HearConfig,
        helper: PathBuf,
        key: impl FnOnce() -> Result<Option<String>>,
    ) -> Result<(Self, crate::audio_stream::AudioSink)> {
        hear.preflight()?;
        let mut command = Command::new(helper);
        command
            .args(hear.arguments())
            .args(["--json", "--pcm-stdin"]);
        if hear.requires_openai() && std::env::var_os("OPENAI_API_KEY").is_none() {
            let key = key()?.context("no OpenAI API key configured")?;
            command.env("OPENAI_API_KEY", key);
        }
        let (sink, audio, health) = crate::audio_stream::channel();
        let (sender, result) = std::sync::mpsc::channel();
        let job = process::Job::spawn(move |cancellation| {
            let output = process::run_streaming(
                &mut command,
                audio,
                &cancellation,
                Duration::from_secs(3600),
            );
            let _ = sender.send(output);
        });
        Ok((
            Self {
                job,
                result,
                health,
            },
            sink,
        ))
    }

    /// Call only after capture stops and closes the audio sender.
    pub fn finish(self, recording: &Recording, cancellation: &Cancellation) -> Result<Transcript> {
        let result = (|| {
            let output = loop {
                if cancellation.is_cancelled() {
                    bail!("operation cancelled");
                }
                match self.result.recv_timeout(Duration::from_millis(20)) {
                    Ok(result) => break result?,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(error) => return Err(error.into()),
                }
            };
            let text = decode(recording, output)?;
            self.health.check()?;
            Ok(text)
        })();
        // Dropping the job cancels and reaps on every exit, including cancellation.
        drop(self.job);
        result.with_context(|| {
            format!(
                "streaming failed; recording retained at {}",
                recording.path().display()
            )
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn streaming_failure_keeps_audio_and_available_raw_transcript() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("helper");
        std::fs::write(&script, "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"status\":\"failure\",\"phase\":\"polishing\",\"message\":\"quota\",\"raw\":\"last words\"}'\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let config = HearConfig {
            stream: true,
            engine: Some(crate::Engine::Whisper),
            polish: false,
            ..Default::default()
        };
        let (streaming, sink) = Streaming::start(&config, script, || {
            panic!("local streaming must not access credentials")
        })
        .unwrap();
        sink.send(vec![1, 2]);
        drop(sink);
        let recording = Recording::new(
            tempfile::NamedTempFile::new_in(directory.path())
                .unwrap()
                .into_temp_path(),
        );
        let path = recording.path().to_path_buf();
        let error = streaming
            .finish(&recording, &Cancellation::default())
            .unwrap_err();
        assert!(format!("{error:#}").contains("quota"));
        assert_eq!(
            std::fs::read_to_string(path.with_extension("raw.txt")).unwrap(),
            "last words"
        );
        drop(recording);
        assert!(path.is_file());
    }
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
