//! Bounded microphone capture. The callback only downmixes and sends fixed-size packets.
use anyhow::{Context, Result, bail};
use cpal::{
    Device, SampleFormat, Stream, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, SyncSender},
    },
    thread::{self, JoinHandle},
};
use tempfile::TempPath;
const BLOCK: usize = 1024;
struct Packet {
    samples: [f32; BLOCK],
    len: usize,
}
#[derive(Clone, Default)]
struct Failure(Arc<Mutex<Option<String>>>);
impl Failure {
    fn set(&self, message: impl Into<String>) {
        if let Ok(mut slot) = self.0.lock()
            && slot.is_none()
        {
            *slot = Some(message.into());
        }
    }
    fn check(&self) -> Result<()> {
        if let Some(e) = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("recording state poisoned"))?
            .as_ref()
        {
            bail!("microphone recording failed: {e}");
        }
        Ok(())
    }
}
pub struct Recorder {
    stream: Stream,
    pending: PendingRecording,
}
pub struct PendingRecording {
    worker: Option<JoinHandle<Result<TempPath>>>,
    failure: Failure,
}
impl Recorder {
    pub fn start() -> Result<Self> {
        let device = cpal::default_host()
            .default_input_device()
            .context("no default microphone was found")?;
        let supported = device
            .default_input_config()
            .context("could not read microphone configuration")?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let temporary = tempfile::Builder::new()
            .prefix("hear-recording-")
            .suffix(".wav")
            .tempfile()?
            .into_temp_path();
        let (tx, rx) = mpsc::sync_channel::<Packet>(32);
        let failure = Failure::default();
        let worker_failure = failure.clone();
        let rate = config.sample_rate;
        let worker = thread::spawn(move || {
            let result = (|| {
                let spec = hound::WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                };
                let mut writer = hound::WavWriter::create(&temporary, spec)?;
                let mut resampler = Resampler::new(rate, 16000);
                let mut count = 0_u64;
                for packet in rx {
                    for sample in &packet.samples[..packet.len] {
                        resampler.push(*sample, |v| {
                            writer.write_sample((v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
                            count += 1;
                            Ok(())
                        })?;
                    }
                }
                writer.finalize()?;
                if count == 0 {
                    bail!("the microphone recording contained no audio");
                }
                Ok(temporary)
            })();
            if let Err(e) = &result {
                worker_failure.set(format!("{e:#}"));
            }
            result
        });
        macro_rules! build {
            ($t:ty, $f:expr) => {
                build_stream::<$t, _>(&device, &config, tx, failure.clone(), $f)
            };
        }
        let stream = match format {
            SampleFormat::F32 => build!(f32, |v: f32| v),
            SampleFormat::F64 => build!(f64, |v: f64| v as f32),
            SampleFormat::I8 => build!(i8, |v: i8| v as f32 / 128.0),
            SampleFormat::I16 => build!(i16, |v: i16| v as f32 / 32768.0),
            SampleFormat::I32 => build!(i32, |v: i32| v as f32 / 2147483648.0),
            SampleFormat::U8 => build!(u8, |v: u8| (v as f32 - 128.0) / 128.0),
            SampleFormat::U16 => build!(u16, |v: u16| (v as f32 - 32768.0) / 32768.0),
            SampleFormat::U32 => build!(u32, |v: u32| ((v as f64 - 2147483648.0) / 2147483648.0)
                as f32),
            other => bail!("unsupported microphone format: {other}"),
        }
        .context("could not open microphone")?;
        stream.play().context("could not start microphone")?;
        Ok(Self {
            stream,
            pending: PendingRecording {
                worker: Some(worker),
                failure,
            },
        })
    }
    pub fn check(&self) -> Result<()> {
        self.pending.failure.check()
    }
    /// Stop the stream on its owning thread; finish disk IO on a worker.
    pub fn stop(self) -> PendingRecording {
        drop(self.stream);
        self.pending
    }
    pub fn finish(self) -> Result<TempPath> {
        self.stop().finish()
    }
}
impl PendingRecording {
    pub fn finish(mut self) -> Result<TempPath> {
        let result = self
            .worker
            .take()
            .expect("recording worker")
            .join()
            .map_err(|_| anyhow::anyhow!("recording worker panicked"))?;
        self.failure.check()?;
        result
    }
}
fn build_stream<T, F>(
    device: &Device,
    config: &StreamConfig,
    tx: SyncSender<Packet>,
    failure: Failure,
    convert: F,
) -> std::result::Result<Stream, cpal::Error>
where
    T: cpal::SizedSample,
    F: Fn(T) -> f32 + Send + 'static,
{
    let channels = usize::from(config.channels);
    let mut sum = 0.0;
    let mut n = 0;
    let errors = failure.clone();
    device.build_input_stream(
        *config,
        move |input: &[T], _| {
            let mut packet = Packet {
                samples: [0.0; BLOCK],
                len: 0,
            };
            for value in input {
                sum += convert(*value);
                n += 1;
                if n == channels {
                    packet.samples[packet.len] = sum / channels as f32;
                    packet.len += 1;
                    sum = 0.0;
                    n = 0;
                    if packet.len == BLOCK {
                        if tx.try_send(packet).is_err() {
                            failure.set("audio writer could not keep up; recording is incomplete");
                            return;
                        }
                        packet = Packet {
                            samples: [0.0; BLOCK],
                            len: 0,
                        };
                    }
                }
            }
            if packet.len > 0 && tx.try_send(packet).is_err() {
                failure.set("audio writer could not keep up; recording is incomplete");
            }
        },
        move |e| errors.set(e.to_string()),
        None,
    )
}
/// Area resampling keeps constant signals and fractional sample-rate ratios stable.
struct Resampler {
    source: u64,
    target: u64,
    phase: u64,
    sum: f64,
}
impl Resampler {
    fn new(source: u32, target: u32) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            phase: 0,
            sum: 0.0,
        }
    }
    fn push(&mut self, v: f32, mut emit: impl FnMut(f32) -> Result<()>) -> Result<()> {
        let mut remaining = self.target;
        while remaining > 0 {
            let take = remaining.min(self.source - self.phase);
            self.sum += f64::from(v) * take as f64;
            self.phase += take;
            remaining -= take;
            if self.phase == self.source {
                emit((self.sum / self.source as f64) as f32)?;
                self.phase = 0;
                self.sum = 0.0;
            }
        }
        Ok(())
    }
}
impl Drop for PendingRecording {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaming_resampling_preserves_rate_and_constants() {
        for source in [8000, 44100, 48000] {
            let mut r = Resampler::new(source, 16000);
            let mut out = Vec::new();
            for _ in 0..source {
                r.push(0.25, |v| {
                    out.push(v);
                    Ok(())
                })
                .unwrap();
            }
            assert_eq!(out.len(), 16000);
            assert!(out.iter().all(|v| (*v - 0.25).abs() < 1e-6));
        }
    }
    #[test]
    fn microphone_error_prevents_success() {
        let f = Failure::default();
        f.set("device disconnected");
        assert!(f.check().is_err());
        f.set("second error");
        assert!(f.check().unwrap_err().to_string().contains("disconnected"));
    }
}
