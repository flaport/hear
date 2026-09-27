//! Run with: cargo run --no-default-features --features tts --example speak -- "Hello"
use hear::speech::{Cancellation, OpenAiSpeech, SpeechRequest, synthesize_to_wav};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Hello from Hear.".into());
    let summary = synthesize_to_wav(
        &OpenAiSpeech::from_env()?,
        &SpeechRequest::new(&text),
        std::path::Path::new("speech.wav"),
        false,
        &Cancellation::default(),
    )?;
    eprintln!(
        "Saved speech.wav: {:.2}s, voice {}",
        summary.audio_duration.as_secs_f64(),
        summary.voice
    );
    Ok(())
}
