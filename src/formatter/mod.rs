mod prepare;
mod request;
mod response;

use anyhow::Result;
#[cfg(feature = "local-polish")]
use anyhow::{Context, bail};
#[cfg(feature = "local-polish")]
use std::path::PathBuf;
#[cfg(feature = "local-polish")]
use std::process::Command;

use crate::FormatContext;
use crate::openai_transport;

use self::prepare::prepare;
use self::request::build;
use self::response::parse;

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

    polish_with_client(
        &crate::OpenAiClient::from_env()?,
        transcript,
        model,
        explicit_context,
        dictionary_context,
        custom_instruction,
    )
    .map_err(Into::into)
}
pub(crate) fn polish_with_client(
    client: &crate::OpenAiClient,
    transcript: &str,
    model: Option<&str>,
    explicit_context: Option<FormatContext>,
    dictionary_context: Option<&str>,
    custom_instruction: Option<&str>,
) -> std::result::Result<String, crate::Error> {
    let prepared = prepare(transcript, explicit_context).map_err(crate::Error::Input)?;
    if prepared.context == FormatContext::Verbatim {
        return Ok(prepared.body.to_owned());
    }
    client.report(crate::ProgressEvent::Polishing);
    let request = build(
        model,
        prepared.context,
        prepared.body,
        dictionary_context,
        custom_instruction,
    );
    let response = client
        .http
        .post(client.endpoint("responses"))
        .bearer_auth(&client.key)
        .json(&request)
        .send()
        .map_err(crate::Error::Transport)?;
    let body = openai_transport::response_body(response)?;
    parse(&body).map_err(crate::Error::Response)
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
    let output = hear_core::process::run(
        &mut Command::new(local_helper_path()),
        Some(request.to_string().into_bytes()),
        &hear_core::process::Cancellation::default(),
        std::time::Duration::from_secs(3600),
        false,
    )?;
    if !output.status.success() {
        bail!(
            "local polishing helper failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
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
