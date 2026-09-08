#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
prefix="${PREFIX:-$HOME/.local}"

GGML_NATIVE="${GGML_NATIVE:-OFF}" cargo build --manifest-path "$repository_root/Cargo.toml" --locked --release \
    -p hear -p hear-linux

mkdir -p "$prefix/bin"
install_binary() {
    source_path=$1
    destination=$2
    temporary="$destination.installing"
    install -m 755 "$source_path" "$temporary"
    mv -f "$temporary" "$destination"
}

install_binary "$repository_root/target/release/hear" "$prefix/bin/hear"
install_binary "$repository_root/target/release/hear-app" "$prefix/bin/hear-app"
# Remove the helper installed by older versions after both replacements succeed.
rm -f "$prefix/bin/hear-local-polish"

desktop_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$desktop_dir"
cat > "$desktop_dir/hear.desktop" << DESKTOP
[Desktop Entry]
Type=Application
Name=Hear
Comment=Dictation companion — press Alt-X to transcribe
Exec=$prefix/bin/hear-app
Terminal=false
Categories=Utility;Audio;
DESKTOP

echo "Installed hear and hear-app to $prefix/bin"
echo "Desktop entry written to $desktop_dir/hear.desktop"
