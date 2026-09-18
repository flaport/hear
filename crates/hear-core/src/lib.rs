//! Shared policy and infrastructure; contains no transcription model runtime.
pub mod audio_stream;
#[cfg(feature = "capture")]
pub mod capture;
mod config;
#[cfg(feature = "capture")]
pub mod dictation;
pub mod files;
pub mod helper;
pub mod process;
pub use config::{
    Engine, FormatContext, HearConfig, PolishEngine, is_local_polish_model, is_whisper_model,
};
