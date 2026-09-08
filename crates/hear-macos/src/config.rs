pub use hear_core::HearConfig;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub paste_automatically: bool,
    pub hear: HearConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paste_automatically: true,
            hear: HearConfig::default(),
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

fn config_path() -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine the user configuration directory")?;
    Ok(base.config_dir().join("hear-app/config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_shared_options_and_paste_preference() {
        let c: Config = toml::from_str(
            "paste_automatically=false\n[hear]\nmodel='tiny.en'\npolish_model='qwen3.5-0.8b'",
        )
        .unwrap();
        c.hear.validate().unwrap();
        assert!(!c.paste_automatically);
        assert!(!c.hear.requires_openai());
    }
}
