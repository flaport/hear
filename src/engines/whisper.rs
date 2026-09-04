use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use reqwest::blocking::Client;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::openai::{require_ffmpeg, run_ffmpeg};

const MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub fn transcribe(input: &Path, model: &str) -> Result<String> {
    let model_path = ensure_model(model)?;
    let samples = load_audio(input)?;
    eprintln!("Running whisper.cpp model {model}...");

    let model_path = model_path
        .to_str()
        .context("Whisper model path is not valid UTF-8")?;
    let context = WhisperContext::new_with_params(model_path, WhisperContextParameters::default())
        .context("could not load the whisper.cpp model")?;
    let mut state = context
        .create_state()
        .context("could not initialize whisper.cpp")?;
    let mut parameters = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    parameters.set_language(Some("en"));
    parameters.set_translate(false);
    parameters.set_n_threads(
        std::thread::available_parallelism()
            .map(|threads| threads.get().min(8) as i32)
            .unwrap_or(4),
    );
    parameters.set_print_special(false);
    parameters.set_print_progress(false);
    parameters.set_print_realtime(false);
    parameters.set_print_timestamps(false);

    state
        .full(parameters, &samples)
        .context("whisper.cpp could not transcribe the audio")?;
    let transcript = state
        .as_iter()
        .map(|segment| segment.to_string())
        .collect::<String>();
    Ok(transcript.trim().to_owned())
}

fn ensure_model(model: &str) -> Result<PathBuf> {
    let filename = model_filename(model).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown Whisper model '{model}'; choose tiny.en, base.en, small.en, medium.en, or large-v3-turbo"
        )
    })?;
    let base = BaseDirs::new().context("could not determine the platform cache directory")?;
    let directory = base.cache_dir().join("hear").join("models");
    let destination = directory.join(filename);
    if destination.is_file() {
        return Ok(destination);
    }

    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "could not create the Whisper model cache: {}",
            directory.display()
        )
    })?;
    let url = format!("{MODEL_BASE_URL}/{filename}");
    eprintln!(
        "Downloading Whisper model {model} to {}...",
        destination.display()
    );
    let client = Client::builder()
        .build()
        .context("could not initialize the model download client")?;
    let mut response = client
        .get(url)
        .send()
        .context("could not download the Whisper model")?
        .error_for_status()
        .context("Whisper model download was rejected")?;
    let expected_length = response.content_length();
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)
        .context("could not create a temporary model file")?;
    let downloaded = std::io::copy(&mut response, &mut temporary)
        .context("could not save the downloaded Whisper model")?;
    temporary
        .flush()
        .context("could not flush the downloaded Whisper model")?;
    if let Some(expected_length) = expected_length
        && downloaded != expected_length
    {
        bail!(
            "Whisper model download was incomplete (received {downloaded} of {expected_length} bytes)"
        );
    }
    if downloaded < 1_000_000 {
        bail!("downloaded Whisper model is unexpectedly small ({downloaded} bytes)");
    }
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .with_context(|| format!("could not install Whisper model: {}", destination.display()))?;
    Ok(destination)
}

fn model_filename(model: &str) -> Option<&'static str> {
    match model {
        "tiny.en" => Some("ggml-tiny.en.bin"),
        "base.en" => Some("ggml-base.en.bin"),
        "small.en" => Some("ggml-small.en.bin"),
        "medium.en" => Some("ggml-medium.en.bin"),
        "large-v3-turbo" => Some("ggml-large-v3-turbo.bin"),
        _ => None,
    }
}

fn load_audio(input: &Path) -> Result<Vec<f32>> {
    if let Some(samples) = read_normalized_wav(input)? {
        return Ok(samples);
    }

    require_ffmpeg("converting audio to the 16 kHz mono WAV format required by whisper.cpp")?;
    eprintln!("Converting audio for whisper.cpp with FFmpeg...");
    let directory = tempfile::Builder::new()
        .prefix("hear-whisper-")
        .tempdir()
        .context("could not create a temporary audio directory")?;
    let converted = directory.path().join("audio.wav");
    run_ffmpeg(
        input,
        &["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"],
        &converted,
        "convert the audio for whisper.cpp",
    )?;
    read_normalized_wav(&converted)?.context("FFmpeg produced an invalid WAV file")
}

fn read_normalized_wav(path: &Path) -> Result<Option<Vec<f32>>> {
    let reader = match hound::WavReader::open(path) {
        Ok(reader) => reader,
        Err(_) => return Ok(None),
    };
    let specification = reader.spec();
    if specification.channels != 1
        || specification.sample_rate != 16_000
        || specification.bits_per_sample != 16
        || specification.sample_format != hound::SampleFormat::Int
    {
        return Ok(None);
    }
    let samples = reader
        .into_samples::<i16>()
        .map(|sample| {
            sample
                .map(|sample| sample as f32 / 32_768.0)
                .context("WAV contains an invalid sample")
        })
        .collect::<Result<Vec<_>>>()?;
    if samples.is_empty() {
        bail!("audio contains no samples: {}", path.display());
    }
    Ok(Some(samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_supported_model_names() {
        assert_eq!(model_filename("tiny.en"), Some("ggml-tiny.en.bin"));
        assert_eq!(
            model_filename("large-v3-turbo"),
            Some("ggml-large-v3-turbo.bin")
        );
        assert_eq!(model_filename("surprise"), None);
    }
}
