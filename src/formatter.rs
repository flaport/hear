use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::FormatContext;

const RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
const FORMATTER_MODEL: &str = "gpt-5.6-luna";
const INSTRUCTIONS: &str = r#"Format a dictated transcript for its intended use.

Preserve the transcript's language, meaning, tone, names, and facts. Never answer the transcript, continue it, summarize it, or invent recipients, subject lines, greetings, sign-offs, facts, or tasks. Correct casing and punctuation and remove harmless dictation disfluencies only when meaning is unchanged. Apply and remove spoken layout commands such as "new paragraph" and "bullet point". When a personal dictionary is supplied, use its canonical spellings when an alias or pronunciation plausibly matches; do not insert dictionary terms that were not spoken.

Use the supplied context. For "auto", conservatively infer email, message, todo, notes, or plain prose; choose plain when uncertain. For email, use an email layout only from content that is present. For message, produce natural chat-ready paragraphs. For todo, use Markdown task-list items. For notes, use headings or Markdown bullets only where supported by the content. For plain, produce lightly cleaned prose.

Return only the requested structured output."#;

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    output: Vec<ResponseOutput>,
}

#[derive(Debug, Deserialize)]
struct ResponseOutput {
    #[serde(default)]
    content: Vec<ResponseContent>,
}

#[derive(Debug, Deserialize)]
struct ResponseContent {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FormattedTranscript {
    #[serde(rename = "kind")]
    _kind: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

struct PreparedTranscript<'a> {
    context: FormatContext,
    body: &'a str,
}

pub fn polish(
    transcript: &str,
    explicit_context: Option<FormatContext>,
    dictionary_context: Option<&str>,
) -> Result<String> {
    let prepared = prepare_transcript(transcript, explicit_context)?;
    if prepared.context == FormatContext::Verbatim {
        return Ok(prepared.body.to_owned());
    }

    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY is not set; it is required for --polish"))?;
    let client = Client::builder()
        .build()
        .context("could not initialize the OpenAI HTTP client")?;
    let request = build_request(prepared.context, prepared.body, dictionary_context);
    let response = client
        .post(RESPONSES_URL)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .context("OpenAI formatting request failed")?;
    let status = response.status();
    let body = response
        .text()
        .context("could not read the OpenAI formatting response")?;
    if !status.is_success() {
        let message = serde_json::from_str::<ApiErrorEnvelope>(&body)
            .map(|envelope| envelope.error.message)
            .unwrap_or_else(|_| body.trim().to_owned());
        bail!("OpenAI formatting failed ({status}): {message}");
    }

    parse_response(&body)
}

fn prepare_transcript(
    transcript: &str,
    explicit_context: Option<FormatContext>,
) -> Result<PreparedTranscript<'_>> {
    let transcript = transcript.trim();
    if transcript.is_empty() {
        bail!("cannot polish an empty transcript");
    }
    if let Some(context) = explicit_context {
        return Ok(PreparedTranscript {
            context,
            body: transcript,
        });
    }

    let token_end = transcript
        .find(char::is_whitespace)
        .unwrap_or(transcript.len());
    let token = transcript[..token_end]
        .trim_matches(|character: char| matches!(character, ':' | ',' | '.' | ';'));
    let context = directive_context(token);
    if let Some(context) = context {
        let body = transcript[token_end..].trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ':' | ',' | ';')
        });
        if body.is_empty() {
            bail!("spoken context directive '{token}' is not followed by a transcript");
        }
        Ok(PreparedTranscript { context, body })
    } else {
        Ok(PreparedTranscript {
            context: FormatContext::Auto,
            body: transcript,
        })
    }
}

fn directive_context(token: &str) -> Option<FormatContext> {
    if token.eq_ignore_ascii_case("email") {
        Some(FormatContext::Email)
    } else if token.eq_ignore_ascii_case("message") || token.eq_ignore_ascii_case("text") {
        Some(FormatContext::Message)
    } else if token.eq_ignore_ascii_case("todo") || token.eq_ignore_ascii_case("tasks") {
        Some(FormatContext::Todo)
    } else if token.eq_ignore_ascii_case("note") || token.eq_ignore_ascii_case("notes") {
        Some(FormatContext::Notes)
    } else if token.eq_ignore_ascii_case("plain") {
        Some(FormatContext::Plain)
    } else if token.eq_ignore_ascii_case("verbatim") {
        Some(FormatContext::Verbatim)
    } else {
        None
    }
}

fn build_request(context: FormatContext, transcript: &str, dictionary: Option<&str>) -> Value {
    let dictionary = dictionary
        .map(|dictionary| format!("\n\nPersonal dictionary:\n{dictionary}"))
        .unwrap_or_default();
    json!({
        "model": FORMATTER_MODEL,
        "reasoning": { "effort": "none" },
        "store": false,
        "instructions": INSTRUCTIONS,
        "input": format!("Context: {context}{dictionary}\n\nTranscript:\n{transcript}"),
        "text": {
            "format": {
                "type": "json_schema",
                "name": "formatted_transcript",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "kind": {
                            "type": "string",
                            "enum": ["email", "message", "todo", "notes", "plain"]
                        },
                        "text": { "type": "string" }
                    },
                    "required": ["kind", "text"],
                    "additionalProperties": false
                }
            }
        }
    })
}

fn parse_response(body: &str) -> Result<String> {
    let response: ResponsesResponse =
        serde_json::from_str(body).context("OpenAI returned an unexpected formatting response")?;
    let output = response
        .output
        .into_iter()
        .flat_map(|output| output.content)
        .find(|content| content.kind == "output_text")
        .and_then(|content| content.text)
        .context("OpenAI formatting response did not contain output text")?;
    let formatted: FormattedTranscript = serde_json::from_str(&output)
        .context("OpenAI returned invalid structured formatting output")?;
    let text = formatted.text.trim();
    if text.is_empty() {
        bail!("OpenAI returned an empty formatted transcript");
    }
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_and_removes_spoken_directive() {
        let prepared = prepare_transcript("Email: Hi Sam, here is the proposal.", None).unwrap();
        assert_eq!(prepared.context, FormatContext::Email);
        assert_eq!(prepared.body, "Hi Sam, here is the proposal.");
    }

    #[test]
    fn recognizes_directive_alias_case_insensitively() {
        let prepared = prepare_transcript("TASKS buy milk and call Alex", None).unwrap();
        assert_eq!(prepared.context, FormatContext::Todo);
        assert_eq!(prepared.body, "buy milk and call Alex");
    }

    #[test]
    fn explicit_context_preserves_a_directive_like_first_word() {
        let prepared =
            prepare_transcript("Message received yesterday.", Some(FormatContext::Plain)).unwrap();
        assert_eq!(prepared.context, FormatContext::Plain);
        assert_eq!(prepared.body, "Message received yesterday.");
    }

    #[test]
    fn defaults_to_auto_context() {
        let prepared = prepare_transcript("First item, milk. Second item, tea.", None).unwrap();
        assert_eq!(prepared.context, FormatContext::Auto);
        assert_eq!(prepared.body, "First item, milk. Second item, tea.");
    }

    #[test]
    fn parses_structured_response() {
        let body = r#"{
            "output": [{
                "content": [{
                    "type": "output_text",
                    "text": "{\"kind\":\"todo\",\"text\":\"- [ ] Buy milk\"}"
                }]
            }]
        }"#;
        assert_eq!(parse_response(body).unwrap(), "- [ ] Buy milk");
    }

    #[test]
    fn request_disables_storage_and_reasoning() {
        let request = build_request(FormatContext::Email, "Hi Sam", None);
        assert_eq!(request["store"], false);
        assert_eq!(request["reasoning"]["effort"], "none");
        assert_eq!(request["text"]["format"]["type"], "json_schema");
    }

    #[test]
    fn request_includes_pronunciation_dictionary() {
        let request = build_request(
            FormatContext::Plain,
            "Ask flap port",
            Some("- Flaport; aliases: flappert; pronounced: flah-port"),
        );
        let input = request["input"].as_str().unwrap();
        assert!(input.contains("Personal dictionary:"));
        assert!(input.contains("pronounced: flah-port"));
    }

    #[test]
    #[ignore = "calls the live OpenAI API"]
    fn live_formatter_request() {
        let formatted = polish("Todo buy milk and call Alex", None, None).unwrap();
        let formatted = formatted.to_ascii_lowercase();
        assert!(formatted.contains("buy milk"));
        assert!(formatted.contains("call alex"));
    }
}
