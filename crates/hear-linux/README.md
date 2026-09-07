# hear-linux

`hear-linux` is a Linux-only system-tray companion for `hear`. Press Alt-Space
to start recording, press it again to stop, and the app runs the bundled `hear`
CLI. Successful transcripts are always copied to the clipboard and can
optionally be pasted into the active application.

The crate intentionally remains separate from the reusable `hear` library.
Linux UI, hotkey, clipboard, and paste-injection concerns live here;
transcription engines and formatting remain owned by `hear`.

## Development

Build both binaries:

```sh
cargo build -p hear -p hear-linux
```

Install the companion and helper binary:

```sh
crates/hear-linux/install.sh
```

Store the API key in the system keyring before launching:

```sh
hear-linux install-api-key
```

The command reads the key without echoing it. Remove it with
`hear-linux remove-api-key`. An inherited `OPENAI_API_KEY` takes precedence over
the keyring entry during development.

## Automatic paste

The "Paste Automatically" option requires `xdotool` (X11) or `wtype` (Wayland)
to be installed:

```sh
# X11
sudo apt install xdotool   # or: sudo pacman -S xdotool

# Wayland
sudo apt install wtype      # or: sudo pacman -S wtype
```

When neither is available, the transcript stays on the clipboard.

## Desktop integration

The install script places a `.desktop` file in
`~/.local/share/applications/hear.desktop` so the app appears in your
application launcher.
