"""Offline experimental PP-OCRv6 conversion; never modifies installed models.

Install scripts/onnx-tools-requirements.txt into a separate Python environment.
The desktop and native Worker do not depend on Python.
"""
import argparse
import copy
import hashlib
import importlib.metadata
import json
import math
from pathlib import Path
import re
import shutil
import tempfile
from datetime import datetime, timezone

import numpy as np
import onnx
from onnxruntime.transformers import float16
from onnxruntime.transformers.onnx_model import OnnxModel
from onnxruntime.quantization import (
    CalibrationDataReader, CalibrationMethod, QuantFormat, QuantType,
    quantize_static,
)
from onnxruntime.quantization.shape_inference import quant_pre_process
from PIL import Image


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path):
    return json.loads(path.read_text(encoding="utf-8-sig"))


def source_files(manifest_path, manifest):
    if (manifest.get("schemaVersion"), manifest.get("runtime"),
            manifest.get("adapter")) != (2, "onnx", "ppocrv6"):
        raise ValueError("Expected a schema v2 PP-OCRv6 ONNX manifest")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", manifest["id"]):
        raise ValueError("Unsafe model ID")
    files = {}
    for entry in manifest["files"]:
        name = entry["path"]
        if not name or name in (".", "..") or any(c in name for c in "/\\:"):
            raise ValueError("Manifest file paths must be plain filenames")
        path = manifest_path.parent / name
        if path.stat().st_size != entry["size"] or sha256(path) != entry["sha256"]:
            raise ValueError(f"Source file size/hash mismatch: {path}")
        if entry["role"] in files:
            raise ValueError("Duplicate file role")
        files[entry["role"]] = path
    if set(files) != {"detector", "recognizer", "dictionary"}:
        raise ValueError("Expected detector, recognizer and dictionary files")
    return files


def calibration_items(corpus_path, maximum):
    root = corpus_path.parent.resolve()
    scenes = read_json(corpus_path)["scenarios"]
    detectors, recognizers, images = [], [], []
    for scene in scenes:
        path = (root / scene["image"]).resolve()
        if not path.is_relative_to(root):
            raise ValueError("Calibration image must be inside the corpus directory")
        if not path.is_file():
            raise ValueError(f"Missing calibration image: {path}")
        images.append({"image": scene["image"], "sha256": sha256(path)})
        detectors.append((path, None))
        for line in scene["groundTruth"]:
            box = line["bbox"]
            if len(box) != 4 or not all(math.isfinite(x) for x in box):
                raise ValueError("Invalid calibration bounding box")
            recognizers.append((path, box))
    # Spread selection through the corpus instead of using only its first scene.
    def sample(items):
        if not items:
            raise ValueError("Empty calibration data")
        indices = np.linspace(0, len(items) - 1, min(maximum, len(items)), dtype=int)
        return [items[i] for i in indices]
    return sample(detectors), sample(recognizers), images


class Reader(CalibrationDataReader):
    def __init__(self, role, name, items):
        self.role, self.name, self.items = role, name, items
        self.rewind()

    def rewind(self):
        self.iterator = iter(self.items)

    def get_next(self):
        item = next(self.iterator, None)
        if item is None:
            return None
        path, box = item
        with Image.open(path) as source:
            image = source.convert("RGB")
        if self.role == "detector":
            ratio = min(1.0, 960 / max(image.size))
            width, height = [max(32, math.floor(x * ratio / 32 + 0.5) * 32)
                             for x in image.size]
            rgb = np.asarray(image.resize((width, height), Image.Resampling.BILINEAR),
                             dtype=np.float32)
            array = (rgb[:, :, ::-1] / 255 - [0.485, 0.456, 0.406]) / [0.229, 0.224, 0.225]
        else:
            left, top, right, bottom = box
            bounds = (max(0, math.floor(left)), max(0, math.floor(top)),
                      min(image.width, math.ceil(right)), min(image.height, math.ceil(bottom)))
            if bounds[2] <= bounds[0] or bounds[3] <= bounds[1]:
                raise ValueError("Empty calibration text crop")
            image = image.crop(bounds)
            if image.height >= image.width * 1.5:
                image = image.transpose(Image.Transpose.ROTATE_90)
            width = min(3200, max(1, math.ceil(48 * image.width / image.height)))
            padded = max(320, math.ceil(width / 64) * 64)
            array = np.zeros((48, padded, 3), dtype=np.float32)
            rgb = np.asarray(image.resize((width, 48), Image.Resampling.BILINEAR),
                             dtype=np.float32)
            array[:, :width] = rgb[:, :, ::-1] / 127.5 - 1
        return {self.name: np.ascontiguousarray(array.transpose(2, 0, 1)[None],
                                              dtype=np.float32)}


def check_model(path):
    model = onnx.load(path)
    onnx.checker.check_model(model)
    for value in [*model.graph.input, *model.graph.output]:
        if value.type.tensor_type.elem_type != onnx.TensorProto.FLOAT:
            raise ValueError("Converted model must preserve float32 I/O")


def convert_fp16(model, blocked):
    model = float16.convert_float_to_float16(model, keep_io_types=True, op_block_list=blocked)
    OnnxModel(model).topological_sort()
    return model


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True, help="New directory; must not exist")
    parser.add_argument("--mode", choices=["fp16", "int8"], required=True)
    parser.add_argument("--calibration", type=Path, default=Path("tests/fixtures/parallel/corpus.json"))
    parser.add_argument("--max-samples", type=int, default=64)
    args = parser.parse_args()
    if not 1 <= args.max_samples <= 1024:
        parser.error("--max-samples must be between 1 and 1024")
    manifest = read_json(args.manifest)
    files = source_files(args.manifest, manifest)
    if args.output.exists():
        parser.error("Output already exists; choose a new directory")
    calibration = None
    if args.mode == "int8":
        detectors, recognizers, images = calibration_items(args.calibration, args.max_samples)
        calibration = {"corpus": str(args.calibration), "sha256": sha256(args.calibration),
                       "images": images, "detectorSamples": len(detectors),
                       "recognizerSamples": len(recognizers),
                       "note": "Synthetic axis-aligned GT crops; evaluation corpus is separate"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # Publish a manifest only after every graph passes ONNX validation. A failed
    # conversion leaves no partially installed model or overwritten source.
    with tempfile.TemporaryDirectory(prefix="onnx-convert-", dir=args.output.parent) as temp:
        stage = Path(temp)
        output = copy.deepcopy(manifest)
        output["id"] += "-" + args.mode + "-experimental"
        output["name"] += " " + args.mode.upper() + " (experimental)"
        output["quantization"] = "FP16 mixed" if args.mode == "fp16" else "INT8 QDQ"
        output["installed"], output["imported"] = True, True
        output["files"] = []
        blocked = sorted(set(float16.DEFAULT_OP_BLOCK_LIST) | {
            "Softmax", "Sigmoid", "LayerNormalization", "ReduceMean", "Pow",
        })
        for role, source in files.items():
            name = output["id"] + "-" + role + (".txt" if role == "dictionary" else ".onnx")
            destination = stage / name
            print(f"Converting {role}: {args.mode}", flush=True)
            if role == "dictionary":
                shutil.copyfile(source, destination)
            elif args.mode == "fp16":
                model = convert_fp16(onnx.load(source), blocked)
                onnx.save(model, destination)
                check_model(destination)
            else:
                prepared = stage / f"prepared-{role}.onnx"
                model = onnx.load(source)
                # Per-channel QDQ requires axis support (opset >= 13).
                if next(o.version for o in model.opset_import if o.domain == "") < 13:
                    model = onnx.version_converter.convert_version(model, 14)
                quant_pre_process(model, str(prepared), skip_optimization=True,
                                  skip_symbolic_shape=True)
                model = onnx.load(prepared)
                items = detectors if role == "detector" else recognizers
                quantize_static(
                    str(prepared), str(destination), Reader(role, model.graph.input[0].name, items),
                    quant_format=QuantFormat.QDQ, activation_type=QuantType.QInt8,
                    weight_type=QuantType.QInt8, per_channel=True,
                    op_types_to_quantize=["Conv", "MatMul", "Gemm"],
                    calibrate_method=CalibrationMethod.MinMax,
                    calibration_providers=["CPUExecutionProvider"],
                )
                prepared.unlink()
                check_model(destination)
            output["files"].append({"role": role, "path": name,
                                    "size": destination.stat().st_size, "sha256": sha256(destination)})
        output["parameters"]["experimental"] = {
            "mode": args.mode, "evaluatedDevice": "cuda" if args.mode == "fp16" else "cpu",
            "sourceId": manifest["id"],
        }
        metadata = {
            "createdAt": datetime.now(timezone.utc).isoformat(),
            "sourceManifestSha256": sha256(args.manifest),
            "sourceFiles": manifest["files"], "calibration": calibration,
            "versions": {name: importlib.metadata.version(name) for name in
                         ["onnx", "onnxruntime", "numpy", "Pillow"]},
            "configuration": {"mode": args.mode, "keepIoFloat32": True,
                              "float32Ops": blocked if args.mode == "fp16" else None,
                              "int8Ops": ["Conv", "MatMul", "Gemm"] if args.mode == "int8" else None,
                              "perChannel": args.mode == "int8",
                              "calibrationMethod": "MinMax" if args.mode == "int8" else None},
        }
        (stage / "conversion-metadata.json").write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
        (stage / (output["id"] + ".json")).write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
        # mkdir is exclusive even if another invocation finishes concurrently.
        args.output.mkdir()
        for path in stage.iterdir():
            shutil.copyfile(path, args.output / path.name)
    print(f"Experimental model written to {args.output}. Run native quality/speed evaluation before use.")


if __name__ == "__main__":
    main()
