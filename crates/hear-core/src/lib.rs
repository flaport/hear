//! Shared policy and infrastructure; contains no transcription model runtime.
#[cfg(feature = "capture")]
pub mod capture;
mod config;
pub mod files;
pub mod helper;
pub mod process;
pub use config::{
    Engine, FormatContext, HearConfig, PolishEngine, is_local_polish_model, is_whisper_model,
};
