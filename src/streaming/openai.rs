use super::Adapter;
use crate::OpenAiClient;
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hear_core::capture::Resampler;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::ErrorKind,
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest, stream::MaybeTlsStream};

const RATE: usize = 24000;
const TURN_SAMPLES: usize = 15 * RATE;
const FINAL_TIMEOUT: Duration = Duration::from_secs(60);
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

pub(super) struct Realtime {
    socket: Socket,
    resampler: Resampler,
    pending: usize,
    commits: usize,
    turns: Turns,
}

#[derive(Default)]
struct Turns {
    order: Vec<String>,
    completed: HashMap<String, String>,
    bytes: usize,
    configured: bool,
}
impl Turns {
    fn event(&mut self, event: Value) -> Result<()> {
        match event["type"].as_str().unwrap_or("") {
            "session.updated" => self.configured = true,
            "input_audio_buffer.committed" => {
                let id = event["item_id"]
                    .as_str()
                    .context("missing committed item ID")?;
                if self.order.iter().any(|known| known == id) {
                    bail!("duplicate committed item ID");
                }
                self.order.push(id.into());
                if self.order.len() > 1024 {
                    bail!("too many realtime audio turns");
                }
            }
            "conversation.item.input_audio_transcription.completed" => {
                let id = event["item_id"]
                    .as_str()
                    .context("missing transcript item ID")?;
                let text = event["transcript"]
                    .as_str()
                    .context("missing completed transcript")?;
                self.bytes += text.len();
                if self.bytes > 8 * 1024 * 1024 {
                    bail!("realtime transcript exceeded 8 MiB");
                }
                self.completed.insert(id.into(), text.into());
            }
            "error" | "conversation.item.input_audio_transcription.failed" => {
                bail!(
                    "OpenAI realtime: {}",
                    event["error"]["message"]
                        .as_str()
                        .unwrap_or("transcription failed")
                );
            }
            // Deltas may be revised. Paste only the completed transcript for each turn.
            _ => {}
        }
        Ok(())
    }
    fn complete(&self, expected: usize) -> bool {
        expected > 0
            && self.order.len() == expected
            && self.order.iter().all(|id| self.completed.contains_key(id))
    }
    fn text(&self) -> String {
        self.order
            .iter()
            .filter_map(|id| self.completed.get(id))
            .map(|text| text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Realtime {
    pub fn new(client: &OpenAiClient, vocabulary: &[String]) -> Result<Self> {
        for term in vocabulary {
            if term.contains(['<', '>', '\r', '\n']) {
                bail!("OpenAI realtime dictionary terms cannot contain <, >, or newlines");
            }
        }
        let mut url = reqwest::Url::parse(&client.endpoint("realtime"))?;
        let scheme = if url.scheme() == "https" { "wss" } else { "ws" };
        url.set_scheme(scheme)
            .map_err(|_| anyhow::anyhow!("invalid realtime endpoint"))?;
        url.set_query(Some("intent=transcription"));
        let host = url.host_str().context("missing realtime host")?;
        let port = url
            .port_or_known_default()
            .context("missing realtime port")?;
        let mut connected = None;
        let start = Instant::now();
        for address in (host, port).to_socket_addrs()? {
            let remaining = Duration::from_secs(15).saturating_sub(start.elapsed());
            if remaining.is_zero() {
                break;
            }
            if let Ok(socket) = TcpStream::connect_timeout(&address, remaining) {
                connected = Some(socket);
                break;
            }
        }
        let tcp = connected.context("could not connect to OpenAI realtime within 15 seconds")?;
        tcp.set_read_timeout(Some(Duration::from_secs(15)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(15)))?;
        tcp.set_nodelay(true)?;
        let mut request = url.as_str().into_client_request()?;
        request
            .headers_mut()
            .insert("Authorization", format!("Bearer {}", client.key).parse()?);
        let config =
            tungstenite::protocol::WebSocketConfig::default().max_message_size(Some(1024 * 1024));
        let (socket, _) = tungstenite::client_tls_with_config(request, tcp, Some(config), None)
            .map_err(|error| anyhow::anyhow!("OpenAI realtime handshake failed: {error}"))?;
        let mut session = Self {
            socket,
            resampler: Resampler::new(16000, 24000),
            pending: 0,
            commits: 0,
            turns: Turns::default(),
        };
        session.set_read_timeout(Duration::from_millis(10))?;
        session.send(json!({"type":"session.update", "session": {
            "type":"transcription", "audio":{"input":{
                "format":{"type":"audio/pcm", "rate":24000},
                "transcription":{"model":"gpt-live-transcribe", "keywords":vocabulary, "delay":"medium"},
                "turn_detection":null
            }}
        }}))?;
        let start = Instant::now();
        while !session.turns.configured {
            if start.elapsed() >= Duration::from_secs(15) {
                bail!("OpenAI realtime session setup timed out");
            }
            session.read_event()?;
        }
        session.set_read_timeout(Duration::from_millis(1))?;
        Ok(session)
    }
    fn set_read_timeout(&mut self, timeout: Duration) -> Result<()> {
        match self.socket.get_mut() {
            MaybeTlsStream::Plain(tcp) => tcp.set_read_timeout(Some(timeout))?,
            MaybeTlsStream::Rustls(tls) => tls.sock.set_read_timeout(Some(timeout))?,
            _ => bail!("unsupported realtime TLS transport"),
        }
        Ok(())
    }
    fn send(&mut self, event: Value) -> Result<()> {
        self.socket.send(Message::Text(event.to_string().into()))?;
        Ok(())
    }
    fn read_event(&mut self) -> Result<bool> {
        match self.socket.read() {
            Ok(Message::Text(text)) => self.turns.event(serde_json::from_str(&text)?)?,
            Ok(Message::Close(_)) => bail!("OpenAI realtime closed before transcription finished"),
            Ok(Message::Ping(_)) => self.socket.flush()?,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                return Ok(false);
            }
            Err(error) => return Err(error.into()),
        }
        Ok(true)
    }
    fn commit(&mut self) -> Result<()> {
        // The API requires at least 100 ms in a committed buffer.
        if self.pending < RATE / 10 {
            let padding = vec![0_u8; (RATE / 10 - self.pending) * 2];
            self.send(
                json!({"type":"input_audio_buffer.append", "audio":STANDARD.encode(padding)}),
            )?;
        }
        self.send(json!({"type":"input_audio_buffer.commit"}))?;
        self.commits += 1;
        self.pending = 0;
        Ok(())
    }
}
impl Adapter for Realtime {
    fn push(&mut self, samples: &[i16]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 3 + 2);
        for &sample in samples {
            self.resampler.push(sample as f32, |v| {
                bytes.extend_from_slice(
                    &(v.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16).to_le_bytes(),
                );
                Ok(())
            })?;
        }
        self.pending += bytes.len() / 2;
        if !bytes.is_empty() {
            self.send(json!({"type":"input_audio_buffer.append", "audio":STANDARD.encode(bytes)}))?;
        }
        // Bound each turn for long dictations; final events can arrive out of order.
        if self.pending >= TURN_SAMPLES {
            self.commit()?;
        }
        for _ in 0..100 {
            if !self.read_event()? {
                break;
            }
        }
        Ok(())
    }
    fn finish(mut self: Box<Self>) -> Result<String> {
        if self.pending > 0 {
            self.commit()?;
        }
        self.set_read_timeout(Duration::from_millis(50))?;
        let start = Instant::now();
        while !self.turns.complete(self.commits) {
            if start.elapsed() >= FINAL_TIMEOUT {
                bail!("timed out waiting for the final realtime transcript");
            }
            self.read_event()?;
        }
        let text = self.turns.text();
        let _ = self.socket.close(None);
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[allow(clippy::result_large_err)] // tungstenite fixes the callback error type.
    fn websocket_stream_resamples_commits_and_flushes_short_final_turn() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut socket = tungstenite::accept_hdr(
                tcp,
                |request: &tungstenite::handshake::server::Request, response| {
                    assert_eq!(request.uri().to_string(), "/realtime?intent=transcription");
                    assert_eq!(request.headers()["authorization"], "Bearer test");
                    Ok(response)
                },
            )
            .unwrap();
            let event: Value =
                serde_json::from_str(socket.read().unwrap().to_text().unwrap()).unwrap();
            assert_eq!(
                event["session"]["audio"]["input"]["transcription"]["model"],
                "gpt-live-transcribe"
            );
            assert_eq!(
                event["session"]["audio"]["input"]["transcription"]["keywords"][0],
                "Qdrant"
            );
            socket
                .send(Message::Text(
                    json!({"type":"session.updated"}).to_string().into(),
                ))
                .unwrap();
            let mut bytes = Vec::new();
            let mut sizes = Vec::new();
            loop {
                let message = socket.read().unwrap();
                let event: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
                match event["type"].as_str().unwrap() {
                    "input_audio_buffer.append" => {
                        bytes.extend(STANDARD.decode(event["audio"].as_str().unwrap()).unwrap())
                    }
                    "input_audio_buffer.commit" => {
                        sizes.push(bytes.len());
                        let id = format!("turn{}", sizes.len());
                        socket
                            .send(Message::Text(
                                json!({"type":"input_audio_buffer.committed", "item_id":id})
                                    .to_string()
                                    .into(),
                            ))
                            .unwrap();
                        if sizes.len() == 1 {
                            assert!(
                                bytes
                                    .chunks_exact(2)
                                    .all(|b| i16::from_le_bytes([b[0], b[1]]) == 1000)
                            );
                            bytes.clear();
                        } else {
                            // Complete the second turn first, before the first turn.
                            for (id, text) in [("turn2", "last words"), ("turn1", "use quadrant")] {
                                socket.send(Message::Text(json!({"type":"conversation.item.input_audio_transcription.completed", "item_id":id, "transcript":text}).to_string().into())).unwrap();
                            }
                            // Wait for client close, keeping the socket alive through finalization.
                            let _ = socket.read();
                            return sizes;
                        }
                    }
                    other => panic!("unexpected event {other}"),
                }
            }
        });
        let mut dictionary = crate::dictionary::Dictionary::default();
        dictionary
            .add("Qdrant", &["quadrant".into()], None)
            .unwrap();
        let client = OpenAiClient::builder("test")
            .base_url(format!("http://{address}"))
            .build()
            .unwrap();
        let audio: Vec<_> = std::iter::repeat_n(1000_i16, 15 * 16000 + 160)
            .flat_map(i16::to_le_bytes)
            .collect();
        let transcript = crate::Workflow::new(crate::HearConfig {
            stream: true,
            polish: false,
            ..Default::default()
        })
        .openai_client(client)
        .dictionary(dictionary)
        .run_streaming(audio.as_slice())
        .unwrap();
        assert_eq!(transcript.raw, "use Qdrant last words");
        assert_eq!(transcript.text, transcript.raw);
        assert_eq!(server.join().unwrap(), vec![15 * RATE * 2, RATE / 10 * 2]);
    }
    #[test]
    fn orders_turns_by_commit_not_completion_and_ignores_partial_text() {
        let mut turns = Turns::default();
        for id in ["a", "b"] {
            turns
                .event(json!({"type":"input_audio_buffer.committed", "item_id":id}))
                .unwrap();
        }
        turns.event(json!({"type":"conversation.item.input_audio_transcription.delta", "item_id":"a", "delta":"wrong"})).unwrap();
        turns.event(json!({"type":"conversation.item.input_audio_transcription.completed", "item_id":"b", "transcript":"second"})).unwrap();
        assert!(!turns.complete(2));
        turns.event(json!({"type":"conversation.item.input_audio_transcription.completed", "item_id":"a", "transcript":"first"})).unwrap();
        assert!(turns.complete(2));
        assert_eq!(turns.text(), "first second");
    }
    #[test]
    fn server_failures_are_not_partial_successes() {
        let mut turns = Turns::default();
        assert!(turns.event(json!({"type":"conversation.item.input_audio_transcription.failed", "error":{"message":"quota"}})).unwrap_err().to_string().contains("quota"));
    }
}
