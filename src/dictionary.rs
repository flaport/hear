use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use directories::BaseDirs;
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

use crate::cli::DictionaryCommand;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Dictionary {
    #[serde(default)]
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Entry {
    term: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sounds_like: Option<String>,
}

pub fn run(command: &DictionaryCommand) -> Result<()> {
    match command {
        DictionaryCommand::Add {
            term,
            aliases,
            sounds_like,
        } => {
            let mut dictionary = Dictionary::load()?;
            let updated = dictionary.add(term, aliases, sounds_like.as_deref())?;
            dictionary.save()?;
            println!(
                "{} dictionary entry: {}",
                if updated { "Updated" } else { "Added" },
                term.trim()
            );
        }
        DictionaryCommand::List => Dictionary::load()?.print(),
        DictionaryCommand::Remove { term } => {
            let mut dictionary = Dictionary::load()?;
            if !dictionary.remove(term) {
                bail!("dictionary entry not found: {}", term.trim());
            }
            dictionary.save()?;
            println!("Removed dictionary entry: {}", term.trim());
        }
    }
    Ok(())
}

impl Dictionary {
    pub fn load() -> Result<Self> {
        let path = dictionary_path()?;
        Self::load_from(&path)
    }

    fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(path)
            .with_context(|| format!("could not read dictionary: {}", path.display()))?;
        let dictionary: Self = serde_json::from_str(&contents)
            .with_context(|| format!("dictionary contains invalid JSON: {}", path.display()))?;
        dictionary.validate()?;
        Ok(dictionary)
    }

    fn save(&self) -> Result<()> {
        let path = dictionary_path()?;
        self.save_to(&path)
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let directory = path
            .parent()
            .context("dictionary path has no parent directory")?;
        fs::create_dir_all(directory).with_context(|| {
            format!(
                "could not create dictionary directory: {}",
                directory.display()
            )
        })?;
        let mut temporary = tempfile::NamedTempFile::new_in(directory)
            .context("could not create temporary dictionary file")?;
        serde_json::to_writer_pretty(&mut temporary, self)
            .context("could not serialize dictionary")?;
        writeln!(temporary).context("could not finish dictionary file")?;
        temporary
            .flush()
            .context("could not flush dictionary file")?;
        temporary
            .persist(path)
            .map_err(|error| error.error)
            .with_context(|| format!("could not save dictionary: {}", path.display()))?;
        Ok(())
    }

    pub fn canonical_terms(&self) -> Vec<String> {
        self.entries
            .iter()
            .map(|entry| entry.term.clone())
            .collect()
    }

    pub fn formatter_context(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let entries = self
            .entries
            .iter()
            .map(|entry| {
                let mut description = entry.term.clone();
                if !entry.aliases.is_empty() {
                    description.push_str(&format!("; aliases: {}", entry.aliases.join(", ")));
                }
                if let Some(sounds_like) = &entry.sounds_like {
                    description.push_str(&format!("; pronounced: {sounds_like}"));
                }
                format!("- {description}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(entries)
    }

    pub fn correct_aliases(&self, transcript: &str) -> Result<String> {
        let mut aliases = self
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .aliases
                    .iter()
                    .map(move |alias| (alias.as_str(), entry.term.as_str()))
            })
            .collect::<Vec<_>>();
        aliases.sort_by_key(|(alias, _)| std::cmp::Reverse(alias.chars().count()));

        let mut corrected = transcript.to_owned();
        for (alias, term) in aliases {
            let starts_with_word = alias.chars().next().is_some_and(is_word_character);
            let ends_with_word = alias.chars().next_back().is_some_and(is_word_character);
            let pattern = format!(
                "(?i){}{}{}",
                if starts_with_word { r"\b" } else { "" },
                regex::escape(alias),
                if ends_with_word { r"\b" } else { "" }
            );
            let regex = Regex::new(&pattern).context("could not compile a dictionary alias")?;
            corrected = regex
                .replace_all(&corrected, |_captures: &Captures<'_>| term)
                .into_owned();
        }
        Ok(corrected)
    }

    fn add(&mut self, term: &str, aliases: &[String], sounds_like: Option<&str>) -> Result<bool> {
        let mut candidate = self.clone();
        let updated = candidate.add_validated(term, aliases, sounds_like)?;
        candidate.validate()?;
        candidate
            .entries
            .sort_by_key(|entry| entry.term.to_lowercase());
        *self = candidate;
        Ok(updated)
    }

    fn add_validated(
        &mut self,
        term: &str,
        aliases: &[String],
        sounds_like: Option<&str>,
    ) -> Result<bool> {
        let term = clean_value(term, "term")?;
        let aliases = aliases
            .iter()
            .map(|alias| clean_value(alias, "alias"))
            .collect::<Result<Vec<_>>>()?;
        let sounds_like = sounds_like
            .map(|value| clean_value(value, "pronunciation"))
            .transpose()?;

        let existing_index = self
            .entries
            .iter()
            .position(|entry| same_value(&entry.term, &term));
        let mut entry = existing_index
            .map(|index| self.entries.remove(index))
            .unwrap_or_else(|| Entry {
                term: term.clone(),
                aliases: Vec::new(),
                sounds_like: None,
            });
        entry.term = term;
        for alias in aliases {
            if same_value(&alias, &entry.term) {
                bail!("dictionary alias must differ from its canonical term: {alias}");
            }
            if !entry
                .aliases
                .iter()
                .any(|existing| same_value(existing, &alias))
            {
                entry.aliases.push(alias);
            }
        }
        if sounds_like.is_some() {
            entry.sounds_like = sounds_like;
        }
        self.entries.push(entry);
        Ok(existing_index.is_some())
    }

    fn remove(&mut self, term: &str) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| same_value(&entry.term, term.trim()))
        else {
            return false;
        };
        self.entries.remove(index);
        true
    }

    fn validate(&self) -> Result<()> {
        let mut values = HashSet::new();
        for entry in &self.entries {
            clean_value(&entry.term, "term")?;
            if !values.insert(entry.term.to_lowercase()) {
                bail!("duplicate dictionary term or alias: {}", entry.term);
            }
            for alias in &entry.aliases {
                clean_value(alias, "alias")?;
                if !values.insert(alias.to_lowercase()) {
                    bail!("duplicate dictionary term or alias: {alias}");
                }
            }
            if let Some(sounds_like) = &entry.sounds_like {
                clean_value(sounds_like, "pronunciation")?;
            }
        }
        Ok(())
    }

    fn print(&self) {
        if self.entries.is_empty() {
            println!("Dictionary is empty.");
            return;
        }
        for entry in &self.entries {
            println!("{}", entry.term);
            if !entry.aliases.is_empty() {
                println!("  aliases: {}", entry.aliases.join(", "));
            }
            if let Some(sounds_like) = &entry.sounds_like {
                println!("  sounds like: {sounds_like}");
            }
        }
    }
}

fn dictionary_path() -> Result<PathBuf> {
    let base = BaseDirs::new().context("could not determine the platform config directory")?;
    Ok(base.config_dir().join("hear").join("dictionary.json"))
}

fn clean_value(value: &str, description: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("dictionary {description} cannot be empty");
    }
    if value.chars().any(char::is_control) {
        bail!("dictionary {description} cannot contain control characters");
    }
    Ok(value.to_owned())
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn same_value(left: &str, right: &str) -> bool {
    left.to_lowercase() == right.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_updates_and_removes_entries() {
        let mut dictionary = Dictionary::default();
        assert!(
            !dictionary
                .add("Flaport", &["flap port".to_owned()], Some("flah-port"))
                .unwrap()
        );
        assert!(
            dictionary
                .add("flaport", &["flappert".to_owned()], None)
                .unwrap()
        );
        assert_eq!(dictionary.entries.len(), 1);
        assert_eq!(dictionary.entries[0].aliases.len(), 2);
        assert_eq!(
            dictionary.entries[0].sounds_like.as_deref(),
            Some("flah-port")
        );
        assert!(dictionary.remove("FLAPORT"));
        assert!(dictionary.entries.is_empty());
    }

    #[test]
    fn rejects_alias_collisions() {
        let mut dictionary = Dictionary::default();
        dictionary
            .add("Qdrant", &["quadrant".to_owned()], None)
            .unwrap();
        assert!(dictionary.add("Quadrant", &[], None).is_err());
        assert_eq!(dictionary.entries.len(), 1);
        assert_eq!(dictionary.entries[0].term, "Qdrant");
    }

    #[test]
    fn corrects_aliases_as_whole_words() {
        let dictionary = Dictionary {
            entries: vec![Entry {
                term: "Qdrant".to_owned(),
                aliases: vec!["quadrant".to_owned()],
                sounds_like: None,
            }],
        };
        assert_eq!(
            dictionary
                .correct_aliases("Use quadrant, not quadrants.")
                .unwrap(),
            "Use Qdrant, not quadrants."
        );
    }

    #[test]
    fn persists_dictionary_as_json() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("dictionary.json");
        let dictionary = Dictionary {
            entries: vec![Entry {
                term: "Léonie".to_owned(),
                aliases: vec!["Leonie".to_owned()],
                sounds_like: Some("lay-oh-nee".to_owned()),
            }],
        };
        dictionary.save_to(&path).unwrap();
        let mut dictionary = dictionary;
        dictionary.entries[0].aliases.push("Layonie".to_owned());
        dictionary.save_to(&path).unwrap();
        let loaded = Dictionary::load_from(&path).unwrap();
        assert_eq!(loaded.entries[0].term, "Léonie");
        assert_eq!(loaded.entries[0].aliases, ["Leonie", "Layonie"]);
        assert_eq!(loaded.entries[0].sounds_like.as_deref(), Some("lay-oh-nee"));
    }
}
