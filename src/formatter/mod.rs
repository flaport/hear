mod prepare;
mod request;
mod response;

use anyhow::{Context, Result};

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
}
