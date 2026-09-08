use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize};
use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $text:literal $(| $alias:literal)*),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
        pub enum $name {
            $(#[serde(rename = $text $(, alias = $alias)*)]
              #[cfg_attr(feature = "cli", value(name = $text $(, alias = $alias)*))]
              $variant),+
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self { $(Self::$variant => $text),+ })
            }
        }
        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> std::result::Result<Self, String> {
                match s { $($text $(| $alias)* => Ok(Self::$variant)),+, _ => Err(format!("unknown {}: {s:?}", stringify!($name))) }
            }
        }
    };
}
string_enum!(Engine { GptTranscribe => "gpt-transcribe" | "1", Codex => "codex" | "2", Whisper => "whisper" | "3" });
string_enum!(PolishEngine { Openai => "openai", Local => "local" });
string_enum!(FormatContext { Auto => "auto", Email => "email", Message => "message" | "text", Todo => "todo" | "tasks", Notes => "notes" | "note", Plain => "plain", Verbatim => "verbatim" });

/// Shared CLI/app configuration. Legacy empty TOML strings deserialize as `None`.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HearConfig {
    #[serde(deserialize_with = "optional")]
    pub engine: Option<Engine>,
    #[serde(deserialize_with = "optional")]
    pub model: Option<String>,
    #[serde(deserialize_with = "optional")]
    pub language: Option<String>,
    #[serde(deserialize_with = "optional")]
    pub polish_engine: Option<PolishEngine>,
    #[serde(deserialize_with = "optional")]
    pub polish_model: Option<String>,
    pub context: FormatContext,
    pub polish: bool,
    #[serde(deserialize_with = "optional")]
    pub save_recording: Option<PathBuf>,
    #[serde(deserialize_with = "optional")]
    pub output: Option<PathBuf>,
    #[serde(deserialize_with = "optional")]
    pub raw_output: Option<PathBuf>,
    pub force: bool,
}
impl Default for HearConfig {
    fn default() -> Self {
        Self {
            engine: None,
            model: None,
            language: None,
            polish_engine: None,
            polish_model: None,
            context: FormatContext::Auto,
            polish: true,
            save_recording: None,
            output: None,
            raw_output: None,
            force: false,
        }
    }
}
fn optional<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    let text = String::deserialize(d)?;
    let text = text.trim();
    if text.is_empty() {
        Ok(None)
    } else {
        text.parse().map(Some).map_err(serde::de::Error::custom)
    }
}
impl HearConfig {
    pub fn resolved_engine(&self) -> Engine {
        self.engine.unwrap_or_else(|| {
            if self.language.is_some() || self.model.as_deref().is_some_and(is_whisper_model) {
                Engine::Whisper
            } else if self.model.is_some() {
                Engine::Codex
            } else {
                Engine::GptTranscribe
            }
        })
    }
    pub fn resolved_polish_engine(&self) -> PolishEngine {
        self.polish_engine.unwrap_or_else(|| {
            if self
                .polish_model
                .as_deref()
                .is_some_and(is_local_polish_model)
            {
                PolishEngine::Local
            } else {
                PolishEngine::Openai
            }
        })
    }
    pub fn requires_openai(&self) -> bool {
        self.resolved_engine() == Engine::GptTranscribe
            || (self.polish
                && self.context != FormatContext::Verbatim
                && self.resolved_polish_engine() == PolishEngine::Openai)
    }
    pub fn save_recording_path(&self) -> Option<&Path> {
        self.save_recording.as_deref()
    }
    pub fn output_path(&self) -> Option<&Path> {
        self.output.as_deref()
    }
    pub fn validate(&self) -> Result<()> {
        if self.model.is_some() && self.resolved_engine() == Engine::GptTranscribe {
            bail!("--model is only valid with codex or whisper");
        }
        if self.engine.is_none()
            && self.language.is_none()
            && self.model.as_deref().is_some_and(|m| !is_whisper_model(m))
        {
            bail!(
                "unrecognized transcription model; specify --engine codex explicitly for a Codex model"
            );
        }
        if self.language.is_some() && self.resolved_engine() != Engine::Whisper {
            bail!("--language is only valid with whisper");
        }
        if self.resolved_engine() == Engine::Whisper {
            let model = self.model.as_deref().unwrap_or("tiny.en");
            if !is_whisper_model(model) {
                bail!("unknown Whisper model: {model}");
            }
            let language = self.language.as_deref().unwrap_or("en");
            if model.ends_with(".en")
                && !language.eq_ignore_ascii_case("en")
                && !language.eq_ignore_ascii_case("auto")
            {
                bail!(
                    "Whisper model {model} only supports English; choose large-v3-turbo for --language {language}"
                );
            }
        }
        if self.polish
            && self.context != FormatContext::Verbatim
            && self.resolved_polish_engine() == PolishEngine::Local
            && let Some(model) = self.polish_model.as_deref()
            && !matches!(
                model,
                "qwen3.5-2b" | "qwen3.5-2b-q4_k_m" | "qwen3.5-0.8b" | "qwen3.5-0.8b-q4_k_m"
            )
            && !Path::new(model).is_file()
        {
            bail!("unknown local polishing model or missing GGUF file: {model}");
        }
        if !self.polish && self.raw_output.is_some() {
            bail!("--raw-output cannot be used with --no-polish");
        }
        crate::files::ensure_distinct(&[
            self.save_recording.as_deref(),
            self.output.as_deref(),
            self.raw_output.as_deref(),
        ])
    }
    /// Check destinations before accepting microphone input.
    pub fn preflight(&self) -> Result<()> {
        self.validate()?;
        for p in [&self.save_recording, &self.output, &self.raw_output]
            .into_iter()
            .flatten()
        {
            crate::files::preflight(p, self.force)?;
        }
        Ok(())
    }
    pub fn arguments(&self) -> Vec<std::ffi::OsString> {
        let mut args = Vec::new();
        macro_rules! arg {
            ($flag:literal, $value:expr) => {
                if let Some(value) = $value {
                    args.push($flag.into());
                    args.push(value.into());
                }
            };
        }
        arg!("--engine", self.engine.map(|v| v.to_string()));
        arg!("--model", self.model.as_deref());
        arg!("--language", self.language.as_deref());
        if self.polish {
            arg!("--polish-engine", self.polish_engine.map(|v| v.to_string()));
            arg!("--polish-model", self.polish_model.as_deref());
            args.extend(["--context".into(), self.context.to_string().into()]);
        } else {
            args.push("--no-polish".into());
        }
        arg!("--output", self.output.as_deref());
        arg!("--raw-output", self.raw_output.as_deref());
        if self.force {
            args.push("--force".into());
        }
        args
    }
}
pub fn is_whisper_model(m: &str) -> bool {
    matches!(
        m,
        "tiny.en" | "base.en" | "small.en" | "medium.en" | "large-v3-turbo"
    )
}
pub fn is_local_polish_model(m: &str) -> bool {
    matches!(
        m,
        "qwen3.5-2b" | "qwen3.5-2b-q4_k_m" | "qwen3.5-0.8b" | "qwen3.5-0.8b-q4_k_m"
    ) || Path::new(m)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_config_and_verbatim_credentials() {
        let c: HearConfig = toml::from_str(
            "engine = 'whisper'\nmodel = ' tiny.en '\npolish_engine = ''\ncontext = 'verbatim'",
        )
        .unwrap();
        c.validate().unwrap();
        assert!(!c.requires_openai());
        assert_eq!(c.model.as_deref(), Some("tiny.en"));
    }
    #[test]
    fn reject_invalid_models_before_recording() {
        let c: HearConfig = toml::from_str("engine = 'whisper'\nmodel = 'large-v3'").unwrap();
        assert!(c.validate().is_err());
        let c: HearConfig = toml::from_str("model = 'tniy.en'").unwrap();
        assert!(c.validate().is_err());
    }
}
