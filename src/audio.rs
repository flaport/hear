use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

enum RecordingEvent {
    Stop,
    StreamError(String),
}

pub fn record(destination: &Path) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .context("no default microphone was found")?;
    let supported = device
        .default_input_config()
        .context("could not read the default microphone configuration")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate;

    let samples = Arc::new(Mutex::new(Vec::<f32>::new()));
    let (event_tx, event_rx) = mpsc::channel();
    let error_tx = event_tx.clone();
    let error_callback = move |error: cpal::Error| {
        let _ = error_tx.send(RecordingEvent::StreamError(error.to_string()));
    };

    let stream = match sample_format {
        SampleFormat::F32 => build_stream(
            &device,
            &config,
            &samples,
            |value: f32| value,
            error_callback,
        ),
        SampleFormat::F64 => build_stream(
            &device,
            &config,
            &samples,
            |value: f64| value as f32,
            error_callback,
        ),
        SampleFormat::I8 => build_stream(
            &device,
            &config,
            &samples,
            |value: i8| value as f32 / 128.0,
            error_callback,
        ),
        SampleFormat::I16 => build_stream(
            &device,
            &config,
            &samples,
            |value: i16| value as f32 / 32_768.0,
            error_callback,
        ),
        SampleFormat::I32 => build_stream(
            &device,
            &config,
            &samples,
            |value: i32| value as f32 / 2_147_483_648.0,
            error_callback,
        ),
        SampleFormat::U8 => build_stream(
            &device,
            &config,
            &samples,
            |value: u8| (value as f32 - 128.0) / 128.0,
            error_callback,
        ),
        SampleFormat::U16 => build_stream(
            &device,
            &config,
            &samples,
            |value: u16| (value as f32 - 32_768.0) / 32_768.0,
            error_callback,
        ),
        SampleFormat::U32 => build_stream(
            &device,
            &config,
            &samples,
            |value: u32| (value as f64 - 2_147_483_648.0) as f32 / 2_147_483_648.0,
            error_callback,
        ),
        other => bail!("the default microphone uses an unsupported sample format: {other}"),
    }
    .context("could not open the default microphone")?;

    let recording_active = Arc::new(AtomicBool::new(true));
    let signal_recording_active = Arc::clone(&recording_active);
    ctrlc::set_handler(move || {
        if signal_recording_active.swap(false, Ordering::SeqCst) {
            let _ = event_tx.send(RecordingEvent::Stop);
        } else {
            std::process::exit(130);
        }
    })
    .context("could not install the Ctrl-C recording handler")?;

    eprintln!("Recording from the default microphone; press Ctrl-C to stop...");
    stream
        .play()
        .context("could not start microphone recording")?;

    match event_rx
        .recv()
        .context("recording control channel closed unexpectedly")?
    {
        RecordingEvent::Stop => {}
        RecordingEvent::StreamError(error) => bail!("microphone recording failed: {error}"),
    }
    drop(stream);
    recording_active.store(false, Ordering::SeqCst);

    let samples = Arc::try_unwrap(samples)
        .map_err(|_| anyhow::anyhow!("microphone recorder did not shut down cleanly"))?
        .into_inner()
        .map_err(|_| anyhow::anyhow!("microphone sample buffer was poisoned"))?;
    if samples.is_empty() {
        bail!("the microphone recording contained no audio");
    }

    let mono = downmix(&samples, channels);
    if mono.is_empty() {
        bail!("the microphone recording was too short to contain an audio frame");
    }
    let normalized = resample_linear(&mono, sample_rate, 16_000);
    write_wav(destination, &normalized)?;
    eprintln!("Recording complete ({}).", destination.display());
    Ok(())
}

fn build_stream<T, F, E>(
    device: &Device,
    config: &StreamConfig,
    samples: &Arc<Mutex<Vec<f32>>>,
    convert: F,
    error_callback: E,
) -> Result<Stream, cpal::Error>
where
    T: cpal::SizedSample,
    F: Fn(T) -> f32 + Send + 'static,
    E: FnMut(cpal::Error) + Send + 'static,
{
    let samples = Arc::clone(samples);
    device.build_input_stream(
        *config,
        move |input: &[T], _| {
            if let Ok(mut output) = samples.lock() {
                output.extend(input.iter().copied().map(&convert));
            }
        },
        error_callback,
        None,
    )
}

fn downmix(samples: &[f32], channels: usize) -> Vec<f32> {
    samples
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

fn resample_linear(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return samples.to_vec();
    }
    let output_len = samples.len() * target_rate as usize / source_rate as usize;
    let ratio = source_rate as f64 / target_rate as f64;
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

fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
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
    fn downmixes_stereo() {
        assert_eq!(downmix(&[1.0, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
    }

    #[test]
    fn resamples_to_expected_length() {
        let samples = vec![0.0; 48_000];
        assert_eq!(resample_linear(&samples, 48_000, 16_000).len(), 16_000);
    }
}
