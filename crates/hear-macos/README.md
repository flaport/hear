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
engine = ""
model = ""
language = ""
polish_engine = ""
polish_model = ""
context = "auto"
polish = true
```

Empty engine values let the CLI infer an engine from its model options, and an
empty model selects the CLI default. For a fully local configuration using the
smaller polishing model:

```toml
[hear]
model = "tiny.en"
language = "en"
polish_model = "qwen3.5-0.8b"
context = "auto"
polish = true
```

Whisper models are `tiny.en`, `base.en`, `small.en`, `medium.en`, and
`large-v3-turbo`. Local polishing models are `qwen3.5-2b` (the default) and
`qwen3.5-0.8b`. Fully local configurations do not access Keychain for an
OpenAI API key.

## Menu-bar visibility

Hear uses the Linux app's microphone icon: adaptive monochrome while idle,
red while recording, and amber while transcribing. On macOS 26, its permission is under
System Settings → Menu Bar → Allow in the Menu Bar. The app gives its native
status item a stable name so macOS can remember your chosen position.

If Hear is running and allowed but its icon is invisible, quit Hear and reset
its saved position, then reopen it:

```sh
defaults write dev.flaport.hear-macos "NSStatusItem Preferred Position Hear" -float 0
open /Applications/Hear.app
```

## Recording recovery

Engine and model validation, audio capture, and helper execution use `hear-core`.
Invalid configurations are rejected before recording. Unknown transcription
models require an explicit `engine = "codex"`. The shared `[hear]` settings also
accept `save_recording`, `output`, `raw_output`, and `force`, as on Linux.

Automatic paste requires the application focused at recording start to remain
focused after transcription. Otherwise the text stays on the clipboard.
Failures retain the WAV and available transcript in the temporary directory;
the error includes the recording path. Retry with `hear /path/to/recording.wav`.
Successful delivery removes the temporary recording. Quitting Hear cancels and
reaps an active helper.
