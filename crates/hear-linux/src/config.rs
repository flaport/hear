use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use global_hotkey::hotkey::HotKey;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub hotkey: HotKey,
    pub paste_automatically: bool,
    pub paste_shortcuts: BTreeMap<String, String>,
    pub hear_options: Vec<String>,
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
            hear_options: Vec::new(),
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
        for option in &self.hear_options {
            let name = option
                .split_once('=')
                .map_or(option.as_str(), |(name, _)| name);
            if matches!(
                name,
                "--record" | "--save-recording" | "--output" | "-o" | "--raw-output"
            ) || name.starts_with("-o")
            {
                bail!("{name} is managed by hear-app and cannot be set in hear_options");
            }
        }
        Ok(())
    }
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
hear_options = ["--engine", "whisper", "--language", "nl"]

[paste_shortcuts]
default = "ctrl+shift+v"
Alacritty = "alt+v"
"#,
        )
        .unwrap();
        config.validate().unwrap();

        assert_eq!(config.hotkey.to_string(), "alt+KeyX");
        assert!(!config.paste_automatically);
        assert_eq!(
            config.hear_options,
            ["--engine", "whisper", "--language", "nl"]
        );
        assert_eq!(config.paste_shortcut_for(Some("alacritty")), "alt+v");
        assert_eq!(config.paste_shortcut_for(Some("Firefox")), "ctrl+shift+v");
    }

    #[test]
    fn supplies_defaults_for_an_absent_configuration() {
        let config = Config::default();

        assert_eq!(config.hotkey.to_string(), "alt+KeyX");
        assert!(config.paste_automatically);
        assert!(config.hear_options.is_empty());
        assert_eq!(config.paste_shortcut_for(Some("Alacritty")), "alt+v");
        assert_eq!(config.paste_shortcut_for(None), "ctrl+v");
    }

    #[test]
    fn rejects_cli_options_owned_by_the_app() {
        for option in ["--record", "--output=result.txt", "-o", "-oresult.txt"] {
            let config = Config {
                hear_options: vec![option.to_owned()],
                ..Config::default()
            };
            assert!(config.validate().is_err(), "accepted {option}");
        }
    }
}
