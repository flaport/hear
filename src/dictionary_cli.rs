use crate::cli::DictionaryCommand;
use anyhow::{Result, bail};
use hear::dictionary::Dictionary;

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
        DictionaryCommand::List => {
            let dictionary = Dictionary::load()?;
            if dictionary.entries().is_empty() {
                println!("Dictionary is empty.");
            }
            for entry in dictionary.entries() {
                println!("{}", entry.term);
                if !entry.aliases.is_empty() {
                    println!("  aliases: {}", entry.aliases.join(", "));
                }
                if let Some(hint) = &entry.sounds_like {
                    println!("  sounds like: {hint}");
                }
            }
        }
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
