pub use hear_core::HearConfig;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

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
    fn parses_shared_options_and_platform_shortcuts() {
        let c:Config=toml::from_str("hotkey='alt+x'\n[paste_shortcuts]\ndefault='ctrl+shift+v'\nAlacritty='alt+v'\n[hear]\nmodel='large-v3-turbo'\nlanguage='nl'\npolish=false").unwrap();
        c.validate().unwrap();
        assert_eq!(c.paste_shortcut_for(Some("alacritty")), "alt+v");
        assert_eq!(c.paste_shortcut_for(None), "ctrl+shift+v");
        assert!(!c.hear.requires_openai());
    }
}
