# Hear: Listen-and-Speak SDK Plan

## Motivation and bigger picture

Hear should become a focused Rust audio interface through which an AI—or any other program—can **listen and speak**. It should own microphone capture, transcription, speech synthesis, audio playback, streaming, cancellation, and useful latency measurements. Dialogue orchestration, memory, tools, and assistant behavior remain outside Hear.

The immediate goal is deliberately small: speak into the local microphone and hear the transcribed words spoken back. This creates a fast, tangible way to evaluate end-to-end voice latency before adding NanoClaw, Cody Link, Android, or conversational behavior.

## Guiding boundaries

- Hear owns audio input/output and speech-to-text/text-to-speech machinery.
- Callers own conversation state and decide what text should be spoken.
- Preserve current transcription-only use cases and lightweight builds through feature flags.
- Prefer streaming interfaces, even when the first implementation internally buffers.
- Keep engines replaceable: cloud and local implementations should share contracts.
- Introduce structured events and document threading, cancellation, and completion semantics alongside each capability; final SDK stabilization should refine these contracts.
- Do not place long-lived provider credentials in a future mobile client; that concern belongs to a server or ephemeral-token layer.

## Existing foundation

Hear already supports microphone capture and streaming transcription through `Workflow::run_streaming` and `streaming::Adapter`. `Workflow::transcript_updates` exposes provisional and committed engine segments while audio is processed; the adapter still returns the completed transcript at `finish()`. Interactive `hear --stream` shows live previews on stderr. Return finishes a CLI recording; Ctrl-C cancels it. Timing instrumentation remains to be added.

Capture uses CPAL and emits mono PCM16 little-endian audio at 16 kHz. `hear-core::audio_stream::AudioSink` already names the bounded capture transport. Desktop companions use isolated helper processes to cancel native inference or network work. Reuse these capabilities and account for their lifetime model instead of building a second transcription pipeline.

The current `workflow` feature includes capture, Whisper, and local polishing. New synthesis and playback capabilities must be independently usable without depending on that umbrella feature.

## Current execution order (2026-09-27)

At the user's request, Stage 2 was completed before Stage 1's timing work. OpenAI
`gpt-4o-mini-tts` is the first engine, with Cedar as the default and a per-call
voice override. Kokoro, Pocket TTS, and Piper have been auditioned and benchmarked
on Linux (see `docs/tts-comparison.md`). The user selected Pocket TTS as the
preferred local engine. Native Pocket TTS is now integrated behind the separate
`pocket-tts` feature and `hear speak --engine pocket`, defaulting to Alba. The
September model and voice caches are verified and downloaded once; Python is
not required. Streaming SDK output works; native streaming playback remains
Stage 3/5 work.

Stage 2 now provides `hear::speech` behind the independent `tts` feature,
`hear speak`, and the synthesis-only `speak` example. The audio contract is mono
PCM16 at 24 kHz; future backends must convert to that contract or explicitly
extend it. The existing capture contract stays at 16 kHz. Synthesis events run
on the calling worker and completion means delivery to the sink. Cancellation
is cooperative. OpenAI has a configurable total deadline (default 60 seconds);
Pocket bounds asset-download network waits to 10 seconds and checks cancellation
between inference frames. Model loading and individual tensor operations cannot
be interrupted; callbacks must bound their own work. Atomic WAV output
protects existing files on errors. CLI `--play` uses a system file player as an
interim convenience; Stage 3's native playback SDK is still pending.

Future incremental text input should use a session with ordered `submit(text)`
calls and explicit `finish_input()` and `cancel()`. Audio order must match text
order, with one terminal completion, failure, or cancellation event. The initial
implementation takes one complete text request; it does not expose that session.

Status: Stage 2 is complete. Stage 0's synthesis contracts and build checks are
complete; playback design and macOS verification remain open. Stage 3 has working
CLI file playback, while its native SDK remains pending. Stage 1's timing harness
and Stages 4–6 have not been implemented.

### Quality follow-up

- [ ] Compare native Pocket Alba with the initial Python Alba audition using the same text and settings. The user found the later samples somewhat worse, but accepts the current quality for now. The cause has not been established; this is a follow-up, not a blocker for the completed TTS integration.

## Stage 0 — Baseline and design decisions

- [x] Verify existing transcription/build checks on Linux after the TTS changes.
- [ ] Verify the current changes and existing streaming path on macOS.
- [x] Decide the first TTS engine and voice: OpenAI with Cedar, plus native Pocket TTS with Alba. Both voices remain configurable.
- [ ] Audit existing audio dependencies and reuse the current capture stack for playback where practical.
- [x] Add independent `tts` and `pocket-tts` features while preserving existing `capture`, `workflow`, and default CLI behavior.
- [ ] Define the additive `playback` feature boundary. Defer a broader feature reorganization unless implementation requires it.
- [x] Add TTS-only build/dependency checks: cloud and local TTS exclude microphone, Whisper, and local polishing; cloud TTS excludes Pocket inference. Verify existing minimal and workflow builds.
- [ ] Add playback-only checks that exclude synthesis and inference engines.
- [x] Document the synthesis audio contract: ordered borrowed mono PCM16 chunks at 24 kHz, consumed or copied before the sink returns. Decode OpenAI little-endian bytes and convert Pocket float samples before delivering PCM; completion means all samples delivered to the sink.
- [x] Preserve 16 kHz transcription input independently of 24 kHz synthesis and report the synthesis format in metadata.
- [ ] Document supported playback-device format conversions.
- [x] Define synthesis cancellation, threading, events, and completion semantics. Use a distinct `SpeechAudioSink` contract alongside the existing capture `AudioSink`.

**Exit criterion:** existing behavior is checked, the first engine is chosen, and the proposed API and feature boundaries are sketched before implementation.

## Stage 1 — Instrument existing transcription

Establish the transcription baseline with a thin example around existing SDK capabilities. Microphone capture, live terminal transcripts, Return to finish, and Ctrl-C cancellation already work in the CLI; the unchecked items below track the new instrumented example.

- [ ] Add an example such as `cargo run --example echo_text -- --stream`.
- [ ] Capture the default microphone using the existing Hear capture machinery.
- [ ] Use `Workflow::run_streaming` and the existing transcription adapter path; keep terminal interaction in the example.
- [ ] Preserve Return to finish and Ctrl-C to cancel.
- [ ] Print the raw transcript immediately.
- [ ] Optionally print the polished transcript afterward, but keep polishing disabled by default for latency testing.
- [ ] Emit structured timing events using a monotonic clock and render them in the example for:
  - recording start
  - explicit stop request
  - capture stopped and final PCM handed off
  - transcript ready
  - transcription latency after the explicit stop request
- [ ] Label the baseline as stop-request-to-transcript latency. Do not equate pressing Return with the actual acoustic end of speech.
- [ ] Record repeated cold and warmed-up trials separately, including model loading and connection setup where applicable. Retain engine/model, input duration, platform, trial count, and per-run timings so comparisons are reproducible.
- [ ] Add a smoke test around the example's non-audio orchestration using fake PCM and a fake transcription adapter.

**Exit criterion:** speaking locally produces terminal text reliably, with a clear latency measurement.

## Stage 2 — TTS as a standalone SDK capability

Add speech generation independently of microphone capture.

- [x] Introduce a provider-neutral speech synthesis contract. The implemented blocking interface is:

```rust
pub trait SpeechEngine {
    fn synthesize(
        &self,
        request: &SpeechRequest,
        sink: &mut dyn SpeechAudioSink,
        cancellation: &Cancellation,
    ) -> Result<SpeechSummary>;
}
```

- [x] Define `SpeechRequest` with text, configurable voice, model, speed, and delivery instructions. Use one fixed output format (mono PCM16, 24 kHz), validate supported ranges, and report the actual format. Provider errors reject unknown models or voices.
- [x] Define decoded PCM chunks without coupling the sink API to a provider protocol. Initially support one well-defined format; encoded provider data is decoded before reaching this sink.
- [x] Specify which thread calls the sink, whether it may block, how errors propagate, and how another thread cancels synthesis. Bound cancellation waits during network reads and sink writes; document any backend limitations and whether helper-process isolation is required.
- [x] Define successful synthesis completion as all audio delivered to the sink. Playback completion is a separate event; cancellation must not appear as successful completion.
- [x] Keep incremental audio output distinct from incremental text input. The initial request may contain complete text, but sketch how a future session could accept ordered text segments and an explicit end-of-input without requiring the whole assistant answer first. Defer implementation until after the initial milestones.
- [x] Emit structured synthesis start, first-audio, completion, and cancellation events, with documented ordering and failure behavior.
- [x] Capture synthesis metadata: selected engine/model/voice, audio duration, request duration, and time to first audio chunk.
- [x] Implement OpenAI TTS with configurable voices and Cedar as the default.
- [x] Audition and benchmark Kokoro, Pocket TTS, and Piper locally; record results in `docs/tts-comparison.md`.
- [x] Integrate native Pocket TTS into the Rust binary with configurable voices and Alba as the default, verified model/voice downloads, and an offline cache; no Python runtime is required.
- [x] Verify native Pocket synthesis, cancellation, engine reuse, sink failures, and cached offline operation.
- [x] Add a `hear speak "hello"` CLI path or a focused `speak` example.
- [x] Initially write generated audio to a file so synthesis can be tested independently from playback.
- [x] Add deterministic tests with a mock HTTP transport and fixture audio.
- [x] Ensure errors retain enough context to distinguish authentication, network, decoding, and output failures.
- [x] Run the TTS-only dependency and build checks from Stage 0.

**Exit criterion:** text can be synthesized through the Rust SDK and CLI/example into a valid audio file.

## Stage 3 — Local audio playback

Partially complete through the CLI's system file player. The native playback SDK and device-level events are still pending.

- [ ] Introduce a `Speaker`/playback abstraction separate from TTS.
- [x] Play local WAV samples through the default output device on Linux.
- [x] Play the file generated in Stage 2 using `hear speak --play`.
- [x] Support CLI playback cancellation through the cancellable system-player process.
- [ ] Support cancellation in the native playback SDK.
- [ ] Distinguish audio accepted, samples submitted to the device, and playback finished. Document how buffered device audio affects cancellation and completion; do not promise instantaneous silence.
- [ ] Handle missing/default-device changes and unsupported formats with useful errors.
- [ ] Avoid blocking real-time audio callbacks with network, allocation-heavy, or inference work.
- [ ] Add fake-output tests that verify chunk order, cancellation, and completion without requiring speakers in CI.
- [ ] Emit structured playback events and document worker/callback responsibilities and format conversion behavior.

**Exit criterion:** the native playback SDK powers `hear speak "hello" --play` (or its chosen equivalent), with documented cancellation and device-level completion semantics. The current system-player implementation already makes the CLI command speak.

## Stage 4 — One-shot spoken echo through a file

Combine the previous stages into the first complete latency experiment before optimizing playback latency.

- [ ] Add `cargo run --example echo_voice -- --stream`.
- [ ] Capture and transcribe one microphone turn.
- [ ] Send the unpolished transcript directly to TTS.
- [ ] Synthesize to a temporary file, then play it through the default speaker. This deliberately buffered path establishes the baseline for Stage 5.
- [ ] Print the transcript before speech begins.
- [ ] Print a compact timing summary:
  - explicit stop request → transcript ready
  - transcript ready → synthesis complete
  - transcript ready → first sample submitted to the output device
  - explicit stop request → first sample submitted to the output device
  - response playback duration
- [ ] Report estimated first-audible timing separately only when the backend provides a defensible estimate; otherwise mark it unavailable. Actual acoustic latency requires a separate loopback measurement.
- [ ] Allow engine/model/voice selection through flags or normal Hear configuration.
- [ ] Add `--save-input` and `--save-output` options for debugging and reproducible comparisons.
- [ ] Stop capture before playback and keep it stopped for this one-shot experiment. Document headphones as the preferred setup for later repeating-turn comparisons.
- [ ] Verify cancellation across transcription, synthesis, and playback, including temporary-file cleanup and explicitly saved debug audio.
- [ ] Repeat the baseline trials from Stage 1 and retain enough metadata to compare the buffered and streaming paths.

**Exit criterion:** the user speaks once, sees the transcript, and hears the same words spoken back, with measured stop-request-to-output latency. Streaming playback is not required for this milestone.

## Stage 5 — Streaming synthesis to playback

Reduce perceived latency by replacing the file boundary in the working echo experiment with streamed PCM.

- [ ] Connect TTS PCM chunks directly to a bounded playback queue, with a documented capacity in audio time.
- [ ] Start playback as soon as sufficient audio is available; do not wait for the complete response. Record the startup buffering threshold.
- [ ] Define backpressure explicitly: producer waits must be bounded and cancellable, the audio callback must never wait for the producer, and overflow must not silently drop speech.
- [ ] Define underrun behavior and emit structured underrun events.
- [ ] Support clean cancellation of the provider request, decoder, queue, and player, including disposal of queued audio and documented device-buffer limits.
- [ ] Report separately:
  - request start → first received provider audio bytes
  - request start → first decoded PCM frames
  - request start → first sample submitted to the output device
  - request start → estimated first audible sample, if available
  - total synthesis and playback duration
- [ ] Use the same event meanings and timing origin for buffered and streaming echo comparisons. Keep the buffered path selectable as a baseline.
- [ ] Test slow producer, slow consumer, cancellation during blocked I/O, truncated audio, underruns, and queue overflow scenarios.
- [ ] Compare repeated cold and warmed-up trials against Stage 4, reporting the trial count, median, and spread for short and long utterances.

**Exit criterion:** long text begins playing before synthesis completes, and the echo harness quantifies the change in stop-request-to-output latency against its buffered baseline.

## Stage 6 — Repeating turns and automatic endpointing

Turn the one-shot experiment into a useful interactive harness.

- [ ] Repeat listen → transcribe → speak until cancelled.
- [ ] Add silence/turn endpoint detection behind a configurable interface.
- [ ] Keep explicit push-to-talk/press-to-stop mode as a deterministic fallback.
- [ ] Prevent generated speech from opening the next microphone turn by pausing capture during playback initially.
- [ ] Make silence thresholds and maximum turn duration configurable.
- [ ] Show state transitions in the terminal: `Listening`, `Transcribing`, `Speaking`, `Cancelled`.
- [ ] Measure false endpoints, missed endpoints, and total latency across several turns.
- [ ] Separate detected speech end from the endpoint decision time so silence-wait latency is visible. Use labeled recordings when assessing actual speech-end accuracy.

**Exit criterion:** a hands-free local echo loop works reliably when headphones are used.

## Stage 7 — Public SDK stabilization

- [ ] Review naming and ownership of `SpeechEngine`, the synthesis sink, `Speaker`, and session types against the implemented examples.
- [ ] Review the structured events introduced in earlier stages for consistency; callers must not need to parse terminal output.
- [ ] Add a focused transcribe-only SDK example.
- [x] Add a synthesize-to-file SDK example (`examples/speak.rs`).
- [ ] Add a synthesize-and-play SDK example.
- [ ] Add a one-shot voice echo example.
- [ ] Add a repeating voice echo example.
- [ ] Consolidate the threading, blocking behavior, cancellation, and callback-safety documentation introduced with each capability.
- [ ] Consolidate supported PCM/audio formats and all implicit conversions.
- [ ] Complete CI coverage using fake capture, transcription, TTS, and playback adapters, plus the minimal/TTS-only/playback-only/existing-workflow feature checks.
- [ ] Preserve semver compatibility where reasonable; explicitly document deliberate breaking changes.

**Exit criterion:** an external Rust program can compose listening and speaking without relying on Hear's CLI internals.

## Stage 8 — Future duplex conversation work

This stage is relevant to Cody Link but is not required for the initial echo benchmark.

- [x] Emit incremental transcription events alongside the final result at `finish()` through `Workflow::transcript_updates`.
- [x] Distinguish provisional and committed transcription segments in `TranscriptUpdate`.
- [ ] Distinguish completed conversational turns from committed transcription segments.
- [ ] Implement incremental text submission to synthesis using the session contract sketched in Stage 2, with explicit ordering, flush/end-of-input, and cancellation semantics. Keep decisions about assistant response content outside Hear.
- [ ] Add barge-in: microphone speech cancels or ducks active playback.
- [ ] Investigate acoustic echo cancellation for speakerphone use without headphones.
- [ ] Support simultaneous capture and playback with explicit session state.
- [ ] Add a transport-friendly protocol for remote capture/playback clients.
- [ ] Provide an Android integration layer, likely using native `AudioRecord`/`AudioTrack` with Rust behind JNI or UniFFI.
- [ ] Keep provider credentials server-side or issue tightly scoped ephemeral session credentials.

**Exit criterion:** Hear can serve as the bidirectional audio foundation beneath Cody Link without absorbing assistant or application logic.

## Suggested implementation order

With Stage 2 complete, finish the remaining Stage 0 design/verification, Stage 1 transcription timing, and Stage 3 native playback work, then compose Stage 4's one-shot spoken echo. Together these form the first milestone. Stage 5 is a separate optimization milestone that measures streaming playback against that working baseline.

Do not begin automatic turn detection, incremental text submission, barge-in, Android bindings, or NanoClaw integration until the spoken echo and streaming comparison work and their latency is measured. Use those measurements to choose subsequent priorities across endpointing, transcription, synthesis, playback buffering, and network placement; the later stages are a roadmap rather than a commitment to build every capability immediately.

## Non-goals for the first milestone

- Conversational AI responses
- Streaming playback (the next milestone)
- Incremental text input to synthesis
- Automatic endpointing and barge-in
- NanoClaw integration
- Android UI or packaging
- Wake-word detection
- Speakerphone echo cancellation
- Multi-user sessions
- Persistent conversation history
- A final production voice or provider choice
