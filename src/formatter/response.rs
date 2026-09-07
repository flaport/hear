use anyhow::{Context, Result, bail};
use serde::Deserialize;

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

pub(super) fn parse(body: &str) -> Result<String> {
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
    fn parses_structured_response() {
        let body = r#"{
            "output": [{
                "content": [{
                    "type": "output_text",
                    "text": "{\"kind\":\"todo\",\"text\":\"- [ ] Buy milk\"}"
                }]
            }]
        }"#;
        assert_eq!(parse(body).unwrap(), "- [ ] Buy milk");
    }

    #[test]
    fn rejects_empty_formatted_transcript() {
        let body = r#"{
            "output": [{
                "content": [{
                    "type": "output_text",
                    "text": "{\"kind\":\"plain\",\"text\":\"  \"}"
                }]
            }]
        }"#;
        assert_eq!(
            parse(body).unwrap_err().to_string(),
            "OpenAI returned an empty formatted transcript"
        );
    }
}
