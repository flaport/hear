# Hear integration

These Rust sources, build script, and license are unmodified from `whisper-rs`
0.16.0 on crates.io (archive SHA-256:
`2088172d00f936c348d6a72f488dc2660ab3f507263a195df308a3c2383229f6`).

Only the Cargo manifest is adapted: the native `whisper-rs-sys` dependency
points directly to `../whisper-rs-sys`, and unused example/dev targets are omitted.
An ordinary Cargo `[patch]` in Hear's manifest would be ignored by downstream
applications. This direct dependency ensures embedded library users get the same
single GGML runtime as our CLI, without copying patches into their manifests.

See [native build notes](../whisper-rs-sys/README.md) for supported backends,
source pins, and upgrade verification.
