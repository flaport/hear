use std::path::Path;

use anyhow::{Context, Result};
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;

use super::uploads::prepare;
use crate::openai_transport;

const TRANSCRIPTIONS_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub(crate) fn transcribe(input: &Path, vocabulary: &[String]) -> Result<String> {
    let api_key = openai_transport::api_key().map_err(|_| {
        anyhow::anyhow!(
            "OPENAI_API_KEY is not set; set it or choose --engine codex/2 or --engine whisper/3"
        )
    })?;
    let uploads = prepare(input)?;
    let client = openai_transport::client()?;

    let mut transcripts = Vec::with_capacity(uploads.paths().len());
    for (index, path) in uploads.paths().iter().enumerate() {
        if uploads.paths().len() > 1 {
            eprintln!(
                "Uploading part {} of {}...",
                index + 1,
                uploads.paths().len()
            );
        }
        transcripts.push(upload(&client, &api_key, path, vocabulary)?);
    }
    Ok(transcripts.join("\n"))
}

fn upload(client: &Client, api_key: &str, path: &Path, vocabulary: &[String]) -> Result<String> {
    let mut form = multipart::Form::new()
        .text("model", "gpt-transcribe")
        .file("file", path)
        .with_context(|| format!("could not open audio for upload: {}", path.display()))?;
    for term in vocabulary {
        form = form.text("keywords[]", term.clone());
    }
    let response = client
        .post(TRANSCRIPTIONS_URL)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .context("OpenAI transcription request failed")?;
    let body = openai_transport::response_body(response, "transcription")?;

    let response: TranscriptionResponse = serde_json::from_str(&body)
        .context("OpenAI returned an unexpected transcription response")?;
    Ok(response.text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "calls the live OpenAI API"]
    fn live_transcription_accepts_dictionary_keywords() {
        let api_key = std::env::var("OPENAI_API_KEY").unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("silence.wav");
        let specification = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, specification).unwrap();
        for _ in 0..16_000 {
            writer.write_sample(0_i16).unwrap();
        }
        writer.finalize().unwrap();

        upload(
            &openai_transport::client().unwrap(),
            &api_key,
            &path,
            &["Flaport".to_owned()],
        )
        .unwrap();
    }
}
