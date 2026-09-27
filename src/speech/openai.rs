use super::*;
use crate::{OpenAiClient, openai_transport::response_body};
use serde_json::json;
use std::{io::Read, sync::Arc, time::Instant};

/// OpenAI Speech API adapter. The default is `gpt-4o-mini-tts` with Cedar.
pub struct OpenAiSpeech {
    client: OpenAiClient,
    timeout: Duration,
    observer: Option<Arc<dyn Fn(SpeechEvent) + Send + Sync>>,
}
impl OpenAiSpeech {
    pub fn new(client: OpenAiClient) -> Self {
        Self {
            client,
            timeout: Duration::from_secs(60),
            observer: None,
        }
    }
    pub fn from_env() -> Result<Self> {
        OpenAiClient::from_env()
            .map(Self::new)
            .map_err(SpeechError::OpenAi)
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    pub fn progress(mut self, observer: impl Fn(SpeechEvent) + Send + Sync + 'static) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }
    fn report(&self, event: SpeechEvent) {
        if let Some(observer) = &self.observer {
            observer(event);
        }
    }
    fn generate(
        &self,
        request: &SpeechRequest<'_>,
        sink: &mut dyn SpeechAudioSink,
        cancellation: &Cancellation,
    ) -> Result<SpeechSummary> {
        validate(request)?;
        check_cancelled(cancellation)?;
        let mut body = json!({"model":request.model,"voice":request.voice,"input":request.text,"speed":request.speed,"response_format":"pcm","stream_format":"audio"});
        if let Some(instructions) = request.instructions {
            body["instructions"] = instructions.into();
        }
        let start = Instant::now();
        let response = self
            .client
            .http
            .post(self.client.endpoint("audio/speech"))
            .bearer_auth(&self.client.key)
            .json(&body)
            .timeout(self.timeout)
            .send();
        check_cancelled(cancellation)?;
        let response = response.map_err(|e| SpeechError::OpenAi(crate::Error::Transport(e)))?;
        if !response.status().is_success() {
            let error = response_body(response).unwrap_err();
            check_cancelled(cancellation)?;
            return Err(SpeechError::OpenAi(error));
        }
        if let Some(content_type) = response.headers().get(reqwest::header::CONTENT_TYPE) {
            let content_type = content_type.to_str().unwrap_or("");
            if !content_type.starts_with("audio/")
                && !content_type.starts_with("application/octet-stream")
            {
                return Err(SpeechError::Decode(format!(
                    "unexpected content type {content_type}"
                )));
            }
        }
        let (samples, first_audio) = decode_pcm(response, sink, cancellation, start, |time| {
            self.report(SpeechEvent::FirstAudio(time))
        })?;
        Ok(SpeechSummary {
            engine: "openai".into(),
            model: request.model.into(),
            voice: request.voice.into(),
            samples,
            sample_rate: SAMPLE_RATE,
            audio_duration: Duration::from_secs_f64(samples as f64 / SAMPLE_RATE as f64),
            first_audio,
            request_duration: start.elapsed(),
        })
    }
}
impl SpeechEngine for OpenAiSpeech {
    fn synthesize(
        &self,
        request: &SpeechRequest<'_>,
        sink: &mut dyn SpeechAudioSink,
        cancellation: &Cancellation,
    ) -> Result<SpeechSummary> {
        self.report(SpeechEvent::Started);
        let result = self.generate(request, sink, cancellation);
        self.report(match &result {
            Ok(summary) => SpeechEvent::Completed(summary.clone()),
            Err(SpeechError::Cancelled) => SpeechEvent::Cancelled,
            Err(error) => SpeechEvent::Failed(error.to_string()),
        });
        result
    }
}

fn validate(request: &SpeechRequest<'_>) -> Result<()> {
    let invalid = |s: &str| SpeechError::Configuration(s.into());
    if request.text.trim().is_empty() || request.text.chars().count() > 4096 {
        return Err(invalid("text must contain 1–4096 characters"));
    }
    if request.model.trim().is_empty() || request.voice.trim().is_empty() {
        return Err(invalid("model and voice must not be empty"));
    }
    if !request.speed.is_finite() || !(0.25..=4.0).contains(&request.speed) {
        return Err(invalid("speed must be between 0.25 and 4.0"));
    }
    if request
        .instructions
        .is_some_and(|s| s.chars().count() > 4096)
    {
        return Err(invalid("instructions must not exceed 4096 characters"));
    }
    if request.instructions.is_some() && matches!(request.model, "tts-1" | "tts-1-hd") {
        return Err(invalid(
            "instructions are not supported by tts-1 or tts-1-hd",
        ));
    }
    Ok(())
}

fn decode_pcm(
    mut input: impl Read,
    sink: &mut dyn SpeechAudioSink,
    cancellation: &Cancellation,
    start: Instant,
    first: impl FnOnce(Duration),
) -> Result<(u64, Duration)> {
    let mut bytes = [0; 4096];
    let mut low = None;
    let mut total = 0;
    let mut first = Some(first);
    let mut first_time = None;
    loop {
        check_cancelled(cancellation)?;
        let read = input.read(&mut bytes);
        check_cancelled(cancellation)?;
        let count = match read {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.map_err(SpeechError::Network)?,
        };
        if count == 0 {
            break;
        }
        let mut samples = Vec::with_capacity(count.div_ceil(2));
        for &byte in &bytes[..count] {
            if let Some(previous) = low.take() {
                samples.push(i16::from_le_bytes([previous, byte]));
            } else {
                low = Some(byte);
            }
        }
        if samples.is_empty() {
            continue;
        }
        total += samples.len() as u64;
        if total > u64::from(SAMPLE_RATE) * 60 * 30 {
            return Err(SpeechError::Decode(
                "response exceeds 30 minutes of audio".into(),
            ));
        }
        if let Some(first) = first.take() {
            let elapsed = start.elapsed();
            first_time = Some(elapsed);
            first(elapsed);
        }
        check_cancelled(cancellation)?;
        sink.write(&samples)?;
    }
    if low.is_some() {
        return Err(SpeechError::Decode("truncated PCM16 sample".into()));
    }
    let first_time = first_time.ok_or_else(|| SpeechError::Decode("empty response".into()))?;
    Ok((total, first_time))
}

#[cfg(test)]
mod tests;
