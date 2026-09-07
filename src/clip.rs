use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};

use crate::audio::processing::{MonoBuffer, resample};
use crate::audio::wav;
use crate::cli::{Cli, Engine};

fn pid_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_owned());
    PathBuf::from(dir).join("hear-clip.pid")
}

pub fn run(cli: &Cli) -> Result<()> {
    let pid_file = pid_path();

    if signal_running_instance(&pid_file) {
        return Ok(());
    }

    write_pid_file(&pid_file)?;
    let _cleanup = PidCleanup(&pid_file);

    let stop = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGUSR1,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGTERM,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&stop))
            .context("could not register signal handler")?;
    }

    eprintln!("Recording… (run `hear clip` again to stop)");
    let (stream, samples, sample_rate) = open_microphone()?;
    stream
        .play()
        .context("could not start microphone recording")?;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }

    drop(stream);
    let samples = Arc::try_unwrap(samples)
        .map_err(|_| anyhow::anyhow!("microphone recorder did not shut down cleanly"))?
        .into_inner()
        .map_err(|_| anyhow::anyhow!("microphone sample buffer was poisoned"))?
        .finish();
    if samples.is_empty() {
        bail!("the microphone recording contained no audio");
    }

    let samples = resample(&samples, sample_rate, 16_000);
    let recording = tempfile::Builder::new()
        .prefix("hear-clip-")
        .suffix(".wav")
        .tempfile()
        .context("could not create a temporary recording")?;
    wav::write(recording.path(), &samples)?;

    let dictionary = crate::dictionary::Dictionary::load()?;
    let vocabulary = dictionary.canonical_terms();

    eprintln!("Transcribing with {}...", cli.engine);
    let raw_transcript = match cli.engine {
        Engine::GptTranscribe => hear::transcribe_openai_raw(recording.path(), &vocabulary)?,
        Engine::Codex => crate::engines::codex::transcribe(
            recording.path(),
            cli.model.as_deref(),
            &vocabulary,
        )?,
        Engine::Whisper => crate::engines::whisper::transcribe(
            recording.path(),
            cli.model.as_deref().unwrap_or("tiny.en"),
            cli.language.as_deref().unwrap_or("en"),
            &vocabulary,
        )?,
    };
    let raw_transcript = dictionary.correct_aliases(&raw_transcript)?;

    let transcript = if cli.should_polish() {
        eprintln!("Polishing transcript…");
        let dictionary_context = dictionary.formatter_context();
        hear::polish(
            &raw_transcript,
            cli.format_context(),
            dictionary_context.as_deref(),
        )?
    } else {
        raw_transcript
    };

    deliver(&transcript);
    print!("{transcript}");
    Ok(())
}

fn open_microphone() -> Result<(Stream, Arc<Mutex<MonoBuffer>>, u32)> {
    let device = cpal::default_host()
        .default_input_device()
        .context("no default microphone was found")?;
    let supported = device
        .default_input_config()
        .context("could not read the default microphone configuration")?;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let sample_rate = config.sample_rate;
    let samples = Arc::new(Mutex::new(MonoBuffer::new(usize::from(config.channels))));

    let stream = match sample_format {
        SampleFormat::F32 => build_stream(&device, &config, &samples, |v: f32| v),
        SampleFormat::F64 => build_stream(&device, &config, &samples, |v: f64| v as f32),
        SampleFormat::I8 => build_stream(&device, &config, &samples, |v: i8| v as f32 / 128.0),
        SampleFormat::I16 => {
            build_stream(&device, &config, &samples, |v: i16| v as f32 / 32_768.0)
        }
        SampleFormat::I32 => build_stream(
            &device,
            &config,
            &samples,
            |v: i32| v as f32 / 2_147_483_648.0,
        ),
        SampleFormat::U8 => {
            build_stream(&device, &config, &samples, |v: u8| {
                (v as f32 - 128.0) / 128.0
            })
        }
        SampleFormat::U16 => build_stream(&device, &config, &samples, |v: u16| {
            (v as f32 - 32_768.0) / 32_768.0
        }),
        SampleFormat::U32 => build_stream(&device, &config, &samples, |v: u32| {
            (v as f64 - 2_147_483_648.0) as f32 / 2_147_483_648.0
        }),
        other => bail!("the default microphone uses an unsupported sample format: {other}"),
    }
    .context("could not open the default microphone")?;

    Ok((stream, samples, sample_rate))
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

fn deliver(transcript: &str) {
    if !copy_to_clipboard(transcript) {
        eprintln!("Could not copy to clipboard.");
        return;
    }

    if post_paste() {
        eprintln!("Pasted.");
    } else {
        eprintln!("Copied to clipboard.");
    }
}

fn copy_to_clipboard(text: &str) -> bool {
    if is_wayland() {
        std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
            .is_ok_and(|s| s.success())
    } else {
        std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
            .is_ok_and(|s| s.success())
    }
}

fn post_paste() -> bool {
    if is_wayland() {
        std::process::Command::new("wtype")
            .args(["-M", "ctrl", "-P", "v", "-m", "ctrl", "-p", "v"])
            .status()
            .is_ok_and(|s| s.success())
    } else {
        std::process::Command::new("xdotool")
            .args(["key", "--clearmodifiers", "ctrl+v"])
            .status()
            .is_ok_and(|s| s.success())
    }
}

fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty())
}

fn signal_running_instance(pid_file: &PathBuf) -> bool {
    let Ok(contents) = fs::read_to_string(pid_file) else {
        return false;
    };
    let Ok(pid) = contents.trim().parse::<i32>() else {
        return false;
    };
    unsafe {
        if libc::kill(pid, 0) == 0 {
            libc::kill(pid, libc::SIGUSR1);
            return true;
        }
    }
    let _ = fs::remove_file(pid_file);
    false
}

fn write_pid_file(pid_file: &PathBuf) -> Result<()> {
    let mut file = fs::File::create(pid_file).context("could not create PID file")?;
    write!(file, "{}", std::process::id())?;
    Ok(())
}

struct PidCleanup<'a>(&'a PathBuf);

impl Drop for PidCleanup<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}
