use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use super::processing::MonoBuffer;

enum RecordingEvent {
    Finish,
    Cancel,
    InputError(String),
    StreamError(String),
}

pub(super) struct CapturedAudio {
    pub(super) samples: Vec<f32>,
    pub(super) sample_rate: u32,
}

pub(super) fn capture() -> Result<Option<CapturedAudio>> {
    if !io::stdin().is_terminal() {
        bail!("microphone recording requires an interactive terminal for Return to finish");
    }
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

    let samples = Arc::new(Mutex::new(MonoBuffer::new(channels)));
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

    let input_tx = event_tx.clone();
    let recording_active = Arc::new(AtomicBool::new(true));
    let signal_recording_active = Arc::clone(&recording_active);
    ctrlc::set_handler(move || {
        if signal_recording_active.swap(false, Ordering::SeqCst) {
            let _ = event_tx.send(RecordingEvent::Cancel);
        } else {
            std::process::exit(130);
        }
    })
    .context("could not install the Ctrl-C recording handler")?;

    eprintln!(
        "Recording from the default microphone; press Return to transcribe or Ctrl-C to cancel..."
    );
    stream
        .play()
        .context("could not start microphone recording")?;

    let input_recording_active = Arc::clone(&recording_active);
    thread::spawn(move || {
        let mut input = String::new();
        let event = match io::stdin().read_line(&mut input) {
            Ok(0) => RecordingEvent::InputError("standard input was closed".to_owned()),
            Ok(_) => RecordingEvent::Finish,
            Err(error) => RecordingEvent::InputError(error.to_string()),
        };
        if input_recording_active.swap(false, Ordering::SeqCst) {
            let _ = input_tx.send(event);
        }
    });

    let event = event_rx
        .recv()
        .context("recording control channel closed unexpectedly")?;
    drop(stream);
    recording_active.store(false, Ordering::SeqCst);
    match event {
        RecordingEvent::Finish => {}
        RecordingEvent::Cancel => {
            eprintln!("Recording cancelled.");
            return Ok(None);
        }
        RecordingEvent::InputError(error) => bail!("could not read recording controls: {error}"),
        RecordingEvent::StreamError(error) => bail!("microphone recording failed: {error}"),
    }

    let samples = Arc::try_unwrap(samples)
        .map_err(|_| anyhow::anyhow!("microphone recorder did not shut down cleanly"))?
        .into_inner()
        .map_err(|_| anyhow::anyhow!("microphone sample buffer was poisoned"))?
        .finish();
    if samples.is_empty() {
        bail!("the microphone recording contained no audio");
    }
    Ok(Some(CapturedAudio {
        samples,
        sample_rate,
    }))
}

fn build_stream<T, F, E>(
    device: &Device,
    config: &StreamConfig,
    samples: &Arc<Mutex<MonoBuffer>>,
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
