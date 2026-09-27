use clap::Args;
use hear::speech::{Cancellation, OpenAiSpeech, SpeechRequest, synthesize_to_wav};
use std::{io::Read, path::PathBuf, time::Duration};

#[derive(Debug, Args)]
pub struct SpeakArgs {
    /// Text to speak, or - to read UTF-8 text from stdin (up to 4096 characters).
    pub text: String,
    /// OpenAI voice.
    #[arg(long, default_value = "cedar")]
    pub voice: String,
    #[arg(long, default_value = "gpt-4o-mini-tts")]
    pub model: String,
    #[arg(long, default_value_t = 1.0)]
    pub speed: f32,
    /// Delivery instructions, such as "Speak warmly and slowly".
    #[arg(long)]
    pub instructions: Option<String>,
    /// Save a WAV file (defaults to speech.wav unless --play is used).
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Play the completed file using paplay (Linux) or afplay (macOS).
    #[arg(long)]
    pub play: bool,
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: &SpeakArgs, cancellation: &Cancellation) -> anyhow::Result<()> {
    let mut text = args.text.clone();
    if text == "-" {
        // A pipe or interactive stdin may stay open indefinitely. Keep Ctrl-C
        // responsive while a bounded reader waits; main exits on cancellation.
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut text = String::new();
            let result = std::io::stdin().take(16_385).read_to_string(&mut text);
            let _ = sender.send(result.map(|_| text));
        });
        text = loop {
            anyhow::ensure!(!cancellation.is_cancelled(), "speech cancelled");
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(result) => break result?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(error) => return Err(error.into()),
            }
        };
        anyhow::ensure!(text.len() <= 16_384, "input exceeds 4096 characters");
    }
    let temporary = tempfile::tempdir()?;
    let output = args.output.clone().unwrap_or_else(|| {
        if args.play {
            temporary.path().join("speech.wav")
        } else {
            PathBuf::from("speech.wav")
        }
    });
    let request = SpeechRequest {
        text: &text,
        voice: &args.voice,
        model: &args.model,
        speed: args.speed,
        instructions: args.instructions.as_deref(),
    };
    let summary = synthesize_to_wav(
        &OpenAiSpeech::from_env()?,
        &request,
        &output,
        args.force,
        cancellation,
    )?;
    eprintln!(
        "Generated {:.2}s of speech with {} in {:.2}s (first audio {:.2}s).",
        summary.audio_duration.as_secs_f64(),
        summary.voice,
        summary.request_duration.as_secs_f64(),
        summary.first_audio.as_secs_f64()
    );
    if args.output.is_some() || !args.play {
        eprintln!("Saved {}", output.display());
    }
    if args.play {
        let player = if cfg!(target_os = "macos") {
            "afplay"
        } else {
            "paplay"
        };
        let result = hear_core::process::run(
            std::process::Command::new(player).arg(&output),
            None,
            cancellation,
            summary.audio_duration + Duration::from_secs(30),
            false,
        )
        .map_err(|e| anyhow::anyhow!("playback with {player} failed: {e:#}"))?;
        anyhow::ensure!(
            result.status.success(),
            "{player} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(())
}
