# hear

`hear` is a Rust library and CLI for recording, transcribing, and polishing audio
transcripts. Use OpenAI or run transcription and polishing locally.

## Install

Download binaries for Apple Silicon macOS, x86-64 Linux, or ARM64 Linux from
[Releases](https://github.com/flaport/hear/releases), or build from source:

```sh
cargo build --release -p hear
```

The binary is `target/release/hear`. FFmpeg is needed for audio conversion and
large OpenAI uploads. On macOS, install it with `brew install ffmpeg`.
On Debian or Ubuntu, install build dependencies and FFmpeg first:

```sh
sudo apt install build-essential cmake clang libasound2-dev pkg-config ffmpeg
GGML_NATIVE=OFF cargo build --release -p hear
```

## Usage

OpenAI transcription (`gpt-transcribe`) and polishing (`gpt-5.6-luna`) are the
defaults and require `OPENAI_API_KEY`.

```sh
hear recording.mp3
hear recording.m4a -o transcript.txt
hear --record
hear --record --save-recording message.wav
hear recording.m4a --no-polish
```

Recording uses the default microphone and requires an interactive terminal:
press Return to transcribe or Ctrl-C to cancel. Successful recordings are deleted
unless saved; failed transcription or delivery retains the audio and prints its
path for retry.

Transcripts go to stdout or `-o PATH`; progress goes to stderr. Existing output
files require `--force`. Use `--raw-output PATH` to save text after dictionary
corrections but before polishing; it requires polishing to be enabled.

Supported OpenAI uploads up to 25 MB are sent directly. Other formats are
converted with FFmpeg; oversized uploads are compressed and split into
45-minute MP3 parts.

## Streaming microphone transcription

`--stream` implies `--record`: transcription begins while you speak. Press Return
to finish, then Hear polishes and delivers the completed transcript once.

```sh
# OpenAI realtime (requires OPENAI_API_KEY)
hear --stream --engine gpt-transcribe --model gpt-live-transcribe

# Local Whisper transcription; add --polish-engine local for local polishing
hear --stream --engine whisper --model tiny.en

# Compare transcription latency without polishing, and retain the test recording
hear --stream --model tiny.en --no-polish --save-recording trial.wav
```

With `--stream`, the OpenAI engine defaults to `gpt-live-transcribe`; ordinary
file transcription and `--record` continue to use `gpt-transcribe`. These are
different models with different pricing and accuracy. OpenAI sends PCM through
a realtime WebSocket and commits bounded turns during long dictations. Local
Whisper keeps its model loaded for the recording and processes rolling windows,
retaining unfinished audio for the next window. Streaming can change accuracy;
compare the result with `hear trial.wav --model tiny.en --no-polish`.

`--stream` conflicts with an audio-file argument and does not support Codex.
Explicit `--record --stream` is accepted. Audio is also saved to a recovery WAV;
connection errors or a full audio queue fail instead of delivering a truncated
transcript. Failed recordings can be retried with the ordinary file workflow.
First-use model downloads may exceed the streaming queue's 30-second headroom;
prepare a Whisper model with a file transcription before a long first recording.
Polishing still runs after recording stops. The desktop apps use `stream = true`
inside their existing `[hear]` configuration and must be restarted after edits.

OpenAI's realtime protocol is documented at
https://developers.openai.com/api/docs/guides/realtime-transcription.

Library users with the `workflow` feature can call `Workflow::run_streaming`
with a reader of mono PCM16 little-endian audio at 16 kHz. Set `HearConfig.stream`
to `true`; the caller owns capture, cancellation and saving recovery audio.
Adapters share the `hear::streaming::Adapter` interface. Desktop companions send
PCM to an isolated helper process so cancellation can stop native inference or
network operations without blocking the microphone callback.

## Local transcription and polishing

```sh
# Fully local transcription and polishing
hear recording.m4a --engine whisper --polish-engine local

# Smaller polishing model; engines are inferred from model names
hear recording.m4a --model small.en --polish-model qwen3.5-0.8b

# Multilingual transcription, without polishing
hear recording.m4a --model large-v3-turbo --language de --no-polish
```

Whisper models: `tiny.en` (default), `base.en`, `small.en`, `medium.en`, and
`large-v3-turbo`. English is the default language. Only `large-v3-turbo` supports
other languages and automatic detection with `--language auto`; `.en` models
interpret `auto` as English.

Local polishing models: `qwen3.5-2b` (default, 1.4 GB) and `qwen3.5-0.8b`
(580 MB), both Q4_K_M. `--polish-model` also accepts a compatible GGUF file path.
Built-in models download to the platform's `hear/models` cache and are
checksum-verified before use. Both local engines are included in the binary.

A supported Whisper model or `--language` selects Whisper; a local polishing
model or GGUF path selects local polishing. Explicit engine flags take precedence.
**Local transcription still sends text to OpenAI for default polishing.** Choose
`--polish-engine local` or `--no-polish` to keep transcripts local.

## Formatting

```sh
hear recording.m4a --context email
hear recording.m4a --context verbatim
```

Contexts: `auto` (default), `email`, `message`/`text`, `todo`/`tasks`,
`notes`/`note`, `plain`, and `verbatim`.

In automatic mode, a recognized first word such as “Email”, “Todo”, or “Notes”
selects the format and is removed. A spoken “Verbatim” skips formatting after
removing that directive. Explicit contexts other than `auto` preserve the first
word; `--context verbatim` skips formatting too. `--no-polish` skips both
formatting and directive detection. Dictionary corrections still apply.

## Personal dictionary

```sh
hear dictionary add "Flaport" --sounds-like "flah-port"
hear dictionary add "Qdrant" --alias "quadrant" --alias "Q drant"
hear dictionary list
hear dictionary remove "Qdrant"
```

Entries live in `hear/dictionary.json` under the platform's user configuration
directory. Canonical terms guide transcription; whole-word aliases are corrected
even with `--no-polish`. Pronunciation notes guide polishing but are not
deterministic replacements.

## Rust library

The library exposes the CLI's recording, transcription, dictionary, polishing,
and file-output capabilities. The CLI adds argument parsing, terminal interaction,
signal handling, and display.

Enable the complete library without the CLI adapter:

```toml
hear = { git = "https://github.com/flaport/hear", branch = "main", default-features = false, features = ["workflow"] }
```

```rust,no_run
use hear::{Engine, HearConfig, PolishEngine, Workflow, dictionary::Dictionary};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workflow = Workflow::new(HearConfig {
        engine: Some(Engine::Whisper),
        polish_engine: Some(PolishEngine::Local),
        ..HearConfig::default()
    })
    .dictionary(Dictionary::load()?);

    let transcript = workflow.run(std::path::Path::new("meeting.wav"))?;
    println!("{}", transcript.text);
    Ok(())
}
```

`Workflow::run` returns raw and polished text; failures retain their stage and
available text. Dictionary loading is explicit. `Recorder::start()` and
`finish()` capture audio without a terminal; pass the resulting path to the
workflow. These APIs are blocking.

For smaller builds, disable default features and choose only what you need:

| Features | Available functionality |
| --- | --- |
| None | OpenAI transcription and polishing |
| `capture` | OpenAI APIs and microphone recording |
| `local-polish` | OpenAI APIs and local polishing |
| `workflow` | All engines, recording, dictionary, and file output |
| `cli` (default) | Complete library and CLI binary |

The minimal build omits microphone and native inference dependencies.
`transcribe_openai_with_options` and `polish_with_options` provide the OpenAI APIs;
`OpenAiClient` accepts explicit credentials, timeouts, and progress callbacks.
Otherwise, OpenAI operations read `OPENAI_API_KEY`. `TranscriptionOptions::new()`
requires `.polish(...)` to enable polishing; `HearConfig::default()` enables it.
The OpenAI convenience functions do not apply dictionary corrections.

The dependency above tracks `main`; use `rev` instead of `branch` to pin a commit.
Generate API documentation with `cargo doc --no-deps --open`. See the
[native build notes](vendor/whisper-rs-sys/README.md) before upgrading native dependencies.

## Desktop companions

[macOS menu-bar app](crates/hear-macos/README.md): Option-X starts and stops recording.

```sh
crates/hear-macos/bundle.sh
open dist/Hear.app
```

[Linux tray app](crates/hear-linux/README.md): Alt-X starts and stops recording.

```sh
crates/hear-linux/install.sh
```

Both apps copy transcripts and can paste into the original application if it is
still focused. For Wayland, bind `hear-app oneshot` to a compositor shortcut;
delivery copies to the clipboard. The companion READMEs cover configuration,
credentials, permissions, and recovery.
