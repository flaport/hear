# hear-linux

`hear-linux` is a Linux-only system-tray companion for `hear`. Press Alt-Space
to start recording, press it again to stop, and the app runs the bundled `hear`
CLI. Successful transcripts are always copied to the clipboard and can
optionally be pasted into the active application.

For window managers where a tray or application-managed global shortcut is not
practical, `hear-linux oneshot` provides the same record/transcribe/deliver
workflow for an external hotkey daemon. Invoke it once to begin recording and a
second time to stop; the original process then transcribes, pastes, and exits.
If Alt-Space is already reserved, the tray app remains usable from its menu and
prints a warning instead of exiting during startup.

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

## Clipboard and automatic paste

Clipboard persistence requires `xclip` on X11 or `wl-copy` from
`wl-clipboard` on Wayland. The "Paste Automatically" option additionally
requires `xdotool` (X11) or `wtype` (Wayland):

```sh
# X11
sudo apt install xclip xdotool   # or: sudo pacman -S xclip xdotool

# Wayland
sudo apt install wl-clipboard wtype   # or: sudo pacman -S wl-clipboard wtype
```

If paste injection is unavailable, the transcript remains on the clipboard.

## External hotkey daemons

For example, an SXHKD binding can toggle the one-shot mode with:

```text
alt + space
    hear-linux oneshot
```

This mode does not require a system tray. The ordinary `hear-linux` command
continues to launch the tray app, so either integration can be used.

## Desktop integration

The install script places a `.desktop` file in
`~/.local/share/applications/hear.desktop` so the app appears in your
application launcher.
