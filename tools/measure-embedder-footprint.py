#!/usr/bin/env python3
"""Build and measure one minimal-embedder configuration at a time.

The tool deliberately makes one Cargo invocation for every requested row. This
keeps feature unification and multi-bin builds out of the footprint data.
"""

import argparse
import json
import os
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple

from _measure_support import MeasurementError, median, peak_rss_samples, run, wall_ms_samples


WARMUP = 3
RUNS = 30
RSS_RUNS = 5

CONFIGS = {
    "baseline": {
        "bin": "embed-baseline",
        "features": [],
        "fixture": "examples/add.wasm",
        "expected_stdout": "335\n",
    },
    "core": {
        "bin": "embed-core",
        "features": ["simd"],
        "fixture": "examples/add.wasm",
        "expected_stdout": "3\n",
    },
    "component": {
        "bin": "embed-component",
        "features": ["simd", "component"],
        "fixture": "examples/component-add.wasm",
        "expected_stdout": "42\n",
    },
    "wasi": {
        "bin": "embed-wasi",
        "features": ["simd", "wasi"],
        "fixture": "examples/wasi-component-args.wasm",
        "expected_stdout": "0\n",
    },
    "core-nosimd": {
        "bin": "embed-core",
        "features": [],
        "fixture": "examples/add.wasm",
        "expected_stdout": "3\n",
    },
    "core-jit": {
        "bin": "embed-core",
        "features": ["simd", "jit"],
        "fixture": "examples/add.wasm",
        "expected_stdout": "3\n",
    },
    "wasi-threads": {
        "bin": "embed-wasi",
        "features": ["simd", "threads", "wasi"],
        "fixture": "examples/wasi-component-args.wasm",
        "expected_stdout": "0\n",
    },
}

MACOS_TEXT = re.compile(r"^\s*Section __text:\s*(\d+)\s*$", re.M)
MACOS_TEXT_ALTERNATE = re.compile(r"__TEXT\s*,\s*__text\s+(\d+)", re.M)
ELF_TEXT = re.compile(r"^\s*\.text\s+(\d+)\s+", re.M)
MACHO_MAGICS = {
    b"\xfe\xed\xfa\xce",
    b"\xce\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf",
    b"\xcf\xfa\xed\xfe",
}


class FootprintError(RuntimeError):
    """A build, run, or binary inspection failed before a result was valid."""


def command_text(command: Sequence[str]) -> str:
    return shlex.join(list(command))


def parse_configs(value: str) -> List[str]:
    names = [name.strip() for name in value.split(",") if name.strip()]
    if not names:
        raise FootprintError("--configs must select at least one configuration")

    unknown = [name for name in names if name not in CONFIGS]
    if unknown:
        raise FootprintError(
            "unknown configuration(s): " + ", ".join(unknown)
        )
    if len(names) != len(set(names)):
        raise FootprintError("--configs must not repeat a configuration")
    return names


def cargo_command(config: Dict[str, object], profile: str, target: Optional[str]) -> List[str]:
    command = [
        "cargo",
        "build",
        "-p",
        "telomere-minimal-embedder",
        "--profile",
        profile,
        "--no-default-features",
        "--bin",
        str(config["bin"]),
    ]
    features = config["features"]
    if features:
        command.extend(["--features", ",".join(features)])
    if target:
        command.extend(["--target", target])
    return command


def output_path(config: Dict[str, object], profile: str, target: Optional[str]) -> Path:
    root = Path("target")
    if target:
        root /= target
    return root / profile / str(config["bin"])


def run_checked(command: Sequence[str]) -> None:
    completed = subprocess.run(
        list(command),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode:
        detail = completed.stderr.rstrip("\n")
        message = f"`{command_text(command)}` exited with status {completed.returncode}"
        if detail:
            message = f"{message}\n{detail}"
        raise FootprintError(message)


def configured_tool(value: Optional[str], tool_name: str) -> Optional[List[str]]:
    if value is not None:
        command = shlex.split(value)
        if not command:
            raise FootprintError(f"--{tool_name} must not be empty")
        return command

    rustup = subprocess.run(
        ["rustup", "which", tool_name],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    candidate = rustup.stdout.strip()
    if rustup.returncode == 0 and candidate and os.path.isfile(candidate):
        return [candidate]

    resolved = shutil.which(tool_name)
    if resolved:
        return [resolved]
    return None


def resolve_size_tool(value: Optional[str]) -> List[str]:
    if value is not None:
        configured = shlex.split(value)
        if not configured:
            raise FootprintError("--size-tool must not be empty")
        return configured

    llvm_size = configured_tool(None, "llvm-size")
    if llvm_size:
        return llvm_size
    system_size = shutil.which("size")
    if system_size:
        return [system_size]
    raise FootprintError("could not find rustup llvm-size, llvm-size, or size")


def resolve_strip_tool() -> List[str]:
    llvm_strip = configured_tool(None, "llvm-strip")
    if llvm_strip:
        return llvm_strip
    system_strip = shutil.which("strip")
    if system_strip:
        return [system_strip]
    raise FootprintError("could not find rustup llvm-strip, llvm-strip, or strip")


def binary_format(path: Path) -> str:
    with path.open("rb") as binary:
        magic = binary.read(4)
    if magic in MACHO_MAGICS:
        return "macho"
    if magic == b"\x7fELF":
        return "elf"
    raise FootprintError(f"could not identify Mach-O or ELF format for {path}")


def tool_output(command: Sequence[str]) -> str:
    completed = subprocess.run(
        list(command),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode:
        detail = completed.stderr.rstrip("\n")
        message = f"`{command_text(command)}` exited with status {completed.returncode}"
        if detail:
            message = f"{message}\n{detail}"
        raise FootprintError(message)
    return completed.stdout


def text_section(path: Path, size_tool: Sequence[str]) -> Dict[str, object]:
    format_name = binary_format(path)
    if format_name == "macho":
        attempts = [["-m"], ["--format=darwin"]]
        patterns = (MACOS_TEXT, MACOS_TEXT_ALTERNATE)
        section_name = "__TEXT,__text"
    else:
        attempts = [["-A"]]
        patterns = (ELF_TEXT,)
        section_name = ".text"

    failures = []
    for arguments in attempts:
        command = list(size_tool) + arguments + [str(path)]
        try:
            output = tool_output(command)
        except FootprintError as error:
            failures.append(str(error))
            continue
        for pattern in patterns:
            match = pattern.search(output)
            if match:
                return {
                    "format": format_name,
                    "section": section_name,
                    "bytes": int(match.group(1)),
                    "command": command_text(command),
                }
        failures.append(
            f"`{command_text(command)}` did not report {section_name}"
        )

    raise FootprintError("; ".join(failures))


def strip_command(strip_tool: Sequence[str], path: Path, format_name: str) -> List[str]:
    if format_name == "macho":
        return list(strip_tool) + [str(path)]
    return list(strip_tool) + ["--strip-all", str(path)]


def artifact_measurement(
    path: Path,
    profile: str,
    size_tool: Sequence[str],
    strip_tool: Sequence[str],
) -> Dict[str, object]:
    if not path.is_file():
        raise FootprintError(f"missing expected build output {path}")

    format_name = binary_format(path)
    result = {
        "path": str(path),
        "file_bytes": path.stat().st_size,
        "text_section": text_section(path, size_tool),
    }
    if profile != "release":
        result.update(
            {
                "stripped_copy_created": False,
                "stripped_file_bytes": path.stat().st_size,
                "stripped_text_section": text_section(path, size_tool),
                "strip_command": None,
            }
        )
        return result

    with tempfile.TemporaryDirectory(prefix="telomere-embedder-strip-") as temp_dir:
        stripped_path = Path(temp_dir) / path.name
        shutil.copy2(path, stripped_path)
        command = strip_command(strip_tool, stripped_path, format_name)
        run_checked(command)
        result.update(
            {
                "stripped_copy_created": True,
                "stripped_file_bytes": stripped_path.stat().st_size,
                "stripped_text_section": text_section(stripped_path, size_tool),
                "strip_command": command_text(command),
            }
        )
    return result


def runtime_measurement(command: Sequence[str], expected_stdout: str) -> Dict[str, object]:
    wall_samples = wall_ms_samples(command, WARMUP, RUNS, expected_stdout)
    rss_samples, timed_command = peak_rss_samples(command, RSS_RUNS, expected_stdout)
    return {
        "warmup_runs": WARMUP,
        "cold_start_runs": RUNS,
        "cold_start_command": command_text(command),
        "cold_start_wall_ms_samples": wall_samples,
        "cold_start_wall_ms_median": median(wall_samples),
        "peak_rss_runs": RSS_RUNS,
        "peak_rss_command": command_text(timed_command),
        "peak_rss_bytes_samples": rss_samples,
        "peak_rss_bytes_median": median(rss_samples),
    }


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="measure minimal embedder artifact, cold-start, and peak-RSS footprints"
    )
    parser.add_argument(
        "--profile",
        choices=("release", "release-size"),
        default="release-size",
        help="Cargo profile to build (default: release-size)",
    )
    parser.add_argument(
        "--target",
        help="optional Cargo target triple; the default is the host target",
    )
    parser.add_argument(
        "--runner",
        help="shlex-split command prefix used to execute a cross-target binary",
    )
    parser.add_argument(
        "--configs",
        default=",".join(CONFIGS),
        help="comma-separated configurations (default: all documented rows)",
    )
    parser.add_argument(
        "--size-tool",
        help="path or command used for Mach-O/ELF section-size inspection",
    )
    parser.add_argument(
        "--sizes-only",
        action="store_true",
        help="skip execution, cold-start, and RSS for a non-runnable target",
    )
    return parser.parse_args(argv)


def config_result(
    name: str,
    profile: str,
    target: Optional[str],
    runner: Sequence[str],
    size_tool: Sequence[str],
    strip_tool: Sequence[str],
    sizes_only: bool,
) -> Dict[str, object]:
    config = CONFIGS[name]
    build = cargo_command(config, profile, target)
    run_checked(build)

    binary = output_path(config, profile, target)
    execution = list(runner) + [str(binary), str(config["fixture"])]
    artifact = artifact_measurement(binary, profile, size_tool, strip_tool)
    result = {
        "config": name,
        "bin": config["bin"],
        "features": config["features"],
        "fixture": config["fixture"],
        "expected_stdout": config["expected_stdout"],
        "build_command": command_text(build),
        "run_command": command_text(execution),
        "artifact": artifact,
    }

    if sizes_only:
        result["execution"] = {
            "status": "skipped",
            "reason": "--sizes-only",
        }
        result["measurements"] = None
        return result

    run(execution, str(config["expected_stdout"]))
    result["execution"] = {"status": "verified"}
    result["measurements"] = runtime_measurement(
        execution, str(config["expected_stdout"])
    )
    return result


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    try:
        selected = parse_configs(args.configs)
        runner = shlex.split(args.runner) if args.runner else []
        if args.runner and not runner:
            raise FootprintError("--runner must not be empty")
        size_tool = resolve_size_tool(args.size_tool)
        strip_tool = resolve_strip_tool() if args.profile == "release" else []
        results = [
            config_result(
                name,
                args.profile,
                args.target,
                runner,
                size_tool,
                strip_tool,
                args.sizes_only,
            )
            for name in selected
        ]
    except subprocess.CalledProcessError as error:
        print(
            f"`{command_text(error.cmd)}` exited with status {error.returncode}",
            file=sys.stderr,
        )
        if error.stderr:
            print(error.stderr.rstrip("\n"), file=sys.stderr)
        return 1
    except (FootprintError, MeasurementError) as error:
        print(error, file=sys.stderr)
        return 1

    json.dump(
        {
            "platform": platform.platform(),
            "machine": platform.machine(),
            "target": args.target or "host",
            "profile": args.profile,
            "runner": runner,
            "sizes_only": args.sizes_only,
            "size_tool": command_text(size_tool),
            "configs": results,
        },
        sys.stdout,
        indent=2,
    )
    print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
