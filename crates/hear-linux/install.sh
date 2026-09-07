#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
prefix="${PREFIX:-$HOME/.local}"

cargo build --manifest-path "$repository_root/Cargo.toml" --locked --release -p hear -p hear-linux

mkdir -p "$prefix/bin"
cp "$repository_root/target/release/hear" "$prefix/bin/hear"
cp "$repository_root/target/release/hear-linux" "$prefix/bin/hear-linux"

desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$desktop_dir"
cat > "$desktop_dir/hear.desktop" << DESKTOP
[Desktop Entry]
Type=Application
Name=Hear
Comment=Dictation companion — press Alt-Space to transcribe
Exec=$prefix/bin/hear-linux
Terminal=false
Categories=Utility;Audio;
DESKTOP

echo "Installed hear and hear-linux to $prefix/bin"
echo "Desktop entry written to $desktop_dir/hear.desktop"
