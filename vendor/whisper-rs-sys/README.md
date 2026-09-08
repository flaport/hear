# Shared GGML build

Hear ships one CLI executable containing Whisper and llama.cpp. This local
`whisper-rs-sys` patch replaces the upstream build script so both engines use
one GGML implementation. It does not rename symbols or suppress linker errors.

## Pinned sources

- Rust API: `whisper-rs` 0.16.0, pinned in the root manifest.
- Whisper sources: unmodified `whisper.cpp` 1.8.3 files from the crates.io
  `whisper-rs-sys` 0.15.0 package (archive SHA-256:
  `6986c0fe081241d391f09b9a071fbcbb59720c3563628c3c829057cf69f2a56f`). Only the build/include/source subset needed by
  its CMake library build is retained; its vendored GGML is deliberately absent.
  The upstream MIT license is in `whisper.cpp/LICENSE`. The Rust FFI wrapper
  originates from `whisper-rs-sys` (Unlicense).
- GGML: the exact sources bundled by `llama-cpp-sys-2` 0.1.156, pinned here and
  selected by `llama-cpp-2` 0.1.156 in `hear-local-polish`.

## Build ownership

`whisper-rs-sys` depends directly on `llama-cpp-sys-2`. Cargo therefore builds
llama.cpp and GGML first and passes `DEP_LLAMA_GGML_CMAKE_DIR` to our build script.
Whisper uses `WHISPER_USE_SYSTEM_GGML=ON` with that exact CMake package; it does
not search for a separately installed system GGML. Bindgen uses the installed
headers from the same build, avoiding header/library ABI mismatches.

The patch links only `libwhisper.a`; `llama-cpp-sys-2` owns GGML and all its
backend/platform link directives. CPU is supported on Linux and macOS, with
Metal enabled on macOS. Upstream Whisper features for other accelerators are
not exposed by this patch. The CMake wrapper builds Whisper as a subproject so
it never generates files in the source checkout (including read-only builds).

## Upgrades and verification

Upgrade these dependencies together. Copy updated Whisper files from the
pinned upstream crate, preserving its license, and verify that Whisper still
compiles against llama.cpp's GGML headers. Never add Whisper's own GGML back.

Run the full CLI, library and platform-app tests and Clippy on Linux and macOS.
Also run the ignored `standalone_binary_runs_both_native_engines` integration
test with `HEAR_TEST_AUDIO` pointing to a short English 16 kHz mono PCM16 WAV:

```sh
HEAR_TEST_AUDIO=/absolute/path/speech.wav cargo test --release --test native \
  standalone_binary_runs_both_native_engines -- --ignored --nocapture
```

This copies only `hear` into an empty directory and removes `PATH`, then runs
Whisper and local polishing together. It downloads the checked tiny.en and
Qwen3.5-0.8B models if absent. `HEAR_TEST_EXPECTED` can specify text that must
appear in both raw and polished output (default: `hello`). Model inference is
explicitly opt-in so normal tests do not download hundreds of megabytes.

Check release dynamic dependencies with `otool -L` (macOS) or `ldd` (Linux):
there must be no dependency on a separately installed Whisper, llama or GGML
library. The CLI archive must contain only `hear`; app packages contain the
platform executable plus `hear`. System libraries and FFmpeg for converting
non-normalized audio retain their existing requirements.
