#![cfg(all(feature = "cli", unix))]

use std::{fs, process::Command, time::Duration};

#[test]
#[ignore = "requires HEAR_TEST_AUDIO (mono PCM16 16 kHz speech, at least four seconds) and tiny.en"]
fn streaming_previews_arrive_before_eof_and_preserve_final_output() {
    use std::{
        io::{Read, Write},
        process::Stdio,
        sync::mpsc,
        time::Instant,
    };
    let audio = std::env::var_os("HEAR_TEST_AUDIO").expect("set HEAR_TEST_AUDIO");
    let mut wav = hound::WavReader::open(audio).unwrap();
    assert_eq!(wav.spec().channels, 1);
    assert_eq!(wav.spec().sample_rate, 16000);
    assert_eq!(wav.spec().bits_per_sample, 16);
    let pcm: Vec<u8> = wav
        .samples::<i16>()
        .map(Result::unwrap)
        .flat_map(i16::to_le_bytes)
        .collect();
    let first = 4 * 16000 * 2;
    assert!(pcm.len() >= first);

    for window in ["5", "12"] {
        let args = [
            "--stream",
            "--pcm-stdin",
            "--json",
            "--engine",
            "whisper",
            "--model",
            "tiny.en",
            "--no-polish",
            "--stream-window",
            window,
        ];
        let baseline = hear_core::process::run(
            Command::new(env!("CARGO_BIN_EXE_hear")).args(args),
            Some(pcm.clone()),
            &hear_core::process::Cancellation::default(),
            Duration::from_secs(60),
            false,
        )
        .unwrap();
        assert!(
            baseline.status.success(),
            "{}",
            String::from_utf8_lossy(&baseline.stderr)
        );
        assert!(!String::from_utf8_lossy(&baseline.stderr).contains("[live]"));

        let mut child = Command::new(env!("CARGO_BIN_EXE_hear"))
            .args(args)
            .arg("--live-transcript")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            while let Ok(n) = stderr.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                if sender.send(buffer[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let (out_sender, out_receiver) = mpsc::channel();
        let out_reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).unwrap();
            out_sender.send(bytes).unwrap();
        });
        input.write_all(&pcm[..first]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut diagnostics = Vec::new();
        while !String::from_utf8_lossy(&diagnostics).contains("[live] ") {
            match receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(bytes) => diagnostics.extend(bytes),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!(
                        "no preview before EOF: {error}; {}",
                        String::from_utf8_lossy(&diagnostics)
                    );
                }
            }
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "helper must still be accepting audio"
        );
        assert!(
            out_receiver.try_recv().is_err(),
            "final output must wait for EOF"
        );
        input.write_all(&pcm[first..]).unwrap();
        drop(input);
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("helper did not finish after EOF");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        reader.join().unwrap();
        out_reader.join().unwrap();
        assert!(status.success());
        let live: serde_json::Value =
            serde_json::from_slice(&out_receiver.recv().unwrap()).unwrap();
        let baseline: serde_json::Value = serde_json::from_slice(&baseline.stdout).unwrap();
        assert_eq!(
            live, baseline,
            "previews must not change the final result for window={window}"
        );
    }
}

#[test]
#[ignore = "requires HEAR_TEST_AUDIO and downloads/runs both native models"]
fn standalone_binary_runs_both_native_engines() {
    let audio = std::env::var_os("HEAR_TEST_AUDIO").expect("set HEAR_TEST_AUDIO to an English WAV");
    let audio = fs::canonicalize(audio).unwrap();
    let expected = std::env::var("HEAR_TEST_EXPECTED").unwrap_or_else(|_| "hello".into());
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("hear");
    fs::copy(env!("CARGO_BIN_EXE_hear"), &binary).unwrap();
    let output = hear_core::process::run(
        Command::new(binary)
            .current_dir(directory.path())
            .arg(audio)
            .args([
                "--engine",
                "whisper",
                "--model",
                "tiny.en",
                "--polish-engine",
                "local",
                "--polish-model",
                "qwen3.5-0.8b",
                "--context",
                "plain",
                "--json",
            ])
            .env("PATH", "")
            .env_remove("OPENAI_API_KEY"),
        None,
        &hear_core::process::Cancellation::default(),
        Duration::from_secs(900),
        true,
    )
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["status"], "success");
    for field in ["raw", "text"] {
        let text = result[field].as_str().unwrap();
        assert!(
            text.to_lowercase().contains(&expected.to_lowercase()),
            "{field}: {text}"
        );
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}
