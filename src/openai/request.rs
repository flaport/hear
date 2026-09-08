use std::path::Path;

use crate::{Error, OpenAiClient, ProgressEvent};
use anyhow::{Context, Result};
use reqwest::blocking::multipart;
use serde::Deserialize;

use super::uploads::prepare;
use crate::openai_transport;

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub(crate) fn transcribe(input: &Path, vocabulary: &[String]) -> Result<String> {
    OpenAiClient::from_env()?
        .transcribe_raw(input, vocabulary)
        .map_err(Into::into)
}
pub(crate) fn transcribe_with_client(
    client: &OpenAiClient,
    input: &Path,
    vocabulary: &[String],
) -> std::result::Result<String, Error> {
    client.report(ProgressEvent::PreparingAudio);
    let uploads = prepare(input, &|message| {
        client.report(ProgressEvent::Message(message))
    })
    .map_err(Error::Input)?;
    let mut transcripts = Vec::new();
    for (index, path) in uploads.paths().iter().enumerate() {
        client.report(ProgressEvent::Uploading {
            part: index + 1,
            total: uploads.paths().len(),
        });
        transcripts.push(upload(client, path, vocabulary)?);
    }
    Ok(transcripts.join("\n"))
}

fn upload(
    client: &OpenAiClient,
    path: &Path,
    vocabulary: &[String],
) -> std::result::Result<String, Error> {
    let mut form = multipart::Form::new()
        .text("model", "gpt-transcribe")
        .file("file", path)
        .with_context(|| format!("could not open audio for upload: {}", path.display()))
        .map_err(Error::Input)?;
    for term in vocabulary {
        form = form.text("keywords[]", term.clone());
    }
    let response = client
        .http
        .post(client.endpoint("audio/transcriptions"))
        .bearer_auth(&client.key)
        .multipart(form)
        .send()
        .map_err(Error::Transport)?;
    let body = openai_transport::response_body(response)?;

    let response: TranscriptionResponse = serde_json::from_str(&body)
        .context("OpenAI returned an unexpected transcription response")
        .map_err(Error::Response)?;
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
            &OpenAiClient::builder(api_key).build().unwrap(),
            &path,
            &["Flaport".to_owned()],
        )
        .unwrap();
    }
}
