use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub paste_automatically: bool,
    pub hear: HearConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HearConfig {
    pub engine: String,
    pub model: String,
    pub language: String,
    pub polish_engine: String,
    pub polish_model: String,
    pub context: String,
    pub polish: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paste_automatically: true,
            hear: HearConfig::default(),
        }
    }
}

impl Default for HearConfig {
    fn default() -> Self {
        Self {
            engine: "gpt-transcribe".to_owned(),
            model: String::new(),
            language: String::new(),
            polish_engine: "openai".to_owned(),
            polish_model: String::new(),
            context: "auto".to_owned(),
            polish: true,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        };
        let config: Self = toml::from_str(&source)
            .with_context(|| format!("could not parse {}", path.display()))?;
        config.hear.validate()?;
        Ok(config)
    }
}

impl HearConfig {
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec!["--engine".to_owned(), self.engine.clone()];
        push_value(&mut arguments, "--model", &self.model);
        push_value(&mut arguments, "--language", &self.language);
        if self.polish {
            push_value(&mut arguments, "--polish-engine", &self.polish_engine);
            push_value(&mut arguments, "--polish-model", &self.polish_model);
            push_value(&mut arguments, "--context", &self.context);
        } else {
            arguments.push("--no-polish".to_owned());
        }
        arguments
    }

    pub fn requires_openai(&self) -> bool {
        self.engine == "gpt-transcribe" || (self.polish && self.polish_engine == "openai")
    }

    fn validate(&self) -> Result<()> {
        if !matches!(self.engine.as_str(), "gpt-transcribe" | "codex" | "whisper") {
            bail!("unknown hear engine: {:?}", self.engine);
        }
        if !matches!(self.polish_engine.as_str(), "openai" | "local") {
            bail!("unknown hear polishing engine: {:?}", self.polish_engine);
        }
        if !matches!(
            self.context.as_str(),
            "auto" | "email" | "message" | "todo" | "notes" | "plain" | "verbatim"
        ) {
            bail!("unknown hear formatting context: {:?}", self.context);
        }
        if !self.model.trim().is_empty() && self.engine == "gpt-transcribe" {
            bail!("hear.model is only valid with the codex or whisper engine");
        }
        if !self.language.trim().is_empty() && self.engine != "whisper" {
            bail!("hear.language is only valid with the whisper engine");
        }
        Ok(())
    }
}

fn push_value(arguments: &mut Vec<String>, flag: &str, value: &str) {
    if !value.trim().is_empty() {
        arguments.extend([flag.to_owned(), value.to_owned()]);
    }
}

fn config_path() -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine the user configuration directory")?;
    Ok(base.config_dir().join("hear-app/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_model_configuration() {
        let config: Config = toml::from_str(
            r#"
paste_automatically = false

[hear]
engine = "whisper"
model = "small.en"
language = "en"
polish_engine = "local"
polish_model = "qwen3.5-0.8b"
context = "notes"
polish = true
"#,
        )
        .unwrap();
        config.hear.validate().unwrap();
        assert!(!config.paste_automatically);
        assert_eq!(config.hear.model, "small.en");
        assert_eq!(config.hear.polish_model, "qwen3.5-0.8b");
        assert!(!config.hear.requires_openai());
    }

    #[test]
    fn builds_helper_arguments() {
        let hear = HearConfig {
            engine: "whisper".to_owned(),
            model: "tiny.en".to_owned(),
            polish_engine: "local".to_owned(),
            polish_model: "qwen3.5-0.8b".to_owned(),
            ..HearConfig::default()
        };
        assert_eq!(
            hear.arguments(),
            [
                "--engine",
                "whisper",
                "--model",
                "tiny.en",
                "--polish-engine",
                "local",
                "--polish-model",
                "qwen3.5-0.8b",
                "--context",
                "auto",
            ]
        );
    }

    #[test]
    fn no_polish_omits_polishing_options() {
        let hear = HearConfig {
            engine: "whisper".to_owned(),
            polish: false,
            ..HearConfig::default()
        };
        assert_eq!(hear.arguments(), ["--engine", "whisper", "--no-polish"]);
        assert!(!hear.requires_openai());
    }
}
