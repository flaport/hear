#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
app_dir="$repository_root/dist/Hear.app"
contents_dir="$app_dir/Contents"

if [ -n "${TARGET:-}" ]; then
    cargo build --manifest-path "$repository_root/Cargo.toml" --locked --release \
        -p hear -p hear-macos -p hear-local-polish --target "$TARGET"
    release_dir="$repository_root/target/$TARGET/release"
else
    cargo build --manifest-path "$repository_root/Cargo.toml" --locked --release \
        -p hear -p hear-macos -p hear-local-polish
    release_dir="$repository_root/target/release"
fi

rm -rf "$app_dir"
mkdir -p "$contents_dir/MacOS" "$contents_dir/Helpers"
cp "$repository_root/crates/hear-macos/Info.plist" "$contents_dir/Info.plist"
cp "$release_dir/hear-macos" "$contents_dir/MacOS/hear-macos"
cp "$release_dir/hear" "$contents_dir/Helpers/hear"
cp "$release_dir/hear-local-polish" "$contents_dir/Helpers/hear-local-polish"
codesign --force --deep --sign - "$app_dir"

echo "Created $app_dir"
