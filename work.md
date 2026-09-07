# Refactoring roadmap

This document is the source of truth for the staged `hear` refactor. Each stage
must preserve externally observable behavior unless the stage explicitly calls
out a deliberate improvement. After every stage, its status and verification
notes are updated here before the stage is committed and pushed to `main`.

## Status

| Stage | State | Summary |
| --- | --- | --- |
| 0 | Complete | Record the roadmap and establish a clean baseline. |
| 1 | Pending | Centralize shared OpenAI HTTP behavior. |
| 2 | Pending | Split OpenAI transcription and harden upload preparation. |
| 3 | Pending | Split transcript polishing by responsibility. |
| 4 | Pending | Introduce options-based public library APIs. |
| 5 | Pending | Split microphone capture, processing, and WAV output. |
| 6 | Pending | Harden local Whisper configuration and model downloads. |
| 7 | Pending | Add continuous CI and synchronize documentation. |
| 8 | Pending | Run final verification and reconcile the roadmap. |

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

State: Pending

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

## Stage 2 — OpenAI transcription and uploads

State: Pending

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

## Stage 3 — Transcript polishing

State: Pending

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

## Stage 4 — Options-based library API

State: Pending

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

## Stage 5 — Microphone recording internals

State: Pending

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

## Stage 6 — Whisper hardening

State: Pending

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

## Stage 7 — Continuous verification and documentation

State: Pending

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
