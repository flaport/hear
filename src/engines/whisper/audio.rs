use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::ffmpeg::{require_ffmpeg, run_ffmpeg};

pub(super) fn load(input: &Path) -> Result<Vec<f32>> {
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
    fn reads_already_normalized_wav() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audio.wav");
        let specification = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, specification).unwrap();
        writer.write_sample(16_384_i16).unwrap();
        writer.finalize().unwrap();

        assert_eq!(read_normalized_wav(&path).unwrap(), Some(vec![0.5]));
    }
}
