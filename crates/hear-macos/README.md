# hear-macos

`hear-macos` is a macOS-only menu-bar companion for `hear`. Press Option-Space
to start recording, press it again to stop, and the app runs the bundled `hear`
CLI. Successful transcripts are always copied to the clipboard and can
optionally be pasted into the active application.

The crate intentionally remains separate from the reusable `hear` library.
macOS UI, permission, hotkey, clipboard, and event-injection concerns live here;
transcription engines and formatting remain owned by `hear`.

## Development

Build both binaries:

```sh
cargo build -p hear -p hear-macos -p hear-local-polish
```

Create an ad-hoc-signed application bundle:

```sh
crates/hear-macos/bundle.sh
open dist/Hear.app
```

Store the API key in macOS Keychain before launching the app from Finder:

```sh
dist/Hear.app/Contents/MacOS/hear-macos install-api-key
```

The command reads the key without echoing it. Remove it with
`hear-macos remove-api-key`. An inherited `OPENAI_API_KEY` takes precedence over
the Keychain entry during development.

The first recording prompts for microphone access. Automatic paste additionally
requires Hear to be enabled under System Settings → Privacy & Security →
Accessibility. When Accessibility access is unavailable, the transcript stays
on the clipboard. The menu includes a shortcut to the appropriate System
Settings page.

The app uses the bundled `Contents/Helpers/hear` binary. Consequently all
ordinary `hear` engine requirements currently apply. The companion injects its
Keychain-backed API key into the helper for the default transcription and
polishing workflow.

## Configuration

The app reads `~/Library/Application Support/hear-app/config.toml`. Every
setting is optional; restart the app after editing the file. The defaults are
equivalent to:

```toml
paste_automatically = true

[hear]
engine = "gpt-transcribe"
model = ""
language = ""
polish_engine = "openai"
polish_model = ""
context = "auto"
polish = true
```

An empty model selects the CLI default. For a fully local configuration using
the smaller polishing model:

```toml
[hear]
engine = "whisper"
model = "tiny.en"
language = "en"
polish_engine = "local"
polish_model = "qwen3.5-0.8b"
context = "auto"
polish = true
```

Whisper models are `tiny.en`, `base.en`, `small.en`, `medium.en`, and
`large-v3-turbo`. Local polishing models are `qwen3.5-2b` (the default) and
`qwen3.5-0.8b`. Fully local configurations do not access Keychain for an
OpenAI API key.
