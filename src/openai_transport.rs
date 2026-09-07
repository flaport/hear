use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct ApiErrorEnvelope {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
}

pub(crate) fn api_key() -> Result<String> {
    std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")
}

pub(crate) fn client() -> Result<Client> {
    Client::builder()
        .build()
        .context("could not initialize the OpenAI HTTP client")
}

pub(crate) fn response_body(response: Response, operation: &str) -> Result<String> {
    let status = response.status();
    let body = response
        .text()
        .with_context(|| format!("could not read the OpenAI {operation} response"))?;
    ensure_success(status, body, operation)
}

fn ensure_success(status: StatusCode, body: String, operation: &str) -> Result<String> {
    if status.is_success() {
        return Ok(body);
    }

    let message = serde_json::from_str::<ApiErrorEnvelope>(&body)
        .map(|envelope| envelope.error.message)
        .unwrap_or_else(|_| body.trim().to_owned());
    bail!("OpenAI {operation} failed ({status}): {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_successful_response_body() {
        let body = ensure_success(StatusCode::OK, "response".to_owned(), "test").unwrap();
        assert_eq!(body, "response");
    }

    #[test]
    fn extracts_structured_api_error() {
        let error = ensure_success(
            StatusCode::BAD_REQUEST,
            r#"{"error":{"message":"invalid input"}}"#.to_owned(),
            "transcription",
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "OpenAI transcription failed (400 Bad Request): invalid input"
        );
    }

    #[test]
    fn falls_back_to_plain_api_error() {
        let error = ensure_success(
            StatusCode::BAD_GATEWAY,
            "  upstream unavailable\n".to_owned(),
            "formatting",
        )
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "OpenAI formatting failed (502 Bad Gateway): upstream unavailable"
        );
    }
}
