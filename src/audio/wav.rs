use std::path::Path;

use anyhow::{Context, Result};

pub(super) fn write(path: &Path, samples: &[f32]) -> Result<()> {
    let specification = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, specification)
        .with_context(|| format!("could not create recording: {}", path.display()))?;
    for sample in samples {
        let sample = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(sample)
            .context("could not write microphone samples")?;
    }
    writer
        .finalize()
        .context("could not finalize WAV recording")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_normalized_mono_wav() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recording.wav");
        write(&path, &[-1.5, 0.0, 1.5]).unwrap();

        let mut reader = hound::WavReader::open(path).unwrap();
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert_eq!(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap(),
            [i16::MIN + 1, 0, i16::MAX]
        );
    }
}
