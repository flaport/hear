#![cfg(all(feature = "cli", unix))]

use std::{fs, process::Command, time::Duration};

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
