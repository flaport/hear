use super::Adapter;
use anyhow::{Context, Result};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

const RATE: usize = 16000;
const WINDOW: usize = 12 * RATE;
const OVERLAP: usize = 3 * RATE;

pub(super) struct Whisper {
    state: WhisperState,
    language: Option<String>,
    prompt: String,
    audio: Vec<f32>,
    text: String,
    fresh_samples: usize,
}
impl Whisper {
    pub fn new(model_name: &str, language: &str, vocabulary: &[String]) -> Result<Self> {
        use crate::engines::whisper::{model, resolve_language};
        let model = model::find(model_name)?;
        let language = resolve_language(model, language)?.map(str::to_owned);
        let path = model::ensure(model)?;
        let context = WhisperContext::new_with_params(
            path.to_str().context("invalid model path")?,
            WhisperContextParameters::default(),
        )
        .context("could not load the whisper.cpp model")?;
        let state = context.create_state()?;
        Ok(Self {
            state,
            language,
            prompt: if vocabulary.is_empty() {
                String::new()
            } else {
                format!("Preferred spellings: {}.", vocabulary.join(", "))
            },
            audio: Vec::new(),
            text: String::new(),
            fresh_samples: 0,
        })
    }

    fn decode(&mut self, final_chunk: bool) -> Result<()> {
        if self.fresh_samples == 0 {
            return Ok(());
        }
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(self.language.as_deref());
        params.set_translate(false);
        params.set_no_context(true);
        params.set_n_threads(
            std::thread::available_parallelism().map_or(4, |n| n.get().min(8) as i32),
        );
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if !self.prompt.is_empty() {
            params.set_initial_prompt(&self.prompt);
        }
        // Whisper needs at least a short input; only pad the final inference copy.
        let mut samples = self.audio.clone();
        samples.resize(samples.len().max(RATE), 0.0);
        self.state.full(params, &samples)?;
        let text = self
            .state
            .as_iter()
            .map(|s| s.to_string())
            .collect::<String>();
        if self.text.is_empty() {
            self.text.push_str(text.trim());
        } else {
            append_overlap(&mut self.text, &text);
        }
        // Whisper timestamps are too approximate for cutting a word boundary.
        // Re-decode three seconds of real audio and reconcile the overlapping text.
        let cut = if final_chunk {
            self.audio.len()
        } else {
            self.audio.len().saturating_sub(OVERLAP)
        };
        self.audio.drain(..cut);
        self.fresh_samples = 0;
        Ok(())
    }
}

fn append_overlap(text: &mut String, next: &str) {
    let previous: Vec<_> = text.split_whitespace().collect();
    let incoming: Vec<_> = next.split_whitespace().collect();
    let normalize = |word: &str| {
        word.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let overlap = (1..=previous.len().min(incoming.len()).min(40))
        .rev()
        .find(|&count| {
            previous[previous.len() - count..]
                .iter()
                .zip(&incoming[..count])
                .all(|(a, b)| normalize(a) == normalize(b))
        })
        .unwrap_or(0);
    if overlap < incoming.len() {
        if !text.is_empty() && !text.ends_with(char::is_whitespace) {
            text.push(' ');
        }
        text.push_str(&incoming[overlap..].join(" "));
    }
}

impl Adapter for Whisper {
    fn push(&mut self, samples: &[i16]) -> Result<()> {
        self.audio
            .extend(samples.iter().map(|s| *s as f32 / 32768.0));
        self.fresh_samples += samples.len();
        if self.audio.len() >= WINDOW {
            self.decode(false)?;
        }
        Ok(())
    }
    fn finish(mut self: Box<Self>) -> Result<String> {
        self.decode(true)?;
        Ok(self.text.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlapping_audio_recovers_boundary_words_without_removing_repetition() {
        let mut text = "This is a streaming transcription".to_owned();
        append_overlap(&mut text, "streaming transcription test. We are very very");
        append_overlap(&mut text, "are very very happy.");
        assert_eq!(
            text,
            "This is a streaming transcription test. We are very very happy."
        );
        let mut text = "hello WORLD.".to_owned();
        append_overlap(&mut text, "Hello world again.");
        assert_eq!(text, "hello WORLD. again.");
    }
}
