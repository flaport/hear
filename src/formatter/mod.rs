mod prepare;
mod request;
mod response;

#[cfg(feature = "local-polish")]
use anyhow::bail;
use anyhow::{Context, Result};
#[cfg(feature = "local-polish")]
use std::io::Write;
#[cfg(feature = "local-polish")]
use std::path::PathBuf;
#[cfg(feature = "local-polish")]
use std::process::{Command, Stdio};

use crate::FormatContext;
use crate::openai_transport;

use self::prepare::prepare;
use self::request::build;
use self::response::parse;

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";

pub(crate) fn polish(
    transcript: &str,
    model: Option<&str>,
    explicit_context: Option<FormatContext>,
    dictionary_context: Option<&str>,
    custom_instruction: Option<&str>,
) -> Result<String> {
    let prepared = prepare(transcript, explicit_context)?;
    if prepared.context == FormatContext::Verbatim {
        return Ok(prepared.body.to_owned());
    }

    let api_key = openai_transport::api_key()
        .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY is not set; it is required for --polish"))?;
    let client = openai_transport::client()?;
    let request = build(
        model,
        prepared.context,
        prepared.body,
        dictionary_context,
        custom_instruction,
    );
    let response = client
        .post(RESPONSES_URL)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .context("OpenAI formatting request failed")?;
    let body = openai_transport::response_body(response, "formatting")?;
    parse(&body)
}

#[cfg(feature = "local-polish")]
pub(crate) fn polish_local(
    transcript: &str,
    model: Option<&str>,
    explicit_context: Option<FormatContext>,
    dictionary_context: Option<&str>,
    custom_instruction: Option<&str>,
) -> Result<String> {
    let prepared = prepare(transcript, explicit_context)?;
    if prepared.context == FormatContext::Verbatim {
        return Ok(prepared.body.to_owned());
    }
    let input = request::input(
        prepared.context,
        prepared.body,
        dictionary_context,
        custom_instruction,
    );
    run_local_helper(request::INSTRUCTIONS, &input, model)
}

#[cfg(feature = "local-polish")]
fn run_local_helper(instructions: &str, input: &str, model: Option<&str>) -> Result<String> {
    let request = serde_json::json!({
        "instructions": instructions,
        "input": input,
        "model": model,
    });
    let mut child = Command::new(local_helper_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context(
            "could not launch the local polishing helper; install hear-local-polish alongside hear",
        )?;
    child
        .stdin
        .take()
        .context("could not open the local polishing helper input")?
        .write_all(request.to_string().as_bytes())
        .context("could not send the transcript to the local polishing helper")?;
    let output = child
        .wait_with_output()
        .context("could not wait for the local polishing helper")?;
    if !output.status.success() {
        bail!("local polishing helper failed with {}", output.status);
    }
    let transcript = String::from_utf8(output.stdout)
        .context("local polishing helper returned text that was not UTF-8")?;
    let transcript = transcript.trim();
    if transcript.is_empty() {
        bail!("local polishing helper returned an empty transcript");
    }
    Ok(transcript.to_owned())
}

#[cfg(feature = "local-polish")]
fn local_helper_path() -> PathBuf {
    if let Some(path) = std::env::var_os("HEAR_LOCAL_POLISH_PATH") {
        return path.into();
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        let sibling = directory.join("hear-local-polish");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("hear-local-polish")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_verbatim_directive_bypasses_formatting_options() {
        let transcript = polish(
            "Verbatim Keep this exactly.",
            Some("unused-model"),
            None,
            Some("- Exactly; aliases: differently"),
            Some("Rewrite the transcript."),
        )
        .unwrap();
        assert_eq!(transcript, "Keep this exactly.");
    }

    #[test]
    #[ignore = "calls the live OpenAI API"]
    fn live_formatter_request() {
        let formatted = polish("Todo buy milk and call Alex", None, None, None, None).unwrap();
        let formatted = formatted.to_ascii_lowercase();
        assert!(formatted.contains("buy milk"));
        assert!(formatted.contains("call alex"));
    }

    #[cfg(feature = "local-polish")]
    #[test]
    #[ignore = "downloads and runs the local model"]
    fn live_local_formatter_request() {
        let formatted =
            polish_local("Todo buy milk and call Alex", None, None, None, None).unwrap();
        let formatted = formatted.to_ascii_lowercase();
        assert!(formatted.contains("buy milk"));
        assert!(formatted.contains("call alex"));
    }
}
