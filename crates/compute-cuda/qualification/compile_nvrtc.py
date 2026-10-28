#!/usr/bin/env python3
"""Compile the production CUDA source with real NVRTC, without a CUDA driver.

Run inside the disposable Linux container created by qualify-nvrtc.sh. Only the
pinned NVIDIA runtime wheel is downloaded; no package setup code is executed.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import sys
import tempfile
import time
import urllib.request
import zipfile

PACKAGE = "nvidia-cuda-nvrtc-cu12"
VERSION = "12.8.93"
WHEELS = {
    "aarch64": (
        "nvidia_cuda_nvrtc_cu12-12.8.93-py3-none-manylinux2014_aarch64.manylinux_2_17_aarch64.whl",
        "fc1fec1e1637854b4c0a65fb9a8346b51dd9ee69e61ebaccc82058441f15bce8",
    ),
    "x86_64": (
        "nvidia_cuda_nvrtc_cu12-12.8.93-py3-none-manylinux2010_x86_64.manylinux_2_12_x86_64.whl",
        "a7756528852ef889772a84c6cd89d41dfa74667e24cca16bb31f8f061e3e9994",
    ),
}
ABI = {
    "conv_f32": ["u64"] * 5,
    "conv_low": ["u64"] * 5 + ["u32"],
    "unary": ["u64"] * 4 + ["u32", "u64", "u32", "f32", "f32"],
    "binary": ["u64"] * 5 + ["u32", "u64", "u64", "u32"],
    "binary_u32": ["u64"] * 5 + ["u32", "u64", "u64", "u32"],
    "reduce_axes": ["u64"] * 5 + ["u32", "u32", "u64", "u32"],
    "reduce_all": ["u64"] * 4 + ["u32", "u64", "u32"],
    "copy_u32": ["u64"] * 4 + ["u32", "u64"],
    "copy_f32": ["u64"] * 4 + ["u32", "u64"],
    "copy_low": ["u64"] * 4 + ["u32", "u64"],
    "cast_to_low": ["u64"] * 4 + ["u32", "u64", "u32"],
    "cast_from_low": ["u64"] * 4 + ["u32", "u64", "u32"],
    "invalid_indices": ["u64"] * 4 + ["u32", "u64", "u64"],
    "normalize_mask": ["u64"] * 4 + ["u32", "u64"],
    "fill_f32": ["u64", "u64", "f32"],
    "fill_u32": ["u64", "u64", "u32"],
    "scatter_owners": ["u64"] * 4 + ["u32", "u64", "u64"],
    "stats_partial": ["u64"] * 7 + ["u32", "u32", "u64", "u32"],
    "stats_merge": ["u64"] * 5 + ["u32"],
    "stats_emit": ["u64"] * 7 + ["u32", "u32", "u64", "u32", "f32"],
    "stats_lse": ["u64"] * 4,
    "stats_moments": ["u64"] * 7 + ["u32", "u32", "u64"],
    "attention_f32": ["u64"] * 12 + ["u32", "f32", "u32", "u32"],
    "unary_low": ["u64"] * 4 + ["u32", "u64", "u32", "u32"],
    "binary_low": ["u64"] * 5 + ["u32", "u64", "u64", "u32", "u32"],
}
ABI["unary_f64"] = ["u64"] * 4 + ["u32", "u64", "u32", "f64", "f64"]
ABI["binary_f64"] = ABI["binary"]
ABI["reduce_axes_f64"] = ABI["reduce_axes"]
ABI["reduce_all_f64"] = ABI["reduce_all"]
ABI["copy_f64"] = ABI["copy_f32"]
ABI["fill_f64"] = ["u64", "u64", "f64"]
ABI["reduce_axes_u32"] = ABI["reduce_axes"]
ABI["reduce_all_u32"] = ABI["reduce_all"]
ABI["reduce_axes_low"] = ABI["reduce_axes"] + ["u32"]
ABI["reduce_all_low"] = ABI["reduce_all"] + ["u32"]
for _dtype in ["f32", "u32", "f64"]:
    ABI[f"compare_{_dtype}"] = ABI["binary"]
    ABI[f"where_{_dtype}"] = ["u64"] * 6 + ["u32"] + ["u64"] * 3
    ABI[f"scan_{_dtype}"] = ["u64"] * 8 + ["u32", "u64", "u64", "u32", "u32"]
    ABI[f"add_scan_{_dtype}"] = ["u64"] * 6 + ["u32"]
    ABI[f"gather_{_dtype}"] = ["u64"] * 8 + ["u32", "u32", "u64", "u64"]
    ABI[f"compact_{_dtype}"] = ["u64"] * 7 + ["u32", "u64"]
    ABI[f"scatter_{_dtype}"] = ["u64"] * 9 + ["u32", "u32", "u64", "u64", "u32"]
ABI["compare_low"] = ABI["compare_f32"] + ["u32"]
ABI["where_low"] = ABI["where_f32"]
ABI["gather_low"] = ABI["gather_f32"]
ABI["compact_low"] = ABI["compact_f32"]
ABI["scan_low_f32"] = ABI["scan_f32"] + ["u32"]
ABI["attention_low"] = ABI["attention_f32"] + ["u32"]
ABI["scatter_low_raw"] = ABI["scatter_f32"]
ABI["scatter_low_f32"] = ABI["scatter_f32"] + ["u32"]
for _name in ["stats_partial", "stats_emit", "stats_moments"]:
    ABI[f"{_name}_low"] = ABI[_name] + ["u32"]


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def ptx_metadata(ptx):
    parameters = {
        name: re.findall(r"\.param\s+\.(\w+)\s+", arguments)
        for name, arguments in re.findall(r"\.visible\s+\.entry\s+(\w+)\s*\((.*?)\)", ptx.decode(), re.S)
    }
    if parameters != ABI:
        raise RuntimeError(f"PTX entry-point parameter ABI differs: {parameters}")
    return {"entry_points": sorted(parameters), "parameter_types": parameters}


def download(url):
    with urllib.request.urlopen(url, timeout=90) as response:
        return response.read()


def extract_libraries(directory):
    filename, expected_hash = WHEELS[platform.machine()]
    metadata_url = f"https://pypi.org/pypi/{PACKAGE}/{VERSION}/json"
    metadata = json.loads(download(metadata_url))
    entry = next(item for item in metadata["urls"] if item["filename"] == filename)
    if entry["digests"]["sha256"] != expected_hash:
        raise RuntimeError("PyPI digest does not match the pinned NVIDIA wheel")
    data = download(entry["url"])
    if sha256(data) != expected_hash:
        raise RuntimeError("Downloaded wheel SHA256 differs from the pinned digest")
    wheel = directory / filename
    wheel.write_bytes(data)
    files = []
    with zipfile.ZipFile(wheel) as archive:
        # Flatten only the shared-library directory; never execute wheel Python
        # or setup code, and never trust an archive path for a filesystem write.
        for name in archive.namelist():
            path = Path(name)
            if str(path.parent) == "nvidia/cuda_nvrtc/lib" and ".so" in path.name:
                destination = directory / path.name
                destination.write_bytes(archive.read(name))
                files.append({"name": path.name, "sha256": sha256(destination.read_bytes())})
    libraries = sorted(directory.glob("libnvrtc.so*"))
    if len(libraries) != 1:
        raise RuntimeError(f"Expected one NVRTC library, found {libraries}")
    return libraries[0], {
        "package": PACKAGE, "version": VERSION, "filename": filename,
        "sha256": expected_hash, "download_url": entry["url"], "libraries": files,
    }


class Nvrtc:
    def __init__(self, path):
        self.lib = ctypes.CDLL(str(path))
        self.bind("nvrtcVersion", [ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int)])
        self.bind("nvrtcGetNumSupportedArchs", [ctypes.POINTER(ctypes.c_int)])
        self.bind("nvrtcGetSupportedArchs", [ctypes.POINTER(ctypes.c_int)])
        self.bind("nvrtcCreateProgram", [ctypes.POINTER(ctypes.c_void_p), ctypes.c_char_p,
                  ctypes.c_char_p, ctypes.c_int, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_char_p)])
        self.bind("nvrtcCompileProgram", [ctypes.c_void_p, ctypes.c_int, ctypes.POINTER(ctypes.c_char_p)])
        self.bind("nvrtcGetProgramLogSize", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)])
        self.bind("nvrtcGetProgramLog", [ctypes.c_void_p, ctypes.c_char_p])
        self.bind("nvrtcGetPTXSize", [ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t)])
        self.bind("nvrtcGetPTX", [ctypes.c_void_p, ctypes.c_char_p])
        self.bind("nvrtcDestroyProgram", [ctypes.POINTER(ctypes.c_void_p)])
        self.lib.nvrtcGetErrorString.argtypes = [ctypes.c_int]
        self.lib.nvrtcGetErrorString.restype = ctypes.c_char_p

    def bind(self, name, args):
        function = getattr(self.lib, name)
        function.argtypes = args
        function.restype = ctypes.c_int

    def check(self, code):
        if code:
            raise RuntimeError(self.lib.nvrtcGetErrorString(code).decode())

    def version(self):
        major, minor = ctypes.c_int(), ctypes.c_int()
        self.check(self.lib.nvrtcVersion(ctypes.byref(major), ctypes.byref(minor)))
        return [major.value, minor.value]

    def architectures(self):
        count = ctypes.c_int()
        self.check(self.lib.nvrtcGetNumSupportedArchs(ctypes.byref(count)))
        if not 0 < count.value < 1024:
            raise RuntimeError("Invalid architecture count")
        architectures = (ctypes.c_int * count.value)()
        self.check(self.lib.nvrtcGetSupportedArchs(architectures))
        return list(architectures)

    def compile(self, source, options):
        program = ctypes.c_void_p()
        # cudarc defaults to no source name and no supplied headers.
        self.check(self.lib.nvrtcCreateProgram(ctypes.byref(program), source, None, 0, None, None))
        try:
            encoded = (ctypes.c_char_p * len(options))(*(s.encode() for s in options))
            started = time.monotonic()
            code = self.lib.nvrtcCompileProgram(program, len(options), encoded)
            seconds = time.monotonic() - started
            log_size = ctypes.c_size_t()
            self.check(self.lib.nvrtcGetProgramLogSize(program, ctypes.byref(log_size)))
            log = ctypes.create_string_buffer(log_size.value)
            self.check(self.lib.nvrtcGetProgramLog(program, log))
            result = {"status": code, "seconds": seconds, "log": log.value.decode(errors="replace")}
            if code == 0:
                size = ctypes.c_size_t()
                self.check(self.lib.nvrtcGetPTXSize(program, ctypes.byref(size)))
                ptx = ctypes.create_string_buffer(size.value)
                self.check(self.lib.nvrtcGetPTX(program, ptx))
                # Persist textual PTX without the API's trailing C-string NUL.
                result["ptx"] = ptx.value
            return result
        finally:
            self.check(self.lib.nvrtcDestroyProgram(ctypes.byref(program)))


def production_source(crate):
    runtime = (crate / "src/runtime.rs").read_text()
    declaration = runtime.split("pub const CUDA_KERNEL_SOURCE:", 1)[1].split(";", 1)[0]
    parts = re.findall(r'include_str!\("([^"\\]+)"\)', declaration)
    if parts != ["kernels.cu", "indexing.cu", "reductions.cu", "scatter.cu", "low_precision.cu", "statistics.cu", "attention.cu", "low_ops.cu", "low_indexing.cu", "low_scatter.cu", "low_statistics.cu", "low_attention.cu", "convolution.cu", "float64.cu", "float64_indexing.cu"]:
        raise RuntimeError(f"Review source concatenation before qualification: {parts}")
    contents = [(name, (crate / "src" / name).read_bytes()) for name in parts]
    options = []
    # Same argument order as cudarc 0.19.9 CompileOptions::build.
    for field, flag in [("ftz", "ftz"), ("prec_sqrt", "prec-sqrt"), ("prec_div", "prec-div"), ("fmad", "fmad")]:
        found = re.findall(rf"\b{field}:\s*Some\((true|false)\)", runtime)
        if len(found) != 1:
            raise RuntimeError(f"Review production compiler option {field}")
        options.append(f"--{flag}={found[0]}")
    if "use_fast_math:" in runtime or "include_paths:" in runtime:
        raise RuntimeError("Review extra production compiler options")
    return b"".join(data for _, data in contents), options, {
        name: {"bytes": len(data), "sha256": sha256(data)} for name, data in contents
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--crate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--compute-capabilities", type=int, nargs="+", default=[70, 80, 90, 120])
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    source, base_options, parts = production_source(args.crate)
    report = {
        "qualification": "NVRTC source compilation only; no CUDA driver, GPU execution, cuBLAS or numerical validation",
        "created_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "container_image": os.environ.get("CUDA_NVRTC_IMAGE_ID", "unknown"),
        "host_architecture": platform.machine(), "python": platform.python_version(),
        "source": {"parts": parts, "bytes": len(source), "sha256": sha256(source)},
        "requested_compute_capabilities": args.compute_capabilities, "compilations": [],
    }
    with tempfile.TemporaryDirectory(prefix="nvrtc-qualification-") as temporary:
        library, wheel = extract_libraries(Path(temporary))
        compiler = Nvrtc(library)
        report.update(wheel=wheel, nvrtc_version=compiler.version(), supported_architectures=compiler.architectures())
        print(f"NVRTC {report['nvrtc_version']}; supported targets {report['supported_architectures']}", flush=True)
        for capability in args.compute_capabilities:
            # The runtime applies this same max-supported <= device policy;
            # these are requested target capabilities, not observed GPU devices.
            architecture = max(a for a in report["supported_architectures"] if a <= capability)
            options = base_options + [f"--gpu-architecture=compute_{architecture}"]
            result = compiler.compile(source, options)
            log_name = f"compute-{architecture}.log"
            (args.output / log_name).write_text(result["log"])
            entry = {"requested_capability": capability, "architecture": architecture,
                     "options": options, "status": result["status"], "compile_seconds": result["seconds"], "log": log_name}
            if "ptx" in result:
                ptx = result["ptx"]
                ptx_name = f"compute-{architecture}.ptx"
                (args.output / ptx_name).write_bytes(ptx)
                entry.update(ptx=ptx_name, ptx_bytes=len(ptx), ptx_sha256=sha256(ptx))
                try:
                    entry.update(ptx_metadata(ptx))
                except RuntimeError as error:
                    entry["entry_point_error"] = str(error)
            report["compilations"].append(entry)
            (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps({key: entry[key] for key in [
                "architecture", "status", "compile_seconds", "ptx_bytes", "ptx_sha256"
            ] if key in entry} | {"entry_count": len(entry.get("entry_points", []))}), flush=True)
        success = all(c["status"] == 0 and "entry_point_error" not in c for c in report["compilations"])
        report["success"] = success
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        return 0 if success else 1


if __name__ == "__main__":
    sys.exit(main())
