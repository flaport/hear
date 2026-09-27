use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Mutex, mpsc},
    thread,
};

fn server(
    handler: impl FnOnce(TcpStream, serde_json::Value) + Send + 'static,
) -> (OpenAiSpeech, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(&mut stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert_eq!(line, "POST /v1/audio/speech HTTP/1.1\r\n");
        let mut length = 0;
        let mut auth = false;
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            let lower = line.to_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            if lower == "authorization: bearer test-key\r\n" {
                auth = true;
            }
        }
        assert!(auth);
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        handler(stream, serde_json::from_slice(&body).unwrap());
    });
    (
        OpenAiSpeech::new(
            OpenAiClient::builder("test-key")
                .base_url(url)
                .build()
                .unwrap(),
        ),
        worker,
    )
}

#[test]
fn streams_before_eof_and_preserves_request_and_events() {
    let (tx, rx) = mpsc::channel();
    let (engine, worker) = server(move |mut stream, body| {
        assert_eq!(body["voice"], "cedar");
        assert_eq!(body["model"], "gpt-4o-mini-tts");
        assert_eq!(body["response_format"], "pcm");
        assert_eq!(body["instructions"], "Speak warmly");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nContent-Length: 4\r\n\r\n\x01\x00",
            )
            .unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        stream.write_all(&[255, 255]).unwrap();
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = events.clone();
    let engine = engine.progress(move |e| captured.lock().unwrap().push(e));
    let mut received = Vec::new();
    let mut request = SpeechRequest::new("Hello");
    request.instructions = Some("Speak warmly");
    let summary = engine
        .synthesize(
            &request,
            &mut |samples: &[i16]| {
                received.extend_from_slice(samples);
                tx.send(()).ok();
                Ok(())
            },
            &Cancellation::default(),
        )
        .unwrap();
    worker.join().unwrap();
    assert_eq!(received, [1, -1]);
    assert_eq!(summary.samples, 2);
    let events = events.lock().unwrap();
    assert!(matches!(
        events.as_slice(),
        [
            SpeechEvent::Started,
            SpeechEvent::FirstAudio(_),
            SpeechEvent::Completed(_)
        ]
    ));
}

#[test]
fn handles_byte_boundaries_errors_and_cancellation() {
    struct OneByte(std::io::Cursor<Vec<u8>>);
    impl Read for OneByte {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(&mut b[..1])
        }
    }
    let token = Cancellation::default();
    let mut samples = Vec::new();
    decode_pcm(
        OneByte(std::io::Cursor::new(vec![0, 128, 255, 127])),
        &mut |s: &[i16]| {
            samples.extend_from_slice(s);
            Ok(())
        },
        &token,
        Instant::now(),
        |_| {},
    )
    .unwrap();
    assert_eq!(samples, [-32768, 32767]);
    for bytes in [vec![], vec![0], vec![0, 0, 1]] {
        assert!(matches!(
            decode_pcm(
                &bytes[..],
                &mut |_: &[i16]| Ok(()),
                &token,
                Instant::now(),
                |_| {}
            ),
            Err(SpeechError::Decode(_))
        ));
    }
    assert!(matches!(
        decode_pcm(
            &[0, 0][..],
            &mut |_: &[i16]| Err(SpeechError::Output(anyhow::anyhow!("disk full"))),
            &token,
            Instant::now(),
            |_| {}
        ),
        Err(SpeechError::Output(_))
    ));
    assert!(matches!(
        decode_pcm(
            &[0, 0][..],
            &mut |_: &[i16]| {
                token.cancel();
                Ok(())
            },
            &token,
            Instant::now(),
            |_| {}
        ),
        Err(SpeechError::Cancelled)
    ));
}

#[test]
fn validates_without_network_and_reports_cancelled() {
    let engine = OpenAiSpeech::new(
        OpenAiClient::builder("test")
            .base_url("http://127.0.0.1:1")
            .build()
            .unwrap(),
    );
    for text in [String::new(), "x".repeat(4097)] {
        assert!(matches!(
            engine.synthesize(
                &SpeechRequest::new(&text),
                &mut |_: &[i16]| Ok(()),
                &Cancellation::default()
            ),
            Err(SpeechError::Configuration(_))
        ));
    }
    let mut request = SpeechRequest::new("Hello");
    request.speed = f32::NAN;
    assert!(validate(&request).is_err());
    let token = Cancellation::default();
    token.cancel();
    assert!(matches!(
        engine.synthesize(
            &SpeechRequest::new("Hello"),
            &mut |_: &[i16]| panic!("cancelled"),
            &token
        ),
        Err(SpeechError::Cancelled)
    ));
}

#[test]
fn api_errors_and_timeout_are_bounded() {
    let (engine, worker) = server(|mut stream, _| {
        let body = r#"{"error":{"message":"invalid voice"}}"#;
        write!(
            stream,
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let error = engine
        .synthesize(
            &SpeechRequest::new("Hello"),
            &mut |_: &[i16]| Ok(()),
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        SpeechError::OpenAi(crate::Error::Api { status: 400, .. })
    ));
    worker.join().unwrap();
    let (engine, worker) = server(|_stream, _| thread::sleep(Duration::from_millis(300)));
    let start = Instant::now();
    assert!(
        engine
            .timeout(Duration::from_millis(100))
            .synthesize(
                &SpeechRequest::new("Hello"),
                &mut |_: &[i16]| Ok(()),
                &Cancellation::default()
            )
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    worker.join().unwrap();
}

#[test]
fn wav_is_finalized_and_failed_replacement_preserves_original() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("speech.wav");
    let (engine, worker) = server(|mut stream, body| {
        assert_eq!(body["voice"], "marin");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n\x01\x00\x02\x00")
            .unwrap();
    });
    let mut request = SpeechRequest::new("Hello");
    request.voice = "marin";
    synthesize_to_wav(&engine, &request, &path, false, &Cancellation::default()).unwrap();
    worker.join().unwrap();
    let mut reader = hound::WavReader::open(&path).unwrap();
    assert_eq!(reader.spec().sample_rate, 24000);
    assert_eq!(
        reader
            .samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap(),
        [1, 2]
    );
    let original = std::fs::read(&path).unwrap();
    assert!(synthesize_to_wav(&engine, &request, &path, false, &Cancellation::default()).is_err());
    let (engine, worker) = server(|mut stream, _| {
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n\x01")
            .unwrap();
    });
    assert!(synthesize_to_wav(&engine, &request, &path, true, &Cancellation::default()).is_err());
    worker.join().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
