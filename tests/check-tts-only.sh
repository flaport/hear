#!/bin/sh
set -eu
cargo test --locked --no-default-features --features tts --lib
cargo test --locked --no-default-features --features pocket-tts --lib
cargo check --locked --no-default-features --features tts --example speak
if cargo tree --locked --no-default-features --features tts --prefix none --edges normal | grep -E '^(cpal|whisper-rs|whisper-rs-sys|hear-local-polish) '; then
    echo 'TTS-only build unexpectedly includes capture or native inference' >&2
    exit 1
fi

if cargo tree --locked --no-default-features --features pocket-tts --prefix none --edges normal | grep -E '^(cpal|whisper-rs|whisper-rs-sys|hear-local-polish) '; then
    echo 'Pocket TTS unexpectedly includes capture or transcription inference' >&2
    exit 1
fi
if cargo tree --locked --no-default-features --features tts --prefix none --edges normal | grep -E '^(ptts|xn) '; then
    echo 'Cloud TTS unexpectedly includes local inference' >&2
    exit 1
fi
