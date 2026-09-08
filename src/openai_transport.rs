use crate::{PolishOptions, Transcript, TranscriptionOptions};
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::{path::Path, sync::Arc, time::Duration};

/// Failures callers can classify without parsing error strings.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Configuration(String),
    Transport(reqwest::Error),
    Api {
        status: u16,
        message: String,
    },
    Input(anyhow::Error),
    Response(anyhow::Error),
    /// Transcription succeeded. The original text remains available for retry.
    Polishing {
        raw: String,
        source: Box<Error>,
    },
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(s) => write!(f, "{s}"),
            Self::Transport(e) => write!(f, "OpenAI transport failed: {e}"),
            Self::Api { status, message } => {
                write!(f, "OpenAI request failed ({status}): {message}")
            }
            Self::Input(e) => write!(f, "invalid input: {e:#}"),
            Self::Response(e) => write!(f, "invalid response: {e:#}"),
            Self::Polishing { source, .. } => {
                write!(f, "polishing failed after transcription: {source}")
            }
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Transport(e) => Some(e),
            Self::Input(e) | Self::Response(e) => Some(e.as_ref()),
            Self::Polishing { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
#[derive(Debug, Clone)]
pub enum ProgressEvent {
    PreparingAudio,
    Uploading { part: usize, total: usize },
    Polishing,
    Message(String),
}
type Observer = Arc<dyn Fn(ProgressEvent) + Send + Sync>;
/// Reusable blocking client with explicit credentials, timeouts and optional progress reporting.
/// Call from a blocking worker when embedding in an asynchronous application.
#[derive(Clone)]
pub struct OpenAiClient {
    pub(crate) http: Client,
    pub(crate) key: String,
    base_url: String,
    observer: Option<Observer>,
}
pub struct OpenAiClientBuilder {
    key: String,
    connect_timeout: Duration,
    request_timeout: Option<Duration>,
    base_url: String,
    observer: Option<Observer>,
}
impl OpenAiClient {
    pub fn builder(api_key: impl Into<String>) -> OpenAiClientBuilder {
        OpenAiClientBuilder {
            key: api_key.into(),
            connect_timeout: Duration::from_secs(15),
            request_timeout: Some(Duration::from_secs(900)),
            base_url: "https://api.openai.com/v1".into(),
            observer: None,
        }
    }
    pub fn from_env() -> Result<Self, Error> {
        Self::builder(
            std::env::var("OPENAI_API_KEY")
                .map_err(|_| Error::Configuration("OPENAI_API_KEY is not set".into()))?,
        )
        .build()
    }
    pub(crate) fn endpoint(&self, path: &str) -> String {
        format!("{}/{path}", self.base_url.trim_end_matches('/'))
    }
    pub(crate) fn report(&self, event: ProgressEvent) {
        if let Some(observer) = &self.observer {
            observer(event);
        }
    }
    pub fn transcribe_raw(&self, input: &Path, vocabulary: &[String]) -> Result<String, Error> {
        crate::openai::transcribe_with_client(self, input, vocabulary)
    }
    pub fn polish(&self, transcript: &str, options: &PolishOptions<'_>) -> Result<String, Error> {
        crate::formatter::polish_with_client(
            self,
            transcript,
            options.model,
            options.context,
            options.dictionary_context,
            options.instruction,
        )
    }
    pub fn transcribe(
        &self,
        input: &Path,
        options: &TranscriptionOptions<'_>,
    ) -> Result<Transcript, Error> {
        let raw = self.transcribe_raw(input, options.vocabulary)?;
        let text = match options.polishing {
            Some(options) => match self.polish(&raw, &options) {
                Ok(text) => text,
                Err(source) => {
                    return Err(Error::Polishing {
                        raw,
                        source: Box::new(source),
                    });
                }
            },
            None => raw.clone(),
        };
        Ok(Transcript { raw, text })
    }
}
impl OpenAiClientBuilder {
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }
    pub fn request_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.request_timeout = timeout;
        self
    }
    /// Override the API endpoint, for example for a local test server.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into();
        self
    }
    pub fn progress(mut self, observer: impl Fn(ProgressEvent) + Send + Sync + 'static) -> Self {
        self.observer = Some(Arc::new(observer));
        self
    }
    pub fn build(self) -> Result<OpenAiClient, Error> {
        if self.key.trim().is_empty() {
            return Err(Error::Configuration("API key cannot be empty".into()));
        }
        let url =
            reqwest::Url::parse(&self.base_url).map_err(|e| Error::Configuration(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::Configuration(
                "API endpoint must use HTTP or HTTPS".into(),
            ));
        }
        let http = Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .build()
            .map_err(Error::Transport)?;
        Ok(OpenAiClient {
            http,
            key: self.key,
            base_url: self.base_url,
            observer: self.observer,
        })
    }
}
#[derive(Deserialize)]
struct Envelope {
    error: ApiError,
}
#[derive(Deserialize)]
struct ApiError {
    message: String,
}
pub(crate) fn response_body(response: Response) -> Result<String, Error> {
    let status = response.status();
    let body = response.text().map_err(Error::Transport)?;
    if status.is_success() {
        return Ok(body);
    }
    let message = serde_json::from_str::<Envelope>(&body)
        .map(|e| e.error.message)
        .unwrap_or_else(|_| body.trim().to_owned());
    Err(Error::Api {
        status: status.as_u16(),
        message,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };
    fn server(responses: Vec<(u16, &'static str)>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            for (status, body) in responses {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = socket.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                    if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                        let len = headers
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix("content-length:")
                                    .and_then(|n| n.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                write!(socket,"HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            }
        });
        (url, worker)
    }
    #[test]
    fn preserves_raw_text_when_polishing_fails() {
        let (url, worker) = server(vec![
            (200, r#"{"text":"hello world"}"#),
            (429, r#"{"error":{"message":"quota"}}"#),
        ]);
        let client = OpenAiClient::builder("test").base_url(url).build().unwrap();
        let audio = tempfile::Builder::new().suffix(".wav").tempfile().unwrap();
        let error = client
            .transcribe(
                audio.path(),
                &TranscriptionOptions::new().polish(PolishOptions::new()),
            )
            .unwrap_err();
        match error {
            Error::Polishing { raw, source } => {
                assert_eq!(raw, "hello world");
                assert!(matches!(*source, Error::Api { status: 429, .. }));
            }
            _ => panic!("unexpected error"),
        }
        worker.join().unwrap();
    }
    #[test]
    fn honors_configured_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(150));
        });
        let client = OpenAiClient::builder("test")
            .base_url(url)
            .request_timeout(Some(Duration::from_millis(30)))
            .build()
            .unwrap();
        let result = client.polish("hello", &PolishOptions::new());
        assert!(matches!(result,Err(Error::Transport(e)) if e.is_timeout()));
        worker.join().unwrap();
    }
}
