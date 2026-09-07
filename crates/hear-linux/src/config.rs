use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use global_hotkey::hotkey::HotKey;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub hotkey: HotKey,
    pub paste_automatically: bool,
    pub paste_shortcuts: BTreeMap<String, String>,
    pub hear: HearConfig,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HearConfig {
    pub engine: String,
    pub model: String,
    pub language: String,
    pub polish_model: String,
    pub context: String,
    pub polish: bool,
    pub save_recording: String,
    pub output: String,
    pub raw_output: String,
    pub force: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: "alt+x".parse().expect("default hotkey must be valid"),
            paste_automatically: true,
            paste_shortcuts: BTreeMap::from([
                ("Alacritty".to_owned(), "alt+v".to_owned()),
                ("default".to_owned(), "ctrl+v".to_owned()),
            ]),
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
            polish_model: "gpt-5.6-luna".to_owned(),
            context: "auto".to_owned(),
            polish: true,
            save_recording: String::new(),
            output: String::new(),
            raw_output: String::new(),
            force: false,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
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
        config.validate()?;
        Ok(config)
    }

    pub fn paste_shortcut_for(&self, window_class: Option<&str>) -> &str {
        if let Some(window_class) = window_class
            && let Some((_, shortcut)) = self.paste_shortcuts.iter().find(|(class, _)| {
                !class.eq_ignore_ascii_case("default") && class.eq_ignore_ascii_case(window_class)
            })
        {
            return shortcut;
        }
        self.paste_shortcuts
            .iter()
            .find(|(class, _)| class.eq_ignore_ascii_case("default"))
            .map_or("ctrl+v", |(_, shortcut)| shortcut)
    }

    fn validate(&self) -> Result<()> {
        if let Some((class, _)) = self
            .paste_shortcuts
            .iter()
            .find(|(_, shortcut)| shortcut.trim().is_empty())
        {
            bail!("paste shortcut for {class:?} cannot be empty");
        }
        self.hear.validate()?;
        Ok(())
    }
}

impl HearConfig {
    pub fn arguments(&self) -> Vec<String> {
        let mut arguments = vec!["--engine".to_owned(), self.engine.clone()];
        push_value(&mut arguments, "--model", &self.model);
        push_value(&mut arguments, "--language", &self.language);
        push_value(&mut arguments, "--polish-model", &self.polish_model);
        push_value(&mut arguments, "--context", &self.context);
        if !self.polish {
            arguments.push("--no-polish".to_owned());
        }
        push_value(&mut arguments, "--output", &self.output);
        push_value(&mut arguments, "--raw-output", &self.raw_output);
        if self.force {
            arguments.push("--force".to_owned());
        }
        arguments
    }

    pub fn save_recording_path(&self) -> Option<&Path> {
        nonempty(&self.save_recording).map(Path::new)
    }

    pub fn output_path(&self) -> Option<&Path> {
        nonempty(&self.output).map(Path::new)
    }

    fn validate(&self) -> Result<()> {
        if !matches!(self.engine.as_str(), "gpt-transcribe" | "codex" | "whisper") {
            bail!("unknown hear engine: {:?}", self.engine);
        }
        if !matches!(
            self.context.as_str(),
            "auto" | "email" | "message" | "todo" | "notes" | "plain" | "verbatim"
        ) {
            bail!("unknown hear formatting context: {:?}", self.context);
        }
        if nonempty(&self.model).is_some() && self.engine == "gpt-transcribe" {
            bail!("hear.model is only valid with the codex or whisper engine");
        }
        if nonempty(&self.language).is_some() && self.engine != "whisper" {
            bail!("hear.language is only valid with the whisper engine");
        }
        if nonempty(&self.polish_model).is_none() {
            bail!("hear.polish_model cannot be empty");
        }
        if !self.polish && nonempty(&self.raw_output).is_some() {
            bail!("hear.raw_output cannot be used when hear.polish is false");
        }
        let paths = [
            ("save_recording", nonempty(&self.save_recording)),
            ("output", nonempty(&self.output)),
            ("raw_output", nonempty(&self.raw_output)),
        ];
        for (index, (left_name, left)) in paths.iter().enumerate() {
            for (right_name, right) in paths.iter().skip(index + 1) {
                if left.is_some() && left == right {
                    bail!("hear.{left_name} and hear.{right_name} must be different paths");
                }
            }
        }
        Ok(())
    }
}

fn push_value(arguments: &mut Vec<String>, flag: &str, value: &str) {
    if let Some(value) = nonempty(value) {
        arguments.extend([flag.to_owned(), value.to_owned()]);
    }
}

fn nonempty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn config_path() -> PathBuf {
    if let Some(directory) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(directory).join("hear-app/config.toml")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config/hear-app/config.toml")
    } else {
        PathBuf::from("hear-app/config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_documented_configuration() {
        let config: Config = toml::from_str(
            r#"
hotkey = "alt+x"
paste_automatically = false

[paste_shortcuts]
default = "ctrl+shift+v"
Alacritty = "alt+v"

[hear]
engine = "whisper"
model = "large-v3"
language = "nl"
polish_model = "gpt-5.6-luna"
context = "message"
polish = true
save_recording = "/tmp/dictation.wav"
output = "/tmp/dictation.txt"
raw_output = "/tmp/dictation-raw.txt"
force = true
"#,
        )
        .unwrap();
        config.validate().unwrap();

        assert_eq!(config.hotkey.to_string(), "alt+KeyX");
        assert!(!config.paste_automatically);
        assert_eq!(config.hear.engine, "whisper");
        assert_eq!(config.hear.model, "large-v3");
        assert_eq!(config.hear.language, "nl");
        assert!(config.hear.force);
        assert_eq!(config.paste_shortcut_for(Some("alacritty")), "alt+v");
        assert_eq!(config.paste_shortcut_for(Some("Firefox")), "ctrl+shift+v");
    }

    #[test]
    fn supplies_defaults_for_an_absent_configuration() {
        let config = Config::default();

        assert_eq!(config.hotkey.to_string(), "alt+KeyX");
        assert!(config.paste_automatically);
        assert_eq!(config.hear.engine, "gpt-transcribe");
        assert!(config.hear.model.is_empty());
        assert_eq!(config.paste_shortcut_for(Some("Alacritty")), "alt+v");
        assert_eq!(config.paste_shortcut_for(None), "ctrl+v");
    }

    #[test]
    fn builds_cli_arguments() {
        let hear = HearConfig {
            engine: "whisper".to_owned(),
            language: "nl".to_owned(),
            polish: false,
            force: true,
            ..HearConfig::default()
        };

        assert_eq!(
            hear.arguments(),
            [
                "--engine",
                "whisper",
                "--language",
                "nl",
                "--polish-model",
                "gpt-5.6-luna",
                "--context",
                "auto",
                "--no-polish",
                "--force"
            ]
        );
    }
}
