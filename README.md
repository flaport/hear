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
cargo build --release
```

The resulting binary is `target/release/hear`. FFmpeg is required when input
audio needs conversion and when an OpenAI upload must be compressed or split.
On macOS, install it with `brew install ffmpeg`.

Linux builds need the ordinary Rust native-build toolchain plus ALSA development
headers for microphone recording. On Debian or Ubuntu:

```sh
sudo apt install build-essential cmake clang libasound2-dev pkg-config ffmpeg
```

## Usage

OpenAI is the default engine and reads `OPENAI_API_KEY`:

```sh
hear recording.mp3
hear recording.mp3 --engine gpt-transcribe
hear recording.mp3 --engine 1
```

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

Use `--polish-engine local` to polish with the bundled `hear-local-polish`
helper and Qwen3.5-2B instead. Its 1.4 GB Q4_K_M GGUF file is downloaded,
checksum-verified, and stored in the platform's standard `hear/models` cache
on first use. Local polishing runs on the CPU on Linux and Intel macOS, and
uses Metal acceleration on Apple silicon. `--polish-model` can select the
built-in `qwen3.5-2b` model or a local GGUF file path. For example:

```sh
hear recording.m4a --engine whisper --polish-engine local
hear recording.m4a --polish-engine local --polish-model /models/custom.gguf
```

The helper is a separate process because whisper.cpp and llama.cpp each vendor
GGML; isolating them prevents duplicate native symbols in the main binary. Its
`llama-cpp-2` dependency is pinned exactly so GGUF compatibility changes are
intentional upgrades.

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
is deleted after transcription. Progress and warnings go to stderr; the
transcript alone goes to stdout or the requested output file.
Ctrl-C cancels recording without saving or transcribing and exits with status
130. Recording requires an interactive terminal so Return can be detected.

## macOS menu-bar companion

The separate [`hear-macos`](crates/hear-macos) workspace crate provides a
pure-Rust menu-bar application. Press Option-Space once to start recording and
again to stop. It invokes a bundled `hear` helper, copies successful transcripts
to the clipboard, and can paste them into the active application when macOS
Accessibility access is granted.

Build an ad-hoc-signed development application with:

```sh
crates/hear-macos/bundle.sh
open dist/Hear.app
```

See the companion crate's README for Keychain setup and permission details.

## Linux system-tray companion

The separate [`hear-linux`](crates/hear-linux) workspace crate provides a
`hear-app` system-tray application for Linux desktops. Press Alt-X once to start
recording and again to stop. It invokes a sibling `hear` binary, copies
successful transcripts to the clipboard, and can paste them into the active
application. Clipboard persistence uses `xclip` (X11) or `wl-copy` (Wayland);
automatic paste uses `xdotool` (X11) or `wtype` (Wayland).

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
transcription facility. It is intentionally best-effort.

## OpenAI upload behavior

Supported files of at most 25 MB are uploaded directly. Other formats are
converted with FFmpeg. Files over the limit—including files whose converted
form crosses it—produce a warning, then are compressed and split into 45-minute
MP3 parts before sequential transcription.

## Rust library

`hear` can be embedded without its microphone, CLI, or local Whisper dependencies:

```toml
hear = { git = "https://github.com/flaport/hear", tag = "0.4.0", default-features = false }
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
includes the `local-polish` client feature. That feature exposes
`polish_local_with_options`, which locates `hear-local-polish` beside the
current executable or on `PATH`.
