#!/usr/bin/env python3
"""Optional CPU audition harness, outside Hear's runtime dependencies.

Install kokoro, pocket-tts, piper-tts, soundfile into a separate environment.
Download Piper's en_US-lessac-medium model with `python -m piper.download_voices`.
Run each engine separately so model loading and CPU contention do not overlap.
"""
import argparse
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("engine", choices=["kokoro", "pocket", "piper"])
parser.add_argument("--output-dir", type=Path, required=True)
parser.add_argument("--piper-model", type=Path)
parser.add_argument("--sentence-chunks", action="store_true", help="Split Kokoro input into sentences")
args = parser.parse_args()
args.output_dir.mkdir(parents=True, exist_ok=True)
os.environ.setdefault("OMP_NUM_THREADS", "2")
os.environ.setdefault("MKL_NUM_THREADS", "2")
import numpy as np
import soundfile as sf

text = "Hear can now turn written words into speech. This is a short test of clarity, pacing, and a natural conversational voice."
start = time.perf_counter()
if args.engine == "kokoro":
    import torch
    from kokoro import KPipeline
    torch.set_num_threads(2)
    model = KPipeline(lang_code="a", device="cpu")
    voice = "af_heart"
    # Include voice download/loading in setup, not first synthesis.
    model.load_voice(voice)
    sample_rate = 24000
    package = "kokoro"
    def generate():
        for _, _, audio in model(text, voice=voice, split_pattern=r"(?<=[.!?])\s+" if args.sentence_chunks else r"\n+"):
            yield audio.detach().cpu().numpy()
elif args.engine == "pocket":
    import torch
    from pocket_tts import TTSModel
    torch.set_num_threads(2)
    model = TTSModel.load_model(language="english_2026-09")
    voice = "alba"
    state = model.get_state_for_audio_prompt(voice)
    sample_rate = model.sample_rate
    package = "pocket-tts"
    def generate():
        for audio in model.generate_audio_stream(state, text):
            yield audio.detach().cpu().numpy()
else:
    import onnxruntime as ort
    from piper import PiperVoice
    from piper.config import PiperConfig
    if args.piper_model is None:
        parser.error("--piper-model is required for Piper")
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    model = PiperVoice(
        config=PiperConfig.from_dict(json.loads(Path(str(args.piper_model) + ".json").read_text())),
        session=ort.InferenceSession(str(args.piper_model), sess_options=options,
                                     providers=["CPUExecutionProvider"]),
    )
    voice = args.piper_model.stem
    sample_rate = model.config.sample_rate
    package = "piper-tts"
    def generate():
        for chunk in model.synthesize(text):
            yield np.frombuffer(chunk.audio_int16_bytes, dtype="<i2").astype(np.float32) / 32768

setup = time.perf_counter() - start
label = args.engine + ("-sentences" if args.engine == "kokoro" and args.sentence_chunks else "")
runs = []
for trial in range(3):
    chunks = []
    start = time.perf_counter()
    first = None
    for chunk in generate():
        if first is None:
            first = time.perf_counter() - start
        chunks.append(chunk)
    elapsed = time.perf_counter() - start
    audio = np.concatenate(chunks)
    output = args.output_dir / f"{label}-{voice}-{trial + 1}.wav"
    sf.write(output, audio, sample_rate, subtype="PCM_16")
    result = dict(trial=trial + 1, first_chunk_seconds=first,
                  synthesis_seconds=elapsed, audio_seconds=len(audio) / sample_rate,
                  chunks=len(chunks), path=str(output))
    runs.append(result)
    print(json.dumps(result), flush=True)
report = dict(engine=args.engine, voice=voice, version=importlib.metadata.version(package),
              platform=platform.platform(), python=platform.python_version(), text=text, sample_rate=sample_rate,
              sentence_chunks=args.sentence_chunks,
              dependencies={name: importlib.metadata.version(name) for name in ["torch", "numpy", "onnxruntime", "transformers"]},
              cpu_threads=2, setup_including_download_seconds=setup, runs=runs)
(args.output_dir / f"{label}-results.json").write_text(json.dumps(report, indent=2) + "\n")
