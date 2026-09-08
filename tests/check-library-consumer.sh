#!/bin/sh
# Build outside this workspace: root [patch] and CLI features must not be needed.
set -eu
repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT HUP INT TERM
ln -s "$repository_root" "$scratch/hear"
cp "$repository_root/Cargo.lock" "$scratch/Cargo.lock"
mkdir "$scratch/src"
cat > "$scratch/Cargo.toml" <<'TOML'
[package]
name = "hear-library-consumer"
version = "0.0.0"
edition = "2024"
[dependencies]
hear = { path = "hear", default-features = false, features = ["workflow"] }
TOML
cat > "$scratch/src/main.rs" <<'RS'
use hear::{HearConfig, Stage, Workflow, dictionary::Dictionary};
fn main() {
    let mut dictionary = Dictionary::default();
    dictionary.add("Qdrant", &["quadrant".into()], None).unwrap();
    assert_eq!(dictionary.correct_aliases("use quadrant").unwrap(), "use Qdrant");
    let workflow = Workflow::new(HearConfig::default()).dictionary(dictionary);
    let error = workflow.run(std::path::Path::new("missing.wav")).unwrap_err();
    assert_eq!(error.stage, Stage::Validation);
}
RS
# Reuse the pinned workspace resolution and artifacts; only this fixture's package
# entry is added to the temporary lockfile. Dependency sources are already cached.
(cd "$scratch" && cargo run --offline --manifest-path Cargo.toml \
    --target-dir "${CARGO_TARGET_DIR:-$repository_root/target}")
