//! Native CPU inference with the September 2026 Pocket checkpoint. No Python.
//! Inference and voice conditioning are retained per engine instance. Cancellation
//! is checked between frames (80 ms of audio), chunks, and asset reads. Model load
//! and an individual tensor operation cannot be interrupted; asset HTTP requests
//! bound each network wait to 10 seconds. Tensor work uses a private two-thread CPU pool; sinks and observers run
//! synchronously on the calling thread.
mod assets;
use super::*;
use ptts::{
    flow_lm::{NormalRng, StepInput},
    plan::{EosPolicy, frame_budget},
    transformer::LayerAttentionState,
    tts_model::{TTSConfig, TTSModel, TTSState},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, TryLockError},
    time::Instant,
};
use xn::{CpuDevice, Tensor, Unquantized, nn::VB};

type Cpu = Unquantized<f32, CpuDevice>;
pub const POCKET_MODEL: &str = "english_2026-09";
pub const POCKET_VOICE: &str = "alba";
pub const POCKET_VOICES: &[&str] = &[
    "alba", "marius", "javert", "jean", "fantine", "cosette", "eponine", "azelma",
];

/// A reusable native Pocket TTS engine. Assets are downloaded lazily into a
/// verified cache; warm calls reuse the model and voice state. Concurrent calls
/// on one instance serialize. Use `SpeechRequest::pocket` for local defaults.
/// Cancellation is checked between frames and reads; individual network waits
/// are bounded to 10 seconds. Loading and individual tensor operations cannot
/// be interrupted. Callbacks run on the caller; inference uses two CPU threads.
pub struct PocketSpeech {
    cache: PathBuf,
    pool: rayon::ThreadPool,
    runtime: Mutex<Option<Runtime>>,
    observer: Option<Arc<dyn Fn(SpeechEvent) + Send + Sync>>,
}
impl PocketSpeech {
    pub fn new() -> Result<Self> {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| local(anyhow::anyhow!("could not determine cache directory")))?;
        Self::with_cache(base.cache_dir().join("hear/pocket").join(POCKET_MODEL))
    }
    pub fn with_cache(path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            cache: path.into(),
            pool: rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .map_err(local)?,
            runtime: Mutex::new(None),
            observer: None,
        })
    }
    pub fn progress(mut self, observer: impl Fn(SpeechEvent) + Send + Sync + 'static) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }
    fn report(&self, event: SpeechEvent) {
        if let Some(observer) = &self.observer {
            observer(event);
        }
    }
    /// Reject unsupported options before downloading weights or loading the model.
    pub fn validate(request: &SpeechRequest<'_>) -> Result<()> {
        let invalid = |s: &str| SpeechError::Configuration(s.into());
        if request.text.trim().is_empty() || request.text.chars().count() > 4096 {
            return Err(invalid("text must contain 1–4096 characters"));
        }
        if request.model != POCKET_MODEL {
            return Err(invalid("Pocket supports model english_2026-09"));
        }
        if !POCKET_VOICES.contains(&request.voice) {
            return Err(invalid(
                "unknown Pocket voice; choose alba, marius, javert, jean, fantine, cosette, eponine, or azelma",
            ));
        }
        if request.speed != 1.0 {
            return Err(invalid("Pocket currently supports only speed 1.0"));
        }
        if request.instructions.is_some() {
            return Err(invalid("Pocket does not support delivery instructions"));
        }
        Ok(())
    }
    fn generate(
        &self,
        request: &SpeechRequest<'_>,
        sink: &mut dyn SpeechAudioSink,
        cancellation: &Cancellation,
    ) -> Result<SpeechSummary> {
        Self::validate(request)?;
        let start = Instant::now();
        let mut guard = loop {
            check_cancelled(cancellation)?;
            match self.runtime.try_lock() {
                Ok(guard) => break guard,
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(20)),
                Err(TryLockError::Poisoned(_)) => {
                    return Err(local(anyhow::anyhow!("Pocket worker state is poisoned")));
                }
            }
        };
        if guard.is_none() {
            let weights = assets::ensure(&self.cache, &assets::MODEL, cancellation)?;
            let tokenizer = assets::ensure(&self.cache, &assets::TOKENIZER, cancellation)?;
            *guard = Some(
                self.pool
                    .install(|| Runtime::load(&weights, &tokenizer))
                    .map_err(local)?,
            );
        }
        check_cancelled(cancellation)?;
        let runtime = guard.as_mut().expect("loaded runtime");
        if !runtime.voices.contains_key(request.voice) {
            let path = assets::ensure(&self.cache, assets::voice(request.voice), cancellation)?;
            let voice = self
                .pool
                .install(|| load_voice(&runtime.model, &path))
                .map_err(local)?;
            runtime.voices.insert(request.voice.into(), voice);
        }
        check_cancelled(cancellation)?;
        // Keep the caller's sink on its calling thread. Only tensor work is
        // dispatched to the private pool; each frame is consumed before the next.
        let tokenizer = runtime
            .model
            .flow_lm
            .conditioner
            .tokenizer
            .as_ref()
            .expect("loaded tokenizer");
        let chunks =
            ptts::tts_model::split_into_best_sentences(tokenizer.as_ref(), request.text, None)
                .map_err(local)?;
        let mut rng = NormalRng::new(
            0.3,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
        )
        .map_err(local)?;
        let mut total = 0;
        let mut first = None;
        for chunk in chunks {
            check_cancelled(cancellation)?;
            let (text, tail) = ptts::tts_model::prepare_text_prompt(&chunk);
            let tokens = runtime
                .model
                .flow_lm
                .conditioner
                .tokenize(&text)
                .map_err(local)?;
            // Bound pathological unpunctuated requests before allocating KV state.
            if tokens.len() > 512 {
                return Err(SpeechError::Configuration(
                    "Pocket sentences must contain at most 512 tokens; add sentence breaks".into(),
                ));
            }
            let frames = frame_budget(tokens.len(), 12.5);
            let base = &runtime.voices[request.voice];
            let budget = voice_length(base).map_err(local)? + tokens.len() + frames;
            let mut state = TTSState {
                flow_lm_state: ptts::flow_lm::FlowLMState {
                    transformer_state: base
                        .flow_lm_state
                        .transformer_state
                        .with_seq_budget(budget)
                        .map_err(local)?,
                },
            };
            self.pool
                .install(|| runtime.model.prompt_text(&mut state, &tokens))
                .map_err(local)?;
            let mut mimi = runtime.model.init_mimi_state(1).map_err(local)?;
            let mut prev = None;
            let mut eos = EosPolicy::new(tail);
            let mut finished = false;
            for _ in 0..frames {
                check_cancelled(cancellation)?;
                let (next, done, pcm) = self
                    .pool
                    .install(|| -> xn::Result<_> {
                        let input = match &prev {
                            None => StepInput::Bos { batch: 1 },
                            Some(p) => StepInput::Latent(p),
                        };
                        let (next, done) =
                            runtime.model.generate_step(&mut state, input, &mut rng)?;
                        let audio = runtime.model.decode_latent(&next, &mut mimi)?.to_vec()?;
                        Ok((next, done, audio))
                    })
                    .map_err(local)?;
                check_cancelled(cancellation)?;
                if !pcm.is_empty() {
                    if first.is_none() {
                        let time = start.elapsed();
                        first = Some(time);
                        self.report(SpeechEvent::FirstAudio(time));
                    }
                    check_cancelled(cancellation)?;
                    let samples = pcm16(&pcm)?;
                    sink.write(&samples)?;
                    total += samples.len() as u64;
                }
                if eos.should_stop(done) {
                    finished = true;
                    break;
                }
                prev = Some(next);
            }
            if !finished {
                return Err(local(anyhow::anyhow!(
                    "Pocket exceeded its audio frame budget before completing the sentence"
                )));
            }
        }
        check_cancelled(cancellation)?;
        Ok(SpeechSummary {
            engine: "pocket".into(),
            model: POCKET_MODEL.into(),
            voice: request.voice.into(),
            samples: total,
            sample_rate: SAMPLE_RATE,
            audio_duration: Duration::from_secs_f64(total as f64 / SAMPLE_RATE as f64),
            first_audio: first
                .ok_or_else(|| SpeechError::Decode("Pocket produced no audio".into()))?,
            request_duration: start.elapsed(),
        })
    }
}
impl SpeechEngine for PocketSpeech {
    fn synthesize(
        &self,
        request: &SpeechRequest<'_>,
        sink: &mut dyn SpeechAudioSink,
        cancellation: &Cancellation,
    ) -> Result<SpeechSummary> {
        self.report(SpeechEvent::Started);
        let result = self.generate(request, sink, cancellation);
        self.report(match &result {
            Ok(s) => SpeechEvent::Completed(s.clone()),
            Err(SpeechError::Cancelled) => SpeechEvent::Cancelled,
            Err(e) => SpeechEvent::Failed(e.to_string()),
        });
        result
    }
}
impl<'a> SpeechRequest<'a> {
    pub fn pocket(text: &'a str) -> Self {
        Self {
            text,
            voice: POCKET_VOICE,
            model: POCKET_MODEL,
            speed: 1.0,
            instructions: None,
        }
    }
}
struct Runtime {
    model: TTSModel<Cpu>,
    voices: HashMap<String, TTSState<Cpu>>,
}
impl Runtime {
    fn load(weights: &Path, tokenizer: &Path) -> anyhow::Result<Self> {
        // Preconditioned September voice caches already contain the learned BOS
        // and speaker conditioning. Neither voice encoder nor speaker projection
        // is needed; the latter has changed shape since the January checkpoint.
        let vb = VB::load_with_key_map(&[weights], CpuDevice, |key| {
            let key = ptts::loader::remap_key(key)?;
            (key != "flow_lm.speaker_proj_weight").then_some(key)
        })?
        .root();
        // The September checkpoint retains this six-layer January architecture.
        // Its newer BOS/voice conditioning is supplied by the cached KV state.
        let model = TTSModel::<Cpu>::load(
            &vb,
            Box::new(ptts::tok::Tok::open(tokenizer)?),
            &TTSConfig::v202601(0.3),
        )?;
        anyhow::ensure!(
            model.sample_rate() == SAMPLE_RATE as usize,
            "unexpected Pocket sample rate"
        );
        Ok(Self {
            model,
            voices: HashMap::new(),
        })
    }
}
fn local(error: impl Into<anyhow::Error>) -> SpeechError {
    SpeechError::Local(error.into())
}
fn pcm16(audio: &[f32]) -> Result<Vec<i16>> {
    if audio.iter().any(|s| !s.is_finite()) {
        return Err(SpeechError::Decode(
            "Pocket produced non-finite samples".into(),
        ));
    }
    Ok(audio
        .iter()
        .map(|s| {
            (s.clamp(-1.0, 1.0) * 32768.0)
                .round()
                .clamp(-32768.0, 32767.0) as i16
        })
        .collect())
}
fn voice_length(state: &TTSState<Cpu>) -> anyhow::Result<usize> {
    match state.flow_lm_state.transformer_state.layer_states.first() {
        Some(LayerAttentionState::FlowLm(s)) => Ok(s.current_end),
        _ => anyhow::bail!("invalid Pocket voice state"),
    }
}
fn load_voice(model: &TTSModel<Cpu>, path: &Path) -> anyhow::Result<TTSState<Cpu>> {
    let bytes = std::fs::read(path)?;
    let tensors = safetensors::SafeTensors::deserialize(&bytes)?;
    let mut positions = Vec::new();
    for i in 0..6 {
        positions.push(voice_position(&tensors, i)?);
    }
    anyhow::ensure!(
        positions.iter().all(|p| *p == positions[0]),
        "voice layers have different offsets"
    );
    let offset = positions[0];
    let voice = VB::load(&[path], CpuDevice)?;
    let mut state = model.init_flow_lm_state(1, offset)?;
    for (i, layer) in state
        .flow_lm_state
        .transformer_state
        .layer_states
        .iter_mut()
        .enumerate()
    {
        let LayerAttentionState::FlowLm(layer) = layer else {
            anyhow::bail!("invalid Pocket attention type")
        };
        let cache: Tensor<f32, CpuDevice> = voice.tensor(
            &format!("transformer.layers.{i}.self_attn/cache"),
            (2, 1, offset, 16, 64),
        )?;
        layer.k_cache.slice_set(
            &cache
                .narrow(0, 0..1)?
                .reshape((1, offset, 16, 64))?
                .contiguous()?,
            1,
            0,
        )?;
        layer.v_cache.slice_set(
            &cache
                .narrow(0, 1..2)?
                .reshape((1, offset, 16, 64))?
                .contiguous()?,
            1,
            0,
        )?;
        layer.current_end = offset;
    }
    Ok(state)
}
fn voice_position(tensors: &safetensors::SafeTensors<'_>, layer: usize) -> anyhow::Result<usize> {
    use safetensors::Dtype;
    let prefix = format!("transformer.layers.{layer}.self_attn/");
    let position = tensors.tensor(&format!("{prefix}offset"))?;
    anyhow::ensure!(
        position.dtype() == Dtype::I64 && position.shape() == [1],
        "invalid voice offset"
    );
    let offset = i64::from_le_bytes(position.data().try_into()?);
    anyhow::ensure!((1..=512).contains(&offset), "voice offset out of range");
    let cache = tensors.tensor(&format!("{prefix}cache"))?;
    anyhow::ensure!(
        cache.dtype() == Dtype::F32 && cache.shape() == [2, 1, offset as usize, 16, 64],
        "invalid voice cache dimensions"
    );
    Ok(offset as usize)
}

#[cfg(test)]
mod tests;
