#![cfg(feature = "workflow")]

use hear::{HearConfig, OpenAiClient, Stage, Workflow, WorkflowEvent, dictionary::Dictionary};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

fn server(responses: Vec<(u16, String)>) -> (OpenAiClient, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let start = std::time::Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            start.elapsed() < Duration::from_secs(10),
                            "missing HTTP request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0, "incomplete HTTP request");
                request.extend_from_slice(&buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let len = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")?
                                .trim()
                                .parse::<usize>()
                                .ok()
                        })
                        .unwrap();
                    if request.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(request).unwrap());
            write!(
                socket,
                "HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        requests
    });
    (
        OpenAiClient::builder("test").base_url(url).build().unwrap(),
        worker,
    )
}

#[test]
fn library_runs_full_workflow_with_dictionary_and_output_files() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("audio.wav");
    fs::write(&audio, b"test audio").unwrap();
    let raw_path = directory.path().join("raw.txt");
    let text_path = directory.path().join("text.txt");
    let saved_path = directory.path().join("saved.wav");
    let mut dictionary = Dictionary::default();
    dictionary
        .add("Qdrant", &["quadrant".into()], None)
        .unwrap();
    let (client, worker) = server(vec![
        (200, r#"{"text":"use quadrant"}"#.into()),
        (200, serde_json::json!({"output":[{"content":[{"type":"output_text","text":r#"{"kind":"plain","text":"Use Qdrant."}"#}]}]}).to_string()),
    ]);
    let stages = Arc::new(Mutex::new(Vec::new()));
    let observed = stages.clone();
    let result = Workflow::new(HearConfig {
        output: Some(text_path.clone()),
        raw_output: Some(raw_path.clone()),
        save_recording: Some(saved_path.clone()),
        ..Default::default()
    })
    .dictionary(dictionary)
    .openai_client(client)
    .progress(move |event| {
        if let WorkflowEvent::Stage(stage) = event {
            observed.lock().unwrap().push(stage);
        }
    })
    .run(&audio)
    .unwrap();
    assert_eq!(result.raw, "use Qdrant");
    assert_eq!(result.text, "Use Qdrant.");
    assert_eq!(fs::read_to_string(raw_path).unwrap(), "use Qdrant\n");
    assert_eq!(fs::read_to_string(text_path).unwrap(), "Use Qdrant.\n");
    assert_eq!(fs::read(saved_path).unwrap(), b"test audio");
    let requests = worker.join().unwrap();
    assert!(requests[0].starts_with("POST /audio/transcriptions"));
    assert!(requests[0].contains("Qdrant"));
    assert!(requests[1].starts_with("POST /responses"));
    assert!(requests[1].contains("use Qdrant"));
    assert!(stages.lock().unwrap().contains(&Stage::Polishing));
}

#[test]
fn polishing_failure_returns_raw_and_keeps_raw_output() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("audio.wav");
    fs::write(&audio, b"test audio").unwrap();
    let raw = directory.path().join("raw.txt");
    let output = directory.path().join("text.txt");
    let (client, worker) = server(vec![
        (200, r#"{"text":"keep this"}"#.into()),
        (429, r#"{"error":{"message":"quota"}}"#.into()),
    ]);
    let error = Workflow::new(HearConfig {
        raw_output: Some(raw.clone()),
        output: Some(output.clone()),
        ..Default::default()
    })
    .openai_client(client)
    .run(&audio)
    .unwrap_err();
    worker.join().unwrap();
    assert_eq!(error.stage, Stage::Polishing);
    assert_eq!(error.raw.as_deref(), Some("keep this"));
    assert!(error.text.is_none());
    assert_eq!(fs::read_to_string(raw).unwrap(), "keep this\n");
    assert!(!output.exists());
}

#[test]
fn output_race_preserves_text_and_does_not_overwrite_new_file() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("audio.wav");
    fs::write(&audio, b"test audio").unwrap();
    let output = directory.path().join("text.txt");
    let writer = output.clone();
    let (client, worker) = server(vec![(200, r#"{"text":"keep this"}"#.into())]);
    let error = Workflow::new(HearConfig {
        polish: false,
        output: Some(output.clone()),
        ..Default::default()
    })
    .openai_client(client)
    .progress(move |event| {
        if matches!(event, WorkflowEvent::Stage(Stage::Output)) {
            fs::write(&writer, "another writer").unwrap();
        }
    })
    .run(&audio)
    .unwrap_err();
    worker.join().unwrap();
    assert_eq!(error.stage, Stage::Output);
    assert_eq!(error.raw.as_deref(), Some("keep this"));
    assert_eq!(error.text.as_deref(), Some("keep this"));
    assert_eq!(fs::read_to_string(output).unwrap(), "another writer");
}

#[test]
fn preflight_rejects_input_alias_before_saving_or_transcribing() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("audio.wav");
    fs::write(&audio, b"original audio").unwrap();
    let alias = directory.path().join("alias.wav");
    fs::hard_link(&audio, &alias).unwrap();
    let error = Workflow::new(HearConfig {
        output: Some(alias),
        force: true,
        ..Default::default()
    })
    .run(&audio)
    .unwrap_err();
    assert_eq!(error.stage, Stage::Validation);
    assert_eq!(fs::read(audio).unwrap(), b"original audio");
}

#[test]
fn dictionary_api_persists_changes_without_cli_commands() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("dictionary.json");
    let mut dictionary = Dictionary::default();
    dictionary
        .add("Qdrant", &["quadrant".into()], Some("quadrant"))
        .unwrap();
    dictionary.save_to(&path).unwrap();
    let mut loaded = Dictionary::load_from(&path).unwrap();
    assert_eq!(loaded.entries()[0].term, "Qdrant");
    assert_eq!(
        loaded.correct_aliases("use quadrant").unwrap(),
        "use Qdrant"
    );
    assert!(loaded.remove("Qdrant"));
    loaded.save_to(&path).unwrap();
    assert!(Dictionary::load_from(&path).unwrap().entries().is_empty());
}
