# Refactoring roadmap

This document is the source of truth for the staged `hear` refactor. Each stage
must preserve externally observable behavior unless the stage explicitly calls
out a deliberate improvement. After every stage, its status and verification
notes are updated here before the stage is committed and pushed to `main`.

## Status

| Stage | State | Summary |
| --- | --- | --- |
| 0 | Complete | Record the roadmap and establish a clean baseline. |
| 1 | Complete | Centralize shared OpenAI HTTP behavior. |
| 2 | Complete | Split OpenAI transcription and harden upload preparation. |
| 3 | Complete | Split transcript polishing by responsibility. |
| 4 | Complete | Introduce options-based public library APIs. |
| 5 | Complete | Split microphone capture, processing, and WAV output. |
| 6 | Complete | Harden local Whisper configuration and model downloads. |
| 7 | Complete | Add continuous CI and synchronize documentation. |
| 8 | Pending | Run final verification and reconcile the roadmap. |

## Stage checklist

- [x] Stage 0 — Roadmap and baseline
- [x] Stage 1 — Shared OpenAI transport
- [x] Stage 2 — OpenAI transcription and uploads
- [x] Stage 3 — Transcript polishing
- [x] Stage 4 — Options-based library API
- [x] Stage 5 — Microphone recording internals
- [x] Stage 6 — Whisper hardening
- [x] Stage 7 — Continuous verification and documentation
- [ ] Stage 8 — Final verification

## Stage 0 — Roadmap and baseline

State: Complete

Objectives:

- Record bounded stages, compatibility expectations, and acceptance criteria.
- Confirm the existing checkout is clean and synchronized with `origin/main`.
- Establish a baseline with formatting, lint, full-feature tests,
  library-only tests, generated documentation, and CLI help output.

Completed:

- Confirmed `main` is synchronized with `origin/main` at version 0.2.5.
- Ran `cargo fmt --check`.
- Ran `cargo clippy --locked --all-targets -- -D warnings`.
- Ran `cargo test --locked --all-targets`: 31 passed and 2 live API tests were
  intentionally ignored.
- Ran `cargo test --locked --no-default-features --lib`: 9 passed and 2 live
  API tests were intentionally ignored.
- Ran `cargo doc --locked --no-deps --no-default-features`.
- Inspected generated CLI help for the root and dictionary commands.

Acceptance: complete when this roadmap is committed and pushed.

## Stage 1 — Shared OpenAI transport

State: Complete

Objectives:

- Add a private OpenAI transport module responsible for reading
  `OPENAI_API_KEY`, constructing the blocking HTTP client, reading response
  bodies, and producing consistent API error messages.
- Remove duplicated authentication, client creation, and error-envelope parsing
  from transcription and polishing.
- Keep endpoint-specific request and success-response parsing in their owning
  modules.

Acceptance:

- Existing public APIs and CLI behavior remain unchanged.
- Unit tests cover successful body extraction and structured/fallback API
  errors without network access.
- Formatting, Clippy, and both feature configurations pass.

Completed:

- Added `src/openai_transport.rs` for API-key lookup, blocking client creation,
  response-body extraction, and consistent structured or plain-text API errors.
- Removed the duplicated transport setup and error-envelope types from OpenAI
  transcription and transcript polishing.
- Added three network-free transport tests covering success, structured errors,
  and plain-text fallback errors.
- Ran formatting, warnings-as-errors Clippy, full-feature tests, and
  library-only tests. All 34 non-live tests passed; the 2 live OpenAI tests
  remained intentionally ignored in each applicable test run.

## Stage 2 — OpenAI transcription and uploads

State: Complete

Objectives:

- Replace the single transcription implementation file with focused modules for
  request execution and upload preparation.
- Preserve direct upload for supported files within the size limit.
- Normalize unsupported formats and decide whether to split based on the
  resulting upload size, not only the original source size.
- Keep temporary-file ownership explicit so prepared uploads remain alive for
  the complete request sequence.
- Expand pure tests around supported extensions and upload-planning decisions.

Acceptance:

- Unsupported inputs whose converted result exceeds the upload limit are split
  rather than rejected solely because conversion made them larger.
- Direct, converted, and split paths have focused tests where external FFmpeg
  execution is not required.
- Existing library and CLI interfaces remain compatible.

Completed:

- Replaced the path-aliased `src/engines/openai.rs` file with an ordinary
  `src/openai/` module containing `request.rs` and `uploads.rs`.
- Isolated multipart request execution and success-response parsing from file
  conversion, splitting, validation, and temporary-file ownership.
- Added explicit direct/convert/split planning and unit tests for supported
  extensions, size boundaries, and converted-output rechecking.
- Changed unsupported-format handling to inspect the converted MP3 and split it
  when conversion produces a file above the 25 MB upload limit.
- Ran formatting, warnings-as-errors Clippy, full-feature tests, and
  library-only tests. All 36 non-live tests passed; the 2 live OpenAI tests
  remained intentionally ignored in each applicable test run.

## Stage 3 — Transcript polishing

State: Complete

Objectives:

- Turn the formatter into a directory module with separate transcript/context
  preparation, request construction, and response parsing responsibilities.
- Keep the base formatting policy in one obvious location.
- Define and test the interaction among explicit contexts, spoken directives,
  personal dictionary context, custom instructions, and verbatim mode.
- Keep the Responses API orchestration small and auditable.

Acceptance:

- Existing formatting behavior and JSON schema remain unchanged unless a test
  captures an intentional correction.
- Each pure transformation is independently unit tested.
- The live formatting test remains opt-in and ignored by default.

Completed:

- Replaced `src/formatter.rs` with a `src/formatter/` module containing
  `prepare.rs`, `request.rs`, `response.rs`, and a small orchestration module.
- Isolated spoken-directive/context preparation, formatting policy and JSON
  request construction, and structured response parsing.
- Added tests for explicit verbatim behavior, spoken verbatim bypass of
  dictionary/custom options, empty custom instructions, and empty structured
  output while retaining all previous formatter coverage.
- Kept the Responses API schema, model, endpoint, and public behavior unchanged.
- Ran formatting, warnings-as-errors Clippy, full-feature tests, and
  library-only tests. All 40 non-live tests passed; the 2 live OpenAI tests
  remained intentionally ignored in each applicable test run.

## Stage 4 — Options-based library API

State: Complete

Objectives:

- Introduce public, non-exhaustive options types for transcription and
  polishing instead of growing one function per argument combination.
- Support vocabulary, context, dictionary context, optional polishing, and an
  optional custom instruction through named configuration.
- Retain the version 0.2 compatibility functions as thin documented wrappers.
- Remove avoidable cloning when callers request only raw transcription.

Acceptance:

- Existing callers continue to compile unchanged.
- New options-based entry points have Rustdoc examples or compile-checked docs.
- Tests cover raw, ordinary polish, and custom-instruction configuration.

Completed:

- Added non-exhaustive `TranscriptionOptions` and `PolishOptions` types with
  named builder methods for vocabulary, context, dictionary context, formatter
  instructions, and optional polishing.
- Added `transcribe_openai_with_options`, `polish_with_options`, and the
  raw-only `transcribe_openai_raw` entry point.
- Retained all version 0.2 functions as compatibility wrappers over the new
  options-based implementation.
- Updated the CLI's OpenAI path to request raw text directly rather than create
  a `Transcript` containing two copies of the raw result.
- Added options tests and a compile-checked Rustdoc example.
- Ran formatting, warnings-as-errors Clippy, full-feature tests, library-only
  tests, doctests, and Rustdoc. All 42 non-live unit tests and the doctest
  passed; the 2 live OpenAI tests remained intentionally ignored in each
  applicable test run.

## Stage 5 — Microphone recording internals

State: Complete

Objectives:

- Split device capture/control, channel conversion and resampling, and WAV
  writing into focused modules.
- Bound avoidable peak memory by downmixing during capture or otherwise avoiding
  simultaneous multichannel, mono, and resampled full-recording buffers.
- Replace or improve naive downsampling so conversion to 16 kHz does not alias
  high-frequency input into the speech band.
- Preserve Return-to-finish, Ctrl-C cancellation, and exit status 130.

Acceptance:

- Signal-processing tests cover channel conversion, output length, boundaries,
  and representative resampling behavior.
- The recorded output remains 16-bit, mono, 16 kHz WAV.
- CLI behavior remains unchanged.

Completed:

- Replaced `src/audio.rs` with separate capture/control, signal-processing, WAV
  output, and orchestration modules under `src/audio/`.
- Added a stateful `MonoBuffer` that downmixes frames inside the audio callback
  and correctly carries partial frames across callback boundaries. Long stereo
  recordings now retain one mono sample per frame instead of both raw channels.
- Split upsampling and downsampling paths. Downsampling now averages a local
  window before rate conversion to attenuate frequencies that would otherwise
  alias into the 16 kHz recording.
- Added tests for callback boundaries, incomplete frames, output length,
  constant-signal preservation, high-frequency attenuation, clipping, and the
  exact mono/16 kHz/16-bit WAV format.
- Preserved interactive completion, Ctrl-C cancellation, output messages, and
  status 130 behavior.
- Ran formatting, warnings-as-errors Clippy, full-feature tests, and
  library-only tests. All 46 non-live tests passed; the 2 live OpenAI tests
  remained intentionally ignored in each applicable test run.

## Stage 6 — Whisper hardening

State: Complete

Objectives:

- Separate model catalog/download/cache behavior from inference and audio
  loading.
- Make English-only behavior explicit: either expose language selection for the
  multilingual model or clearly constrain and document it.
- Verify downloaded models with known integrity metadata rather than trusting
  only content length and a minimum file size.
- Preserve atomic installation through a temporary file.

Acceptance:

- Model selection and language behavior are represented by testable typed data.
- Corrupt or mismatched model downloads are rejected before entering the cache.
- Existing supported model names remain available.

Completed:

- Replaced `src/engines/whisper.rs` with separate inference orchestration,
  normalized-audio loading, and model catalog/cache modules.
- Added typed model metadata covering the existing five model names, exact byte
  sizes, multilingual capability, and upstream LFS SHA-256 identifiers.
- Pinned downloads to whisper.cpp model repository revision
  `5359861c739e955e79d9a303bcbc70fb988958b1` and verify both byte length and
  SHA-256 before installing or reusing a cached model.
- Added `--language LANGUAGE` for the Whisper engine. English-only models reject
  non-English language codes; `large-v3-turbo` accepts language codes and
  `--language auto`. The backward-compatible default remains English.
- Added tests for language/model compatibility, normalized WAV loading, catalog
  lookup, and hashing while copying a download.
- Added optional `sha2` support to the full CLI feature while keeping the
  minimal OpenAI library dependency set free of it.
- Ran formatting, warnings-as-errors Clippy, full-feature tests, library-only
  tests, doctests, and Rustdoc. All 50 non-live unit tests and the doctest
  passed; the 2 live OpenAI tests remained intentionally ignored in each
  applicable test run.

## Stage 7 — Continuous verification and documentation

State: Complete

Objectives:

- Add CI for ordinary pushes and pull requests that runs formatting, Clippy,
  full-feature tests, library-only tests, and documentation checks.
- Keep the tagged release workflow focused on cross-platform artifacts.
- Update the README to name the actual polishing model and document the
  options/custom-instruction API and Whisper language behavior.
- Check the bundled `hear` skill against the final CLI behavior.

Acceptance:

- The documented commands, model names, defaults, and privacy boundaries match
  the implementation.
- CI exercises both the default CLI build and minimal library build before a
  release tag is involved.

Completed:

- Added `.github/workflows/ci.yml` for pushes to `main` and pull requests.
- CI checks formatting, warnings-as-errors Clippy, all full-feature targets,
  minimal-library tests, doctests, and warning-free minimal-library Rustdoc.
- Updated the README to name `gpt-5.6-luna`, describe converted-file splitting,
  document pinned/checksummed Whisper downloads and language selection, and
  demonstrate the options-based library API.
- Updated the bundled `hear` skill with the final Whisper integrity and language
  behavior.
- Confirmed generated CLI help exposes `--language` with the documented
  default and scope.
- Re-ran the full local CI command set successfully: all 50 non-live unit tests
  and the doctest passed; the 2 live OpenAI tests remained intentionally
  ignored in each applicable test run.

## Stage 8 — Final verification

State: Pending

Objectives:

- Run the complete local verification matrix from a clean checkout.
- Review the final module tree for accidental duplication and visibility leaks.
- Confirm all compatibility wrappers, README examples, and CLI help agree.
- Record any intentionally deferred live, hardware, or cross-platform checks.

Acceptance:

- Formatting, warnings-as-errors Clippy, all non-live tests, minimal-feature
  tests, and Rustdoc pass.
- `work.md` accurately describes the delivered state and remaining external
  verification.
