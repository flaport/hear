//! Streaming transcription adapters share PCM input and final transcript output.
//! UI delivery and polishing are deliberately outside the adapter contract.
mod openai;
mod whisper;

use crate::{Engine, HearConfig, OpenAiClient};
use anyhow::{Result, bail};
use std::io::Read;

/// Receives mono 16 kHz signed PCM samples. `finish` flushes the last audio.
pub trait Adapter {
    fn push(&mut self, samples: &[i16]) -> Result<()>;
    fn finish(self: Box<Self>) -> Result<String>;
}

pub(crate) fn adapter(
    config: &HearConfig,
    vocabulary: &[String],
    client: Option<&OpenAiClient>,
) -> Result<Box<dyn Adapter>> {
    match config.resolved_engine() {
        Engine::Whisper => Ok(Box::new(whisper::Whisper::new(
            config.model.as_deref().unwrap_or("tiny.en"),
            config.language.as_deref().unwrap_or("en"),
            vocabulary,
        )?)),
        Engine::GptTranscribe => Ok(Box::new(openai::Realtime::new(
            client.ok_or_else(|| anyhow::anyhow!("OpenAI client required"))?,
            vocabulary,
        )?)),
        Engine::Codex => bail!("Codex does not support streaming"),
    }
}

/// Stream PCM16 LE from a reader, preserving samples across arbitrary read boundaries.
pub fn transcribe(mut input: impl Read, mut adapter: Box<dyn Adapter>) -> Result<String> {
    let mut bytes = [0_u8; hear_core::audio_stream::PACKET_BYTES];
    let mut low = None;
    let mut count = 0;
    loop {
        let n = match input.read(&mut bytes) {
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            break;
        }
        let mut samples = Vec::with_capacity(n.div_ceil(2));
        for &byte in &bytes[..n] {
            if let Some(first) = low.take() {
                samples.push(i16::from_le_bytes([first, byte]));
            } else {
                low = Some(byte);
            }
        }
        count += samples.len();
        if !samples.is_empty() {
            adapter.push(&samples)?;
        }
    }
    if low.is_some() {
        bail!("truncated PCM16 sample at end of stream");
    }
    if count == 0 {
        bail!("the audio stream was empty");
    }
    adapter.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    struct Mock(Rc<RefCell<Vec<i16>>>);
    impl Adapter for Mock {
        fn push(&mut self, samples: &[i16]) -> Result<()> {
            self.0.borrow_mut().extend(samples);
            Ok(())
        }
        fn finish(self: Box<Self>) -> Result<String> {
            Ok("done".into())
        }
    }
    struct OddReader(std::io::Cursor<Vec<u8>>);
    impl Read for OddReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(&mut buf[..3])
        }
    }
    #[test]
    fn preserves_samples_across_odd_reads_and_flushes_tail() {
        let samples = vec![i16::MIN, -1, 0, 1, i16::MAX];
        let bytes = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let received = Rc::new(RefCell::new(Vec::new()));
        assert_eq!(
            transcribe(
                OddReader(std::io::Cursor::new(bytes)),
                Box::new(Mock(received.clone()))
            )
            .unwrap(),
            "done"
        );
        assert_eq!(*received.borrow(), samples);
    }
    #[test]
    fn rejects_empty_or_truncated_audio() {
        for bytes in [vec![], vec![1, 2, 3]] {
            assert!(transcribe(bytes.as_slice(), Box::new(Mock(Rc::default()))).is_err());
        }
    }
}
