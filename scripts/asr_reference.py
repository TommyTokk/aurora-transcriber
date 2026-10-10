"""Generate offline Parakeet expectations with onnx-asr 0.12.0 and ORT 1.20.1.

Use only explicitly authorized test audio; output contains its reference text.
Install dependencies in a development environment, never in the application:
    pip install onnx-asr==0.12.0 onnxruntime==1.20.1 soundfile
"""

import argparse
import json
from importlib.metadata import version
from pathlib import Path

import onnx_asr
import onnxruntime as ort
import soundfile


def generate(model_dir: Path, cases: list[dict]) -> list[dict]:
    """Run the independent CPU implementation with the selected export's graph."""
    if version("onnx-asr") != "0.12.0" or ort.__version__ != "1.20.1":
        raise ValueError("Use onnx-asr 0.12.0 and ONNX Runtime 1.20.1")
    options = ort.SessionOptions()
    options.intra_op_num_threads = 4
    options.inter_op_num_threads = 1
    options.enable_cpu_mem_arena = False
    options.enable_mem_pattern = False
    model = onnx_asr.load_model(
        "nemo-conformer-tdt",
        model_dir,
        providers=["CPUExecutionProvider"],
        sess_options=options,
    )
    preprocessor = ort.InferenceSession(
        str(model_dir / "nemo128.onnx"),
        sess_options=options,
        providers=["CPUExecutionProvider"],
    )

    def preprocess(waveforms, lengths):
        """Use the pinned graph instead of the reference package's bundled copy."""
        return preprocessor.run(
            ["features", "features_lens"],
            {"waveforms": waveforms, "waveforms_lens": lengths},
        )

    # The private hook is intentional and tied to the reference version above.
    model.asr._preprocessor = preprocess
    for case in cases:
        samples, rate = soundfile.read(case["audio_path"], dtype="float32")
        if rate != 16000 or samples.ndim != 1 or not 0 < len(samples) / rate <= 25:
            raise ValueError("Reference cases must be mono 16 kHz chunks up to 25 seconds")
        case["reference_text"] = model.recognize(samples, sample_rate=rate)
        del samples
    return cases


def main():
    """Read local cases and write explicitly requested reference expectations."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model-dir", type=Path, required=True)
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    cases = generate(args.model_dir, json.loads(args.cases.read_text(encoding="utf-8")))
    args.output.write_text(json.dumps(cases, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
