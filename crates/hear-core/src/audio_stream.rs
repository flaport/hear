//! Bounded PCM transport. A slow consumer must never block microphone capture.
use anyhow::{Result, bail};
use std::sync::{Arc, Mutex, mpsc};

/// Mono signed 16-bit little-endian PCM at 16 kHz, in 100 ms packets.
pub const PACKET_BYTES: usize = 3200;
pub type AudioReceiver = mpsc::Receiver<Vec<u8>>;

#[derive(Clone, Default)]
pub struct StreamHealth(Arc<Mutex<Option<String>>>);
impl StreamHealth {
    pub fn check(&self) -> Result<()> {
        let error = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("stream state poisoned"))?;
        if let Some(message) = error.as_ref() {
            bail!("{message}");
        }
        Ok(())
    }
}

pub struct AudioSink {
    sender: mpsc::SyncSender<Vec<u8>>,
    health: StreamHealth,
}
impl AudioSink {
    /// Recording continues into the recovery WAV even if streaming fails.
    pub fn send(&self, packet: Vec<u8>) {
        if self.health.check().is_err() {
            return;
        }
        if let Err(error) = self.sender.try_send(packet) {
            let message = match error {
                mpsc::TrySendError::Full(_) => "streaming could not keep up with recording",
                mpsc::TrySendError::Disconnected(_) => "streaming transcription stopped early",
            };
            if let Ok(mut slot) = self.health.0.lock() {
                *slot = Some(message.into());
            }
        }
    }
}

pub fn channel() -> (AudioSink, AudioReceiver, StreamHealth) {
    // Thirty seconds of headroom for model/connection startup; never unbounded audio.
    channel_with_capacity(300)
}
fn channel_with_capacity(capacity: usize) -> (AudioSink, AudioReceiver, StreamHealth) {
    let (sender, receiver) = mpsc::sync_channel(capacity);
    let health = StreamHealth::default();
    (
        AudioSink {
            sender,
            health: health.clone(),
        },
        receiver,
        health,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_is_sticky_and_cannot_deliver_a_truncated_success() {
        let (sink, receiver, health) = channel_with_capacity(1);
        sink.send(vec![1, 2]);
        sink.send(vec![3, 4]);
        assert!(health.check().unwrap_err().to_string().contains("keep up"));
        assert_eq!(receiver.recv().unwrap(), [1, 2]);
        sink.send(vec![5, 6]);
        assert!(receiver.try_recv().is_err());
    }
}
