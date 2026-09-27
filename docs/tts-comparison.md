# TTS audition and CPU comparison — 2026-09-27

OpenAI is implemented first, using `gpt-4o-mini-tts` and Cedar by default. The
user auditioned Marin, Cedar, Coral, and Ash and chose Cedar. `--voice` and the
SDK's `SpeechRequest.voice` override the default. Local engines are experiments,
not dependencies of Hear or implemented Hear backends yet.

## Method

Machine: Linux x86_64, AMD Ryzen AI 9 HX 370. Local inference uses two CPU threads,
without a GPU. Each local engine loads once, then generates the same text three
times sequentially. Trial 1 is first synthesis after loading; trials 2 and 3 are
warmed up. Setup is measured separately and may include downloads. File writing
and playback are excluded from synthesis time. This is a small audition, not a
statistical performance benchmark.

Text:

> Hear can now turn written words into speech. This is a short test of clarity,
> pacing, and a natural conversational voice.

First chunk means audio available to the caller, not audible sound. The engines
can yield differently sized chunks, so first-chunk times must be read alongside
chunk count. OpenAI measurements include network transfer. Its server's model
loading state is unknown. Local setup and warmed-up generation are different
measurements.

The optional `tools/compare-local-tts.py` harness writes PCM16 WAV files and JSON
per-run metadata. Audio, weights, and the experimental environment are stored
outside the repository, under `~/.cache/hear/tts-comparison/` on the test machine.

## Results

The user selected **Pocket TTS as the preferred local voice** after hearing all
three local samples. Cedar is the selected OpenAI voice. Pocket is the first
candidate for local integration; this change implements only the OpenAI backend.

Warmed-up local trials (runs 2–3):

| Engine / voice | First chunk | Total generation | Audio duration | Chunks |
| --- | --- | --- | --- | --- |
| Piper / Lessac medium | 0.090–0.093 s | 0.282 s | 7.06–7.14 s | 2 |
| Kokoro / Heart, whole paragraph | 2.89–2.91 s | 2.89–2.91 s | 8.23 s | 1 |
| Kokoro / Heart, sentence splitting | 0.92–1.21 s | 2.70–3.11 s | 8.15 s | 2 |
| Pocket / Alba | 0.102–0.106 s | 1.93–2.23 s | 6.56–7.28 s | 82–91 |

One OpenAI Cedar request with the same text returned first PCM after 0.80 s and
finished in 3.92 s, producing 7.95 s of audio. This was a single cloud request,
not a warmed-up local trial.

Pocket delivers roughly 80 ms audio chunks, making it a good match for continuous
playback. Piper was the fastest overall in this test, delivering one sentence
per chunk. Kokoro can improve first-audio latency by splitting the text into
sentences; the default paragraph configuration returned all audio at once for
this input. These are measured configurations, not limits of every possible
runtime or optimization for these models.

Full per-run timings, setup measurements, dependency versions, and model details
are in `docs/tts-measurements.json`. Setup includes imports and model/voice loading;
Kokoro and Pocket also downloaded weights during their initial setup. Piper's
model was downloaded beforehand. Setup times are therefore not comparable.
The installed CPU Torch version was 2.14.0+cpu, with Python 3.14.2. Auditions used
trial 2 from the three engines' default configurations.

## Reproduce

Use a separate Python environment; these are not Hear runtime dependencies:

```sh
uv venv /tmp/hear-tts-env
uv pip install --python /tmp/hear-tts-env/bin/python --torch-backend cpu \
  kokoro==0.9.4 pocket-tts==3.3.0 piper-tts==1.8.0 soundfile
uv pip install --python /tmp/hear-tts-env/bin/python \
  https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl
mkdir -p /tmp/hear-tts-models /tmp/hear-tts-results
(cd /tmp/hear-tts-models && /tmp/hear-tts-env/bin/python -m piper.download_voices en_US-lessac-medium)
/tmp/hear-tts-env/bin/python tools/compare-local-tts.py kokoro --output-dir /tmp/hear-tts-results
/tmp/hear-tts-env/bin/python tools/compare-local-tts.py kokoro --sentence-chunks --output-dir /tmp/hear-tts-results
/tmp/hear-tts-env/bin/python tools/compare-local-tts.py pocket --output-dir /tmp/hear-tts-results
/tmp/hear-tts-env/bin/python tools/compare-local-tts.py piper --output-dir /tmp/hear-tts-results \
  --piper-model /tmp/hear-tts-models/en_US-lessac-medium.onnx
```

Run engines separately to avoid CPU contention. Initial runs download model
weights. Pocket uses the `english_2026-09` model and Alba; Kokoro uses American
English and `af_heart`; Piper uses `en_US-lessac-medium`.

## Upstream references

- OpenAI speech API: https://developers.openai.com/api/docs/guides/text-to-speech
- Kokoro implementation: https://github.com/hexgrad/kokoro
- Kokoro model: https://huggingface.co/hexgrad/Kokoro-82M
- Pocket TTS: https://github.com/kyutai-labs/pocket-tts
- Piper: https://github.com/OHF-Voice/piper1-gpl

Kokoro's weights use Apache-2.0. Pocket's code uses MIT. Piper's engine uses GPL-3.0;
account for that when deciding how a future backend will be distributed. Engine
and individual voice/model licenses should be checked separately before shipping.
