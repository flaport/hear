use std::fs;

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub fn transcribe(input: &Path, model: Option<&str>, vocabulary: &[String]) -> Result<String> {
    if std::env::var_os("HEAR_CODEX_ACTIVE").is_some() {
        bail!("the codex engine cannot recursively invoke hear");
    }

    let input = input
        .canonicalize()
        .with_context(|| format!("could not resolve audio path: {}", input.display()))?;
    let working_directory = input
        .parent()
        .context("audio file has no parent directory")?;
    let result_file = tempfile::Builder::new()
        .prefix("hear-codex-result-")
        .tempfile()
        .context("could not create a temporary Codex result file")?;

    let vocabulary_hint = if vocabulary.is_empty() {
        String::new()
    } else {
        format!(
            " Prefer these exact spellings when they match the audio: {}.",
            vocabulary.join(", ")
        )
    };
    let prompt = format!(
        "Transcribe the spoken audio in the file at {path}. Return JSON in your final response with exactly two fields: text (a transcript string or null) and error (an explanation string or null). On success set text and set error to null. This is a best-effort task: you may use the network and already-installed \
         tools, but you must not invoke the `hear` command, modify the input file, or modify \
         the working directory. If transcription is impossible, set text to null and explain why in error instead of fabricating a transcript.{vocabulary_hint}",
        path = input.display(),
    );

    let mut command = Command::new("codex");
    command
        .arg("exec")
        .args([
            "--ephemeral",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--color",
            "never",
            "--output-last-message",
        ])
        .arg(result_file.path())
        .args([
            "--config",
            "sandbox_permissions=[\"disk-full-read-access\",\"network-full-access\"]",
        ])
        .arg("--cd")
        .arg(working_directory)
        .env_remove("OPENAI_API_KEY")
        .env("HEAR_CODEX_ACTIVE", "1");
    if let Some(model) = model {
        command.args(["--model", model]);
    }
    command.arg(prompt);

    let output = hear_core::process::run(
        &mut command,
        None,
        &hear_core::process::Cancellation::default(),
        Duration::from_secs(3600),
        true,
    )?;
    let status = output.status;

    if !status.success() {
        bail!(
            "codex exec failed with {}; confirm that Codex is installed and authenticated",
            status
        );
    }

    let transcript = fs::read_to_string(result_file.path())
        .context("codex exec completed without a readable final response")?;
    parse_result(&transcript)
}
fn parse_result(result: &str) -> Result<String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ResultBody {
        text: Option<String>,
        error: Option<String>,
    }
    let response: ResultBody = serde_json::from_str(result)
        .context("Codex did not return a structured transcription result")?;
    if let Some(error) = response.error {
        bail!("Codex could not transcribe the audio: {error}");
    }
    let text = response.text.context("Codex returned no transcript")?;
    if text.trim().is_empty() {
        bail!("Codex returned an empty transcript");
    }
    Ok(text.trim().to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_prose_is_not_a_transcript() {
        assert!(parse_result("I could not transcribe this file").is_err());
        assert!(parse_result(r#"{"text":null,"error":"No transcription facility"}"#).is_err());
        assert_eq!(
            parse_result(r#"{"text":"Hello","error":null}"#).unwrap(),
            "Hello"
        );
    }
}
