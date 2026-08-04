#!/usr/bin/env python3
"""Measure logical debug-name retention for issue #208.

The probe itself lives with the private parser representation so it can compare
the compact representation against the exact `Vec` capacities created while
parsing the same name section.  This driver only creates inputs and renders
the probe's prefixed JSON records.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any


PREFIX = "DEBUG_NAME_RETENTION_JSON "
ROOT = pathlib.Path(__file__).resolve().parents[1]
FIXTURES = (
    ("fixture-benchmark", ROOT / "crates/telomere/benches/telomere-benchmark.wasm"),
    ("fixture-add", ROOT / "examples/add.wasm"),
    ("fixture-wasi-preview1-hello", ROOT / "examples/wasi-preview1-hello.wasm"),
)
SYNTHETIC_FUNCTION_COUNTS = (10, 100, 1_000, 5_000)
PROBE_TEST = "common::debug_names::tests::measurement_probe"
ACCOUNTING_FIELDS = (
    "pointer_width_bits",
    "module_names_size_bytes",
    "option_arc_slot_bytes",
    "arc_header_assumption_bytes",
)
RESULT_FIELDS = (
    "module_bytes",
    "name_section_payload_bytes",
    "compact_retained_payload_bytes",
    "compact_retained_total_logical_bytes",
    "compact_live_allocations",
    "vec_as_is_logical_bytes",
    "vec_live_allocations",
)


def run(command: list[str], *, cwd: pathlib.Path = ROOT, env: dict[str, str] | None = None) -> str:
    """Run one documented command and return stdout, retaining stderr on failure."""
    completed = subprocess.run(
        command,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if completed.returncode:
        rendered = " ".join(command)
        raise RuntimeError(
            f"command failed ({completed.returncode}): {rendered}\n{completed.stderr.strip()}"
        )
    return completed.stdout


@contextmanager
def temporary_inputs(keep_temp: bool) -> Iterator[pathlib.Path]:
    """Create one disposable input tree, retaining it only on explicit request."""
    root = pathlib.Path(tempfile.mkdtemp(prefix="telomere-debug-name-retention-"))
    try:
        yield root
    finally:
        if keep_temp:
            print(f"temporary inputs retained at: {root}")
        else:
            shutil.rmtree(root, ignore_errors=True)


def function_name(index: int) -> str:
    """Return a deterministic, valid WAT identifier with a 60-character name."""
    name = f"function_{index:05d}_probe_name_padding_abcdefghijklmnopqrstuvwxyz"
    assert 40 <= len(name) <= 60
    return name


def write_synthetic_wat(path: pathlib.Path, function_count: int) -> None:
    lines = [f"(module $synthetic_f_{function_count}"]
    lines.extend(f"  (func ${function_name(index)})" for index in range(function_count))
    lines.append(")")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_rust_hello(project: pathlib.Path) -> pathlib.Path:
    """Build a debug Rust 2021 wasm module in the temporary input tree."""
    source = project / "src"
    source.mkdir(parents=True)
    (project / "Cargo.toml").write_text(
        """[package]
name = "debug-name-retention-hello"
version = "0.0.0"
edition = "2021"
publish = false

[profile.dev]
debug = 2
""",
        encoding="utf-8",
    )
    (source / "main.rs").write_text(
        """#[inline(never)]
fn greeting() -> &'static str {
    "hello from the debug-name retention probe"
}

fn main() {
    let _ = greeting();
}
""",
        encoding="utf-8",
    )
    return project / "target/wasm32-unknown-unknown/debug/debug-name-retention-hello.wasm"


def make_manifest(inputs: list[tuple[str, pathlib.Path]], path: pathlib.Path) -> None:
    rows = []
    for label, input_path in inputs:
        rendered_path = str(input_path.resolve())
        if any(character in label or character in rendered_path for character in "\t\r\n"):
            raise ValueError("TSV manifest labels and paths may not contain tabs or line breaks")
        rows.append(f"{label}\t{rendered_path}")
    path.write_text("\n".join(rows) + "\n", encoding="utf-8")


def extract_records(output: str, expected_labels: list[str]) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for line in output.splitlines():
        _, marker, payload = line.partition(PREFIX)
        if not marker:
            continue
        try:
            record = json.loads(payload.strip())
        except json.JSONDecodeError as error:
            raise ValueError(f"invalid probe JSON: {payload!r}: {error}") from error
        if not isinstance(record, dict):
            raise ValueError("probe JSON record must be an object")
        records.append(record)

    labels = [record.get("label") for record in records]
    if labels != expected_labels:
        raise ValueError(
            f"probe labels differ from manifest: expected {expected_labels!r}, got {labels!r}"
        )
    for record in records:
        for field in (*RESULT_FIELDS, *ACCOUNTING_FIELDS):
            value = record.get(field)
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                raise ValueError(f"probe record {record['label']!r} has invalid {field!r}: {value!r}")
    return records


def accounting_constants(records: list[dict[str, Any]]) -> dict[str, int]:
    constants = {field: records[0][field] for field in ACCOUNTING_FIELDS}
    for record in records[1:]:
        for field, value in constants.items():
            if record[field] != value:
                raise ValueError(
                    f"probe accounting constant {field!r} changed between inputs: "
                    f"{value} and {record[field]}"
                )
    return constants


def command_version(command: list[str]) -> str:
    return run(command).strip()


def print_report(records: list[dict[str, Any]]) -> None:
    constants = accounting_constants(records)
    print("# Debug-name retention logical-accounting probe")
    print()
    print("This is logical accounting from the parser representation; it excludes allocator bucket rounding.")
    print()
    print("## Toolchain")
    print()
    print(f"- Python: `{sys.version.split()[0]}`")
    print(f"- cargo: `{command_version(['cargo', '--version'])}`")
    print(f"- rustc: `{command_version(['rustc', '--version'])}`")
    print(f"- wasm-tools: `{command_version(['wasm-tools', '--version'])}`")
    print()
    print("## Commands")
    print()
    print("- `wasm-tools parse -o <synthetic>.wasm <synthetic>.wat` (four synthetic modules)")
    print(
        "- `cargo build --manifest-path <temporary>/rust-hello/Cargo.toml "
        "--target wasm32-unknown-unknown --target-dir <temporary>/rust-hello/target`"
    )
    print(
        "- `cargo test -p telomere --lib "
        "common::debug_names::tests::measurement_probe -- --ignored --exact --nocapture`"
    )
    print()
    print("## Logical-accounting constants")
    print()
    print(f"- Pointer width: `{constants['pointer_width_bits']}` bits")
    print(f"- `size_of::<ModuleNames>()`: `{constants['module_names_size_bytes']}` bytes")
    print(f"- `size_of::<Option<Arc<ModuleNames>>>()`: `{constants['option_arc_slot_bytes']}` bytes")
    print(
        "- `Arc` control-block assumption (two `usize` counters): "
        f"`{constants['arc_header_assumption_bytes']}` bytes"
    )
    print()
    print("## Results")
    print()
    print(
        "| input | module bytes | name payload bytes | compact payload bytes | "
        "compact total logical bytes | compact allocations | Vec-as-is logical bytes | Vec allocations |"
    )
    print("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |")
    for record in records:
        print(
            "| {label} | {module_bytes} | {name_section_payload_bytes} | "
            "{compact_retained_payload_bytes} | {compact_retained_total_logical_bytes} | "
            "{compact_live_allocations} | {vec_as_is_logical_bytes} | "
            "{vec_live_allocations} |".format(**record)
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--keep-temp", action="store_true", help="print and retain generated inputs")
    args = parser.parse_args()

    # The detailed fixture construction and table rendering intentionally stay
    # in this small, dependency-free script; no generated artifacts are kept in
    # the repository.
    try:
        return run_measurement(args.keep_temp)
    except (OSError, RuntimeError, ValueError) as error:
        print(f"failed-with-reason: {error}", file=sys.stderr)
        return 1


def run_measurement(keep_temp: bool) -> int:
    for label, fixture in FIXTURES:
        if not fixture.is_file():
            raise RuntimeError(f"required {label} fixture is missing: {fixture}")

    with temporary_inputs(keep_temp) as temporary:
        inputs = [(label, fixture) for label, fixture in FIXTURES]
        for function_count in SYNTHETIC_FUNCTION_COUNTS:
            wat = temporary / f"synthetic-f{function_count}.wat"
            wasm = wat.with_suffix(".wasm")
            write_synthetic_wat(wat, function_count)
            run(["wasm-tools", "parse", "-o", str(wasm), str(wat)])
            inputs.append((f"synthetic-f{function_count}", wasm))

        rust_project = temporary / "rust-hello"
        rust_wasm = write_rust_hello(rust_project)
        run(
            [
                "cargo",
                "build",
                "--manifest-path",
                str(rust_project / "Cargo.toml"),
                "--target",
                "wasm32-unknown-unknown",
                "--target-dir",
                str(rust_project / "target"),
            ]
        )
        if not rust_wasm.is_file():
            raise RuntimeError(f"Rust build reported success but did not create {rust_wasm}")
        inputs.append(("rust-2021-debug-hello", rust_wasm))

        manifest = temporary / "probe-manifest.tsv"
        make_manifest(inputs, manifest)
        environment = os.environ.copy()
        environment["TELOMERE_DEBUG_NAMES_PROBE_MANIFEST"] = str(manifest)
        output = run(
            [
                "cargo",
                "test",
                "-p",
                "telomere",
                "--lib",
                PROBE_TEST,
                "--",
                "--ignored",
                "--exact",
                "--nocapture",
            ],
            env=environment,
        )
        records = extract_records(output, [label for label, _ in inputs])
        print_report(records)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
