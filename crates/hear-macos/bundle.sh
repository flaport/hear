#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
app_dir="$repository_root/dist/Hear.app"
contents_dir="$app_dir/Contents"

cargo build --manifest-path "$repository_root/Cargo.toml" --locked --release -p hear -p hear-macos

rm -rf "$app_dir"
mkdir -p "$contents_dir/MacOS" "$contents_dir/Helpers"
cp "$repository_root/crates/hear-macos/Info.plist" "$contents_dir/Info.plist"
cp "$repository_root/target/release/hear-macos" "$contents_dir/MacOS/hear-macos"
cp "$repository_root/target/release/hear" "$contents_dir/Helpers/hear"
codesign --force --deep --sign - "$app_dir"

echo "Created $app_dir"
