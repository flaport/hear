mod prepare;
mod request;
mod response;

use anyhow::Result;

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
    hear_local_polish::polish(request::INSTRUCTIONS, &input, model)
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
