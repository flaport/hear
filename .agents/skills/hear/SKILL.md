---
name: hear
description: Transcribe audio files or microphone recordings with the hear CLI, format transcripts, and manage hear's pronunciation dictionary. Use for speech-to-text and transcript requests involving hear; do not use for speech synthesis or audio generation.
---

# Use hear

Use the user's instructions as the source of truth for engine, privacy, model,
format, and output choices.

## Run the CLI

- Prefer an installed `hear` executable. In the hear source repository, use
  `cargo run --quiet -- <arguments>` when `hear` is not installed.
- Quote audio and output paths. Confirm the input exists before starting a
  potentially expensive transcription.
- Send the transcript to stdout unless the user requests a file. Use
  `--output PATH` for a file. Never add `--force` unless overwriting that exact
  path is authorized.
- Do not pass the same path as the audio input, transcript output, raw output,
  or saved recording.

## Choose an engine

- `hear AUDIO` or `hear AUDIO --engine gpt-transcribe` is the default and
  requires `OPENAI_API_KEY`. It uploads audio to OpenAI.
- `hear AUDIO --engine whisper` transcribes locally. It defaults to `tiny.en`;
  `base.en`, `small.en`, `medium.en`, and `large-v3-turbo` are also supported.
  Models download on first use. Respect a user-selected model and do not assume
  that a larger model is worth its download and runtime cost.
- Avoid `--engine codex` when running inside a coding agent: it launches a new
  best-effort `codex exec` session instead of a direct transcription backend.
  Use it only when the user explicitly requests that engine.

Polishing is enabled by default and uses OpenAI, even after local Whisper
transcription. For a fully local workflow, use both `--engine whisper` and
`--no-polish`. Check whether `OPENAI_API_KEY` exists without printing its value
before selecting a workflow that needs it.

## Control the result

- Use `--context email|message|todo|notes|plain|verbatim` when the desired form
  is known. Otherwise let hear infer it.
- Use `--no-polish` for the raw transcript with no LLM formatting.
- Use `--raw-output PATH` to retain the raw transcript alongside a polished
  result. It cannot be combined with `--no-polish`.
- Automatic formatting may treat an initial spoken word such as “Email,”
  “Todo,” or “Notes” as a directive and remove it. An explicit context preserves
  directive-like text, except `verbatim`, which removes a spoken directive but
  otherwise skips formatting.

Examples:

```sh
hear "meeting.m4a" --context notes
hear "interview.wav" --engine whisper --no-polish --output "interview.txt"
hear "memo.m4a" --raw-output "memo-raw.txt" --output "memo.txt"
```

## Record audio

Use `hear --record` only in an interactive terminal. Recording stops when
Return is pressed; Ctrl-C cancels with status 130. Add
`--save-recording PATH` only when the user wants to keep the normalized WAV.

## Manage the dictionary

Only mutate the personal dictionary when requested. Canonical spellings and
aliases affect later transcriptions.

```sh
hear dictionary add "Flaport" --sounds-like "flah-port"
hear dictionary add "Qdrant" --alias "quadrant" --alias "Q drant"
hear dictionary list
hear dictionary remove "Qdrant"
```

## Handle prerequisites and failures

- FFmpeg is required for unsupported upload formats, files larger than 25 MB,
  and other audio conversion. Report the missing prerequisite if hear requests
  it; do not silently install software unless installation is in scope.
- Progress and warnings are written to stderr; stdout contains only the
  transcript. Treat a nonzero exit as failure and report hear's error rather
  than presenting partial progress as a transcript.
