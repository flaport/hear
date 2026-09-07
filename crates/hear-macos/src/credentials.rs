use anyhow::{Context, Result, bail};

const SERVICE: &str = "dev.flaport.hear-macos";
const ACCOUNT: &str = "openai-api-key";

pub fn stored_api_key() -> Result<Option<String>> {
    match entry()?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error).context("could not read the OpenAI API key from Keychain"),
    }
}

pub fn install_api_key() -> Result<()> {
    let password = rpassword::prompt_password("OpenAI API key: ")?;
    let password = password.trim();
    if password.is_empty() {
        bail!("API key cannot be empty");
    }
    entry()?
        .set_password(password)
        .context("could not save the OpenAI API key to Keychain")?;
    println!("Saved the OpenAI API key in macOS Keychain.");
    Ok(())
}

pub fn remove_api_key() -> Result<()> {
    match entry()?.delete_credential() {
        Ok(()) => println!("Removed the OpenAI API key from macOS Keychain."),
        Err(keyring::Error::NoEntry) => println!("No stored OpenAI API key was found."),
        Err(error) => return Err(error).context("could not remove the API key from Keychain"),
    }
    Ok(())
}

fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, ACCOUNT).context("could not access macOS Keychain")
}
