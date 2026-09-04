# hear

`hear` is a personal command-line audio transcriber written in Rust. It can use
OpenAI's `gpt-transcribe`, an experimental `codex exec` workflow, or a local
whisper.cpp model.

## Build

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

Polish a transcript for its inferred purpose after transcription:

```sh
hear recording.m4a --polish
hear --record --polish --raw-output raw.txt
hear recording.m4a --context email
```

`--context` accepts `auto`, `email`, `message` (or `text`), `todo` (or
`tasks`), `notes` (or `note`), `plain`, and `verbatim`. Supplying a context
also enables polishing. Without an explicit context, the first spoken word can
act as a directive and is removed from the result:

```text
Email Sam, here is the proposal...  -> email
Todo buy milk and call Alex...      -> todo
Notes launch risks...               -> notes
```

An explicit `--context` takes precedence and preserves a directive-like first
word, so `--context plain` is an escape hatch for text such as "Message
received yesterday." `verbatim` removes a spoken directive but otherwise skips
the formatting request.

Polishing uses `gpt-5.4-mini` through the OpenAI Responses API and requires
`OPENAI_API_KEY`. This means transcript text is sent to OpenAI even when audio
was transcribed locally with whisper.cpp. Use `--raw-output PATH` with
`--polish` to keep the original transcript alongside the formatted result.

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
```

Supported model names are `tiny.en` (the fast default), `base.en`, `small.en`,
`medium.en`, and `large-v3-turbo`. Models download automatically on first use
to the platform's standard user cache directory (`hear/models`). macOS builds
enable whisper.cpp's Metal backend; Linux uses CPU inference.

Record from the default microphone until Ctrl-C, then transcribe:

```sh
hear --record
hear --record --engine whisper
hear --record --save-recording message.wav
```

Unless `--save-recording` is supplied, the normalized 16 kHz mono WAV recording
is deleted after transcription. Progress and warnings go to stderr; the
transcript alone goes to stdout or the requested output file.

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
converted with FFmpeg. Larger files produce a warning, then are compressed and
split into 45-minute MP3 parts before sequential transcription.
