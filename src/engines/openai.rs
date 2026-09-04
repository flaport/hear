use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;
use tempfile::TempDir;

const MAX_UPLOAD_BYTES: u64 = 25_000_000;
const TRANSCRIPTIONS_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

struct Uploads {
    paths: Vec<PathBuf>,
    _temporary_files: Option<TempDir>,
}

pub fn transcribe(input: &Path) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        anyhow::anyhow!(
            "OPENAI_API_KEY is not set; set it or choose --engine codex/2 or --engine whisper/3"
        )
    })?;
    let uploads = prepare_uploads(input)?;
    let client = Client::builder()
        .build()
        .context("could not initialize the OpenAI HTTP client")?;

    let mut transcripts = Vec::with_capacity(uploads.paths.len());
    for (index, path) in uploads.paths.iter().enumerate() {
        if uploads.paths.len() > 1 {
            eprintln!("Uploading part {} of {}...", index + 1, uploads.paths.len());
        }
        transcripts.push(upload(&client, &api_key, path)?);
    }
    Ok(transcripts.join("\n"))
}

fn upload(client: &Client, api_key: &str, path: &Path) -> Result<String> {
    let form = multipart::Form::new()
        .text("model", "gpt-transcribe")
        .file("file", path)
        .with_context(|| format!("could not open audio for upload: {}", path.display()))?;
    let response = client
        .post(TRANSCRIPTIONS_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .context("OpenAI transcription request failed")?;
    let status = response.status();
    let body = response
        .text()
        .context("could not read the OpenAI transcription response")?;

    if !status.is_success() {
        let message = serde_json::from_str::<ApiErrorEnvelope>(&body)
            .map(|envelope| envelope.error.message)
            .unwrap_or_else(|_| body.trim().to_owned());
        bail!("OpenAI transcription failed ({status}): {message}");
    }

    let response: TranscriptionResponse = serde_json::from_str(&body)
        .context("OpenAI returned an unexpected transcription response")?;
    Ok(response.text.trim().to_owned())
}

fn prepare_uploads(input: &Path) -> Result<Uploads> {
    let size = fs::metadata(input)
        .with_context(|| format!("could not inspect audio file: {}", input.display()))?
        .len();
    let supported = is_supported_upload(input);
    if supported && size <= MAX_UPLOAD_BYTES {
        return Ok(Uploads {
            paths: vec![input.to_path_buf()],
            _temporary_files: None,
        });
    }

    require_ffmpeg(if size > MAX_UPLOAD_BYTES {
        "compressing or splitting an audio file larger than 25 MB"
    } else {
        "converting this audio format for OpenAI"
    })?;
    let directory = tempfile::Builder::new()
        .prefix("hear-openai-")
        .tempdir()
        .context("could not create temporary audio directory")?;

    let paths = if size > MAX_UPLOAD_BYTES {
        eprintln!(
            "Warning: {} is larger than OpenAI's 25 MB upload limit; compressing and splitting it with FFmpeg.",
            input.display()
        );
        let pattern = directory.path().join("part-%04d.mp3");
        run_ffmpeg(
            input,
            &[
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-b:a",
                "32k",
                "-f",
                "segment",
                "-segment_time",
                "2700",
                "-reset_timestamps",
                "1",
            ],
            &pattern,
            "compress and split the audio",
        )?;
        let mut paths = fs::read_dir(directory.path())
            .context("could not read temporary audio chunks")?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "mp3"))
            .collect::<Vec<_>>();
        paths.sort();
        paths
    } else {
        eprintln!(
            "Converting unsupported input format to MP3 with FFmpeg: {}",
            input.display()
        );
        let converted = directory.path().join("converted.mp3");
        run_ffmpeg(
            input,
            &["-vn", "-ac", "1", "-ar", "16000", "-b:a", "64k"],
            &converted,
            "convert the audio to MP3",
        )?;
        vec![converted]
    };

    if paths.is_empty() {
        bail!("FFmpeg produced no audio to upload");
    }
    for path in &paths {
        let size = fs::metadata(path)?.len();
        if size > MAX_UPLOAD_BYTES {
            bail!(
                "FFmpeg output still exceeds OpenAI's 25 MB limit: {}",
                path.display()
            );
        }
    }

    Ok(Uploads {
        paths,
        _temporary_files: Some(directory),
    })
}

fn is_supported_upload(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "mp3" | "mp4" | "mpeg" | "mpga" | "m4a" | "wav" | "webm"
            )
        })
        .unwrap_or(false)
}

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
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(arguments)
        .arg(output)
        .status()
        .with_context(|| format!("could not launch FFmpeg to {operation}"))?;
    if !status.success() {
        bail!("FFmpeg could not {operation}: {}", input.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_openai_formats_case_insensitively() {
        assert!(is_supported_upload(Path::new("message.M4A")));
        assert!(is_supported_upload(Path::new("message.webm")));
        assert!(!is_supported_upload(Path::new("message.flac")));
    }
}
