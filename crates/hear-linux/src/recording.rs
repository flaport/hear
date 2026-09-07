use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use tempfile::TempPath;

pub struct Recorder {
    stream: Stream,
    samples: Arc<Mutex<MonoBuffer>>,
    sample_rate: u32,
}

impl Recorder {
    pub fn start() -> Result<Self> {
        let device = cpal::default_host()
            .default_input_device()
            .context("no default microphone was found")?;
        let supported = device
            .default_input_config()
            .context("could not read the default microphone configuration")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let samples = Arc::new(Mutex::new(MonoBuffer::new(usize::from(config.channels))));

        let stream = match sample_format {
            SampleFormat::F32 => build_stream(&device, &config, &samples, |value: f32| value),
            SampleFormat::F64 => {
                build_stream(&device, &config, &samples, |value: f64| value as f32)
            }
            SampleFormat::I8 => {
                build_stream(&device, &config, &samples, |value: i8| value as f32 / 128.0)
            }
            SampleFormat::I16 => build_stream(&device, &config, &samples, |value: i16| {
                value as f32 / 32_768.0
            }),
            SampleFormat::I32 => build_stream(&device, &config, &samples, |value: i32| {
                value as f32 / 2_147_483_648.0
            }),
            SampleFormat::U8 => build_stream(&device, &config, &samples, |value: u8| {
                (value as f32 - 128.0) / 128.0
            }),
            SampleFormat::U16 => build_stream(&device, &config, &samples, |value: u16| {
                (value as f32 - 32_768.0) / 32_768.0
            }),
            SampleFormat::U32 => build_stream(&device, &config, &samples, |value: u32| {
                (value as f64 - 2_147_483_648.0) as f32 / 2_147_483_648.0
            }),
            other => bail!("the default microphone uses an unsupported sample format: {other}"),
        }
        .context("could not open the default microphone")?;
        stream
            .play()
            .context("could not start microphone recording")?;

        Ok(Self {
            stream,
            samples,
            sample_rate: config.sample_rate,
        })
    }

    pub fn finish(self) -> Result<TempPath> {
        drop(self.stream);
        let samples = Arc::try_unwrap(self.samples)
            .map_err(|_| anyhow::anyhow!("microphone recorder did not shut down cleanly"))?
            .into_inner()
            .map_err(|_| anyhow::anyhow!("microphone sample buffer was poisoned"))?
            .finish();
        if samples.is_empty() {
            bail!("the microphone recording contained no audio");
        }

        let samples = resample(&samples, self.sample_rate, 16_000);
        let temporary = tempfile::Builder::new()
            .prefix("hear-linux-")
            .suffix(".wav")
            .tempfile()
            .context("could not create a temporary recording")?;
        write_wav(temporary.path(), &samples)?;
        Ok(temporary.into_temp_path())
    }
}

fn build_stream<T, F>(
    device: &Device,
    config: &StreamConfig,
    samples: &Arc<Mutex<MonoBuffer>>,
    convert: F,
) -> Result<Stream, cpal::Error>
where
    T: cpal::SizedSample,
    F: Fn(T) -> f32 + Send + 'static,
{
    let samples = Arc::clone(samples);
    device.build_input_stream(
        *config,
        move |input: &[T], _| {
            if let Ok(mut output) = samples.lock() {
                output.extend(input.iter().copied().map(&convert));
            }
        },
        move |error| eprintln!("microphone recording failed: {error}"),
        None,
    )
}

struct MonoBuffer {
    channels: usize,
    samples: Vec<f32>,
    frame_sum: f32,
    frame_samples: usize,
}

impl MonoBuffer {
    fn new(channels: usize) -> Self {
        Self {
            channels,
            samples: Vec::new(),
            frame_sum: 0.0,
            frame_samples: 0,
        }
    }

    fn extend(&mut self, samples: impl IntoIterator<Item = f32>) {
        for sample in samples {
            self.frame_sum += sample;
            self.frame_samples += 1;
            if self.frame_samples == self.channels {
                self.samples.push(self.frame_sum / self.channels as f32);
                self.frame_sum = 0.0;
                self.frame_samples = 0;
            }
        }
    }

    fn finish(self) -> Vec<f32> {
        self.samples
    }
}

fn resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return samples.to_vec();
    }
    let output_len = samples.len() * target_rate as usize / source_rate as usize;
    let ratio = source_rate as f64 / target_rate as f64;
    if source_rate > target_rate {
        (0..output_len)
            .map(|index| {
                let center = (index as f64 + 0.5) * ratio;
                let start = (center - ratio).floor().max(0.0) as usize;
                let end = (center + ratio).ceil().min(samples.len() as f64) as usize;
                samples[start..end].iter().sum::<f32>() / (end - start) as f32
            })
            .collect()
    } else {
        (0..output_len)
            .map(|index| {
                let position = index as f64 * ratio;
                let left = position.floor() as usize;
                let right = (left + 1).min(samples.len() - 1);
                let fraction = (position - left as f64) as f32;
                samples[left] * (1.0 - fraction) + samples[right] * fraction
            })
            .collect()
    }
}

fn write_wav(path: &std::path::Path, samples: &[f32]) -> Result<()> {
    let specification = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, specification)
        .context("could not create the temporary WAV recording")?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmixes_across_callbacks() {
        let mut buffer = MonoBuffer::new(2);
        buffer.extend([1.0]);
        buffer.extend([-1.0, 0.5, 0.5]);
        assert_eq!(buffer.finish(), [0.0, 0.5]);
    }

    #[test]
    fn resamples_to_target_length() {
        assert_eq!(resample(&vec![0.0; 48_000], 48_000, 16_000).len(), 16_000);
    }
}
