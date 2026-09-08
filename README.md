# hear

`hear` is a personal command-line audio transcriber written in Rust. It can use
OpenAI's `gpt-transcribe`, an experimental `codex exec` workflow, or a local
whisper.cpp model.

## Build

Tagged versions are built in GitHub Actions for Apple Silicon macOS, x86-64
Linux, and ARM64 Linux. Download the archive for your platform from the
repository's Releases page and verify it against `SHA256SUMS`.

To publish a version, make sure the tag matches the version in `Cargo.toml`,
then push it:

```sh
git tag 0.1.0
git push origin 0.1.0
```

The release workflow caches Cargo dependencies and build output for subsequent
tagged builds.

To build locally instead:

```sh
cargo build --release -p hear
```

The resulting binary is `target/release/hear`. FFmpeg is required when input
audio needs conversion and when an OpenAI upload must be compressed or split.
On macOS, install it with `brew install ffmpeg`.

Linux builds need the ordinary Rust native-build toolchain plus ALSA development
headers for microphone recording. On Debian or Ubuntu:

```sh
sudo apt install build-essential cmake clang libasound2-dev pkg-config ffmpeg
GGML_NATIVE=OFF cargo build --release -p hear
```

## Usage

OpenAI is the default engine and reads `OPENAI_API_KEY`:

```sh
hear recording.mp3
hear recording.mp3 --engine gpt-transcribe
hear recording.mp3 --engine 1
```

The engine flags are optional. A Whisper `--model` or `--language` selects
Whisper automatically; Codex models require an explicit `--engine codex`, so a misspelled Whisper model cannot switch to another engine. A built-in
local `--polish-model` or GGUF path selects local polishing. Explicit
`--engine` and `--polish-engine` values override this inference.

Write plain text to a file with `-o`. Existing files are rejected unless
`--force` is present.

```sh
hear recording.m4a -o transcript.txt
hear recording.m4a -o transcript.txt --force
```

Transcripts are polished for their inferred purpose by default:

```sh
hear recording.m4a
hear --record --raw-output raw.txt
hear recording.m4a --context email
hear recording.m4a --polish-engine local
hear recording.m4a --no-polish
```

`--context` accepts `auto`, `email`, `message` (or `text`), `todo` (or
`tasks`), `notes` (or `note`), `plain`, and `verbatim`. Without an explicit
context, polishing infers the format automatically and the first spoken word
can act as a directive and is removed from the result:

```text
Email Sam, here is the proposal...  -> email
Todo buy milk and call Alex...      -> todo
Notes launch risks...               -> notes
```

An explicit `--context` takes precedence and preserves a directive-like first
word, so `--context plain` is an escape hatch for text such as "Message
received yesterday." `verbatim` removes a spoken directive but otherwise skips
the formatting request. Use `--no-polish` to bypass LLM formatting entirely.

Polishing uses `gpt-5.6-luna` by default through the OpenAI Responses API and
requires `OPENAI_API_KEY`. This means transcript text is sent to OpenAI even
when audio was transcribed locally with whisper.cpp.

Use `--polish-engine local` to polish with the built-in llama.cpp runtime
and Qwen3.5 instead. The default 2B Q4_K_M model is 1.4 GB; the smaller
0.8B model is 580 MB. Each GGUF is downloaded, checksum-verified, and stored in
the platform's standard `hear/models` cache on first use. Local polishing runs
on the CPU on Linux and Intel macOS, and uses Metal acceleration on Apple
silicon. `--polish-model` can select the default `qwen3.5-2b`, the smaller
`qwen3.5-0.8b`, or a local GGUF file path. Both built-in models use Q4_K_M
quantization. For example:

```sh
hear recording.m4a --engine whisper --polish-engine local
hear recording.m4a --model small.en --polish-model qwen3.5-0.8b
hear recording.m4a --polish-engine local --polish-model qwen3.5-0.8b
hear recording.m4a --polish-engine local --polish-model /models/custom.gguf
```

The `hear` executable contains both Whisper transcription and local polishing;
no separate polishing executable is needed. Both engines link against one static
GGML build provided by the exactly pinned `llama-cpp-sys-2` dependency. The
patched Whisper binding in `vendor/whisper-rs-sys` uses that build and its headers.
See [native build notes](vendor/whisper-rs-sys/README.md) before upgrading either
native dependency.

Use `--no-polish` to skip formatting entirely. Use `--raw-output PATH` to keep
the original transcript alongside the formatted result.

## Personal dictionary

Save names and domain terms that should be spelled consistently:

```sh
hear dictionary add "Flaport" --sounds-like "flah-port"
hear dictionary add "Qdrant" --alias "quadrant" --alias "Q drant"
hear dictionary list
hear dictionary remove "Qdrant"
```

The dictionary is stored as `hear/dictionary.json` in the platform's standard
user configuration directory. Canonical terms are supplied to every
transcription engine as vocabulary hints. Aliases are corrected as whole words
after transcription, whether or not `--polish` is enabled.

When polishing is enabled, aliases and `--sounds-like` pronunciation notes are
also given to the formatter. Alias correction is deterministic; matching a
pronunciation note is an LLM judgment and may be less reliable. Adding an
existing canonical term updates it by merging new aliases and replacing the
pronunciation when a new one is supplied.

Transcribe locally with whisper.cpp:

```sh
hear recording.mp3 --engine whisper
hear recording.mp3 --engine 3 --model small.en
hear recording.mp3 --engine whisper --model large-v3-turbo --language de
hear recording.mp3 --engine whisper --model large-v3-turbo --language auto
```

Supported model names are `tiny.en` (the fast default), `base.en`, `small.en`,
`medium.en`, and `large-v3-turbo`. Models download automatically on first use
to the platform's standard user cache directory (`hear/models`). macOS builds
enable whisper.cpp's Metal backend; Linux uses CPU inference. Downloads are
pinned to a specific upstream revision and checked against their expected size
and SHA-256 digest before installation and reuse.

Whisper defaults to English. The `.en` models only accept English;
`large-v3-turbo` also accepts `--language LANGUAGE` with a language code or
`--language auto` for automatic detection.

Record from the default microphone, then press Return to transcribe:

```sh
hear --record
hear --record --engine whisper
hear --record --save-recording message.wav
```

Unless `--save-recording` is supplied, the normalized 16 kHz mono WAV recording
is deleted after successful transcription. Failed jobs retain their recording in
the temporary directory and print its path for retry. Progress and warnings go to stderr; the
transcript alone goes to stdout or the requested output file.
Ctrl-C cancels recording without saving or transcribing and exits with status
130. Recording requires an interactive terminal so Return can be detected.

## macOS menu-bar companion

The separate [`hear-macos`](crates/hear-macos) workspace crate provides a
pure-Rust menu-bar application. Press Option-X once to start recording and
again to stop. It invokes a bundled `hear` helper, copies successful transcripts
to the clipboard, and can paste them into the active application when macOS
Accessibility access is granted.

Build an ad-hoc-signed development application with:

```sh
crates/hear-macos/bundle.sh
open dist/Hear.app
```

See the companion crate's README for Keychain setup and permission details.
The app reads engine, transcription model, polishing model, context, and paste
defaults from `~/Library/Application Support/hear-app/config.toml`.

## Linux system-tray companion

The separate [`hear-linux`](crates/hear-linux) workspace crate provides a
`hear-app` system-tray application for Linux desktops. Press Alt-X once to start
recording and again to stop. It invokes a sibling `hear` binary, copies
successful transcripts to the clipboard, and can paste them into the active
application. Clipboard persistence uses `xclip` (X11) or `wl-copy` (Wayland);
automatic paste uses `xdotool` on X11 when the original window remains focused.
Wayland delivery copies to the clipboard.

Install both binaries and a `.desktop` launcher entry with:

```sh
crates/hear-linux/install.sh
```

See the companion crate's README for keyring setup and paste details.
The Linux binary also provides a `oneshot` mode for bindings managed by SXHKD
or another external hotkey daemon; invoke it once to record and again to stop.

## Experimental Codex engine

```sh
hear recording.wav --engine codex
hear recording.wav --engine 2 --model MODEL
```

This runs an ephemeral, read-only `codex exec` session with network access and a
fixed prompt. `OPENAI_API_KEY` is deliberately removed from the child process,
and recursively invoking `hear` is forbidden. Codex models do not accept audio
directly, and Codex session credentials do not grant access to the transcription
API, so this engine only succeeds if Codex can discover another usable
transcription facility. Its final result must explicitly distinguish a transcript
from an error; explanations of failure are never treated as transcripts. It is
intentionally best-effort.

## OpenAI upload behavior

Supported files of at most 25 MB are uploaded directly. Other formats are
converted with FFmpeg. Files over the limit—including files whose converted
form crosses it—produce a warning, then are compressed and split into 45-minute
MP3 parts before sequential transcription.

## Rust library

`hear` can be embedded without its microphone, CLI, or local Whisper dependencies:

```toml
hear = { git = "https://github.com/flaport/hear", tag = "0.5.0", default-features = false }
```

```rust
let transcript = hear::transcribe_openai(
    std::path::Path::new("note.webm"),
    &[],
    true,
    Some(hear::FormatContext::Notes),
    None,
)?;
println!("{}", transcript.text);
```

For new integrations, named options avoid positional configuration and support
additional formatting instructions:

```rust,no_run
let vocabulary = vec!["Qdrant".to_owned()];
let options = hear::TranscriptionOptions::new()
    .vocabulary(&vocabulary)
    .polish(
        hear::PolishOptions::new()
            .context(hear::FormatContext::Notes)
            .instruction("Use short headings."),
    );
let transcript = hear::transcribe_openai_with_options(
    std::path::Path::new("meeting.m4a"),
    &options,
)?;
println!("{}", transcript.text);
```

Use `transcribe_openai_raw` when only the raw transcript is needed, and
`polish_with_options` to format text that has already been transcribed. The
older positional functions remain available for compatibility.

The library reads `OPENAI_API_KEY` from the environment. The default `cli`
feature builds the full `hear` binary with recording and local Whisper and
includes the `local-polish` feature. That feature exposes
`polish_local_with_options`, which runs local inference in the calling process.
The `hear-local-polish` workspace crate is an internal Rust library, not an
installed executable. Local inference keeps its backend initialized across calls;
models and inference contexts are released after each request.

## Complete library workflow

The `workflow` feature exposes the same engines, dictionary corrections, polishing,
and file-output behavior as the CLI, without enabling argument parsing or terminal
signal handlers. `Workflow::run` returns both raw and formatted text; failures
include their stage and any text already produced. Supply an `OpenAiClient` for
explicit credentials and transport settings, or let OpenAI operations read the
environment. Dictionary loading is explicit for library callers.

```rust,no_run
use hear::{Engine, HearConfig, PolishEngine, Workflow, dictionary::Dictionary};

let workflow = Workflow::new(HearConfig {
    engine: Some(Engine::Whisper),
    polish_engine: Some(PolishEngine::Local),
    ..HearConfig::default()
})
.dictionary(Dictionary::load()?);

let transcript = workflow.run(std::path::Path::new("meeting.wav"))?;
println!("{}", transcript.text);
# Ok::<(), Box<dyn std::error::Error>>(())
```

For a dependency on a source checkout, use `default-features = false` and
`features = ["workflow"]`. The native Whisper dependencies are linked directly;
consumer projects do not need workspace patches. `tests/check-library-consumer.sh`
checks this from a separate Cargo project.

`Recorder::start`, `check`, `stop`, and `finish` expose microphone capture without
requiring a terminal. Pass the completed recording's path to `Workflow::run`.
The `capture` feature also exposes this API without the native inference engines.
`Dictionary` supports loading, adding, listing, removing, and saving entries.
`write_transcript` exposes the same file overwrite policy used by the workflow.
The CLI owns terminal interaction, signal handling, argument parsing, and display.

## Shared workflow and failure recovery

`hear-core` owns typed engine/model configuration, path identity checks, bounded
microphone capture, subprocess management, and the app/helper response protocol.
The platform apps own their UI, credential storage, clipboard, and focus checks.
`hear::Workflow` owns engine dispatch, dictionary corrections, polishing, output
files, and partial-result errors. Whisper and local polishing use the same native
GGML runtime.
Desktop apps remain separate executables and run the CLI as a managed child
process, retaining their cancellation and deadline handling.

Built-in cached models are checked against the catalog size and SHA-256 before
each load. Old checksum sidecars are ignored: matching timestamps alone cannot
prove that the model contents are unchanged.

Recording downmixes fixed-size packets and resamples them into a WAV on a worker.
The packet queue is bounded; a microphone or writer error invalidates the
recording instead of delivering incomplete audio. Application helpers have a
one-hour deadline and are cancelled when the app exits normally.

The apps retain failed recordings, plus available raw or formatted text, in the
OS temporary directory. The error reports the audio path; retry it with
`hear /path/to/hear-recording-XXXX.wav`. Successfully delivered recordings are
removed. Recovery files remain until manually removed or cleaned by the OS.
Use `[hear].save_recording` for a permanent copy on either platform.

Automatic paste requires the original macOS application or X11 window still to
be focused. These checks do not track changes to individual text fields. A changed
or unverifiable application/window leaves the text on the clipboard. The Linux tray
uses X11/XEmbed. On Wayland, use `hear-app oneshot` from a compositor-managed
shortcut; delivery copies to the clipboard because focused clients cannot be
verified through a compositor-independent API.

`hear --json AUDIO` is the app protocol: stdout contains one JSON object with
`status = "success"`, `raw`, and `text`, or `status = "failure"`, `phase`,
`message`, and an optional `raw` result. Failures also return a nonzero exit code.
The ordinary CLI continues to emit plain text.

## Configurable library client

The convenience functions remain available. Embedders can reuse an explicit
blocking client without changing process environment variables:

```rust,no_run
use std::{path::Path, time::Duration};
let client = hear::OpenAiClient::builder("api-key")
    .connect_timeout(Duration::from_secs(15))
    .request_timeout(Some(Duration::from_secs(900)))
    .progress(|event| eprintln!("{event:?}"))
    .build()?;
let transcript = client.transcribe(
    Path::new("meeting.wav"),
    &hear::TranscriptionOptions::new().polish(hear::PolishOptions::new()),
)?;
println!("{}", transcript.text);
# Ok::<(), hear::Error>(())
```

The default connection timeout is 15 seconds and the request timeout is 15
minutes. `request_timeout(None)` disables the request deadline. Progress is
silent unless a callback is supplied. Run blocking operations on a worker when
embedding in an asynchronous application. Errors distinguish configuration,
transport, API status, input, and response failures; `Error::Polishing` retains
the successful raw transcript and the underlying error.
