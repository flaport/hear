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
cargo build -p hear -p hear-macos
```

Create an ad-hoc-signed application bundle:

```sh
crates/hear-macos/bundle.sh
open dist/Hear.app
```

The first recording prompts for microphone access. Automatic paste additionally
requires Hear to be enabled under System Settings → Privacy & Security →
Accessibility. When Accessibility access is unavailable, the transcript stays
on the clipboard.

The app uses the bundled `Contents/Helpers/hear` binary. Consequently all
ordinary `hear` environment and engine requirements currently apply, including
`OPENAI_API_KEY` for the default transcription and polishing workflow.
