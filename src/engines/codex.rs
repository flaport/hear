use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{Context, Result, bail};

pub fn transcribe(input: &Path, model: Option<&str>) -> Result<String> {
    if std::env::var_os("HEAR_CODEX_ACTIVE").is_some() {
        bail!("the codex engine cannot recursively invoke hear");
    }

    let input = input
        .canonicalize()
        .with_context(|| format!("could not resolve audio path: {}", input.display()))?;
    let working_directory = input
        .parent()
        .context("audio file has no parent directory")?;
    let result_file = tempfile::Builder::new()
        .prefix("hear-codex-result-")
        .tempfile()
        .context("could not create a temporary Codex result file")?;

    let prompt = format!(
        "Transcribe the spoken audio in the file at {path}. Return only the plain-text \
         transcript in your final response: no Markdown, commentary, timestamps, or speaker \
         labels. This is a best-effort task: you may use the network and already-installed \
         tools, but you must not invoke the `hear` command, modify the input file, or modify \
         the working directory. If transcription is impossible, explain why in the final \
         response instead of fabricating a transcript.",
        path = input.display()
    );

    let mut command = Command::new("codex");
    command
        .arg("exec")
        .args([
            "--ephemeral",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "--color",
            "never",
            "--output-last-message",
        ])
        .arg(result_file.path())
        .args([
            "--config",
            "sandbox_permissions=[\"disk-full-read-access\",\"network-full-access\"]",
        ])
        .arg("--cd")
        .arg(working_directory)
        .env_remove("OPENAI_API_KEY")
        .env("HEAR_CODEX_ACTIVE", "1");
    if let Some(model) = model {
        command.args(["--model", model]);
    }
    command.arg(prompt);

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            anyhow::anyhow!(
                "the codex executable was not found; install and authenticate Codex before using --engine codex/2"
            )
        } else {
            anyhow::anyhow!(error).context("could not launch codex exec")
        }
    })?;
    let stdout = child
        .stdout
        .take()
        .context("could not capture Codex output")?;
    let stderr = child
        .stderr
        .take()
        .context("could not capture Codex errors")?;
    let stdout_thread = thread::spawn(move || forward_to_stderr(stdout));
    let stderr_thread = thread::spawn(move || forward_to_stderr(stderr));
    let status = child.wait().context("could not wait for codex exec")?;
    stdout_thread
        .join()
        .map_err(|_| anyhow::anyhow!("Codex output forwarding thread panicked"))??;
    stderr_thread
        .join()
        .map_err(|_| anyhow::anyhow!("Codex error forwarding thread panicked"))??;

    if !status.success() {
        bail!(
            "codex exec failed with {}; confirm that Codex is installed and authenticated",
            status
        );
    }

    let transcript = fs::read_to_string(result_file.path())
        .context("codex exec completed without a readable final response")?;
    let transcript = transcript.trim();
    if transcript.is_empty() {
        bail!("codex exec returned an empty final response");
    }
    Ok(transcript.to_owned())
}

fn forward_to_stderr(mut source: impl io::Read) -> Result<()> {
    let mut stderr = io::stderr().lock();
    io::copy(&mut source, &mut stderr).context("could not forward Codex progress output")?;
    stderr
        .flush()
        .context("could not flush Codex progress output")?;
    Ok(())
}
