#![cfg(all(feature = "cli", unix))]
use std::{fs, os::unix::fs::PermissionsExt, process::Command};

fn run_mock_codex(reply: &str, extra: &[&str]) -> (std::process::Output, tempfile::TempDir) {
    let directory = tempfile::tempdir().unwrap();
    let mock = directory.path().join("codex");
    // Pass response contents through a file; shell arguments never interpolate transcript text.
    fs::write(directory.path().join("reply.json"), reply).unwrap();
    fs::write(&mock, "#!/bin/sh\ni=0; while [ $i -lt 3000 ]; do echo abcdefghijklmnopqrstuvwxyz; echo abcdefghijklmnopqrstuvwxyz >&2; i=$((i+1)); done\nwhile [ $# -gt 0 ]; do\nif [ \"$1\" = --output-last-message ]; then shift; cp \"$HEAR_TEST_REPLY\" \"$1\"; exit; fi\nshift\ndone\nexit 1\n").unwrap();
    fs::set_permissions(&mock, fs::Permissions::from_mode(0o700)).unwrap();
    let audio = directory.path().join("audio.wav");
    fs::write(&audio, b"mock audio").unwrap();
    let mut paths = vec![directory.path().to_path_buf()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let output = hear_core::process::run(
        Command::new(env!("CARGO_BIN_EXE_hear"))
            .args(["--engine", "codex", "--json"])
            .args(extra)
            .arg(audio)
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HEAR_TEST_REPLY", directory.path().join("reply.json"))
            .env_remove("HEAR_CODEX_ACTIVE")
            .env_remove("OPENAI_API_KEY"),
        None,
        &hear_core::process::Cancellation::default(),
        // First-use Metal initialization can compile its embedded shaders.
        std::time::Duration::from_secs(60),
        false,
    )
    .unwrap();
    (output, directory)
}
#[test]
fn success_and_failure_are_distinct_machine_results() {
    let (output, _) = run_mock_codex(
        r#"{"text":"zzHearRegression","error":null}"#,
        &["--no-polish"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "success");
    assert_eq!(value["text"], "zzHearRegression");
    let (output, _) = run_mock_codex(r#"{"text":null,"error":"unavailable"}"#, &["--no-polish"]);
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "failure");
    assert_eq!(value["phase"], "transcription");
}
#[test]
fn polishing_failure_retains_raw_transcript_in_protocol() {
    let (output, _) = run_mock_codex(r#"{"text":"zzHearRegression","error":null}"#, &[]);
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["phase"], "polishing");
    assert_eq!(value["raw"], "zzHearRegression");
}
#[test]
fn hard_link_output_is_rejected_before_transcription() {
    let directory = tempfile::tempdir().unwrap();
    let audio = directory.path().join("audio.wav");
    let output = directory.path().join("transcript.txt");
    fs::write(&audio, b"preserve audio").unwrap();
    fs::hard_link(&audio, &output).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_hear"))
        .arg(&audio)
        .arg("--output")
        .arg(&output)
        .args(["--force", "--json"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(audio).unwrap(), b"preserve audio");
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["phase"], "validation");
}

#[test]
fn local_polishing_failure_retains_raw_transcript() {
    let model = tempfile::Builder::new().suffix(".gguf").tempfile().unwrap();
    fs::write(model.path(), b"invalid model").unwrap();
    let (output, _) = run_mock_codex(
        r#"{"text":"zzHearRegression","error":null}"#,
        &[
            "--polish-engine",
            "local",
            "--polish-model",
            model.path().to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["phase"], "polishing", "{value}");
    assert_eq!(value["raw"], "zzHearRegression");
    assert!(
        value["message"]
            .as_str()
            .unwrap()
            .contains("could not load local model")
    );
}
