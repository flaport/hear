# hear-app

`hear-app` is a Linux-only system-tray companion for `hear`. Press Alt-X
to start recording, press it again to stop, and the app runs the bundled `hear`
CLI. Successful transcripts are always copied to the clipboard and can
optionally be pasted into the active application.

For window managers where a tray or application-managed global shortcut is not
practical, `hear-app oneshot` provides the same record/transcribe/deliver
workflow for an external hotkey daemon. Invoke it once to begin recording and a
second time to stop; the original process then transcribes, pastes, and exits.
If the configured shortcut is already reserved, the tray app remains usable
from its menu and prints a warning instead of exiting during startup.

The crate intentionally remains separate from the reusable `hear` library.
Linux UI, hotkey, clipboard, and paste-injection concerns live here;
transcription engines and formatting remain owned by `hear`.

## Development

Build both binaries:

```sh
cargo build -p hear -p hear-linux -p hear-local-polish
```

Install the companion and helper binary:

```sh
crates/hear-linux/install.sh
```

Store the API key in the system keyring before launching:

```sh
hear-app install-api-key
```

The command reads the key without echoing it. Remove it with
`hear-app remove-api-key`. An inherited `OPENAI_API_KEY` takes precedence over
the keyring entry during development.

## Configuration

The app reads `~/.config/hear-app/config.toml`, or
`$XDG_CONFIG_HOME/hear-app/config.toml` when `XDG_CONFIG_HOME` is set. Every
setting is optional; restart the app after editing the file. The defaults are
equivalent to:

```toml
hotkey = "alt+x"
paste_automatically = true

[paste_shortcuts]
default = "ctrl+v"
Alacritty = "alt+v"

[hear]
engine = "gpt-transcribe"
model = ""
language = ""
polish_engine = "openai"
polish_model = ""
context = "auto"
polish = true
save_recording = ""
output = ""
raw_output = ""
force = false
```

Empty paths disable the corresponding file output, while an empty `model`,
`language`, or `polish_model` selects the CLI default. For example, local Dutch
transcription without polishing can be selected with:

```toml
[hear]
engine = "whisper"
model = "large-v3-turbo"
language = "nl"
polish = false
```

To keep both the audio and transcript on the machine, select local polishing:

```toml
[hear]
engine = "whisper"
model = "large-v3-turbo"
language = "nl"
polish_engine = "local"
polish_model = "qwen3.5-0.8b"
```

Audio capture remains app-owned and is always enabled when the hotkey is used.
Paste shortcut overrides use X11 window classes and `xdotool` shortcut syntax.

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
On X11, paste injection uses Alt-V when the focused application is Alacritty
and Ctrl-V for other applications.

## External hotkey daemons

For example, an SXHKD binding can toggle the one-shot mode with:

```text
alt + space
    hear-app oneshot
```

This mode does not require a system tray. The ordinary `hear-app` command
continues to launch the tray app, so either integration can be used.

## Desktop integration

The install script places a `.desktop` file in
`~/.local/share/applications/hear.desktop` so the app appears in your
application launcher.
