#!/usr/bin/env python3
"""Build, prepare, and measure the interpreter baseline matrix."""

import argparse
import itertools
import json
import math
import os
import random
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Dict, List, Mapping, Optional, Sequence, Tuple

from _measure_support import (
    MeasurementError,
    below_noise_floor,
    coremark_score,
    machine_facts,
    noise_floor,
    paired_contrasts,
    paired_slopes,
    sha256_file,
    williams_schedule,
)


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "tools" / "baseline" / "artifacts.json"
DEFAULT_TARGET_DIR = REPO_ROOT / "target" / "baseline"
BUILD_MANIFEST_NAME = "build-manifest.json"

BUILD_CONFIGS: Mapping[str, Mapping[str, object]] = {
    "default": {"features": []},
    "jit": {"features": ["jit"]},
    "opt": {"features": ["measure-switches"]},
    "jit-opt": {"features": ["jit", "measure-switches"]},
}

# The two default labels are separate A/A schedule arms.  They share a copied
# default binary, whereas every remaining entry is a documented build/run cell.
ARM_CONFIGS: Mapping[str, Mapping[str, Optional[str]]] = {
    "default-a": {"cell": "default", "build": "default", "optimizer": None},
    "default-b": {"cell": "default", "build": "default", "optimizer": None},
    "jit": {"cell": "jit", "build": "jit", "optimizer": None},
    "opt-on": {"cell": "opt-on", "build": "opt", "optimizer": None},
    "opt-off": {"cell": "opt-off", "build": "opt", "optimizer": "off"},
    "jit-opt-on": {"cell": "jit,opt-on", "build": "jit-opt", "optimizer": None},
    "jit-opt-off": {"cell": "jit,opt-off", "build": "jit-opt", "optimizer": "off"},
}

COMPARISONS: Tuple[Tuple[str, str, str], ...] = (
    ("measure_switches_control", "default-a", "opt-on"),
    ("measure_switches_control_with_jit", "jit", "jit-opt-on"),
    ("jit_feature_interpreter_tax", "opt-on", "jit-opt-on"),
    ("optimizer_pipeline_upper_bound", "opt-on", "opt-off"),
    (
        "optimizer_pipeline_upper_bound_with_jit",
        "jit-opt-on",
        "jit-opt-off",
    ),
)

SCALE_MULTIPLIERS: Tuple[int, int, int] = (1, 2, 3)
SCALE_PERMUTATIONS: Tuple[Tuple[int, int, int], ...] = tuple(
    tuple(permutation) for permutation in itertools.permutations(SCALE_MULTIPLIERS)
)
# Each residual set places every scale at every within-block position once. It
# turns the 12-round, two-of-each-permutation prefix into 5/5/5 scale positions
# for every arm in a 15-round normal schedule without pretending that all six
# permutations occur equally often across fifteen rounds.
SCALE_RESIDUAL_CYCLES: Tuple[Tuple[Tuple[int, int, int], ...], ...] = (
    ((1, 2, 3), (2, 3, 1), (3, 1, 2)),
    ((1, 3, 2), (3, 2, 1), (2, 1, 3)),
)


class BaselineError(RuntimeError):
    """A build, artifact, witness, or measurement contract failed."""

    def __init__(self, reason: str, detail: str, **context: object) -> None:
        super().__init__(detail)
        self.reason = reason
        self.detail = detail
        self.context = context


class JsonArgumentParser(argparse.ArgumentParser):
    """Keep nonzero exits in the JSON evidence channel."""

    def error(self, message: str) -> None:
        raise BaselineError("invalid_arguments", message)


def command_text(command: Sequence[str]) -> str:
    """Render an argv vector in the project measurement-tool style."""

    return shlex.join(list(command))


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    """Parse command-line options without argparse bypassing JSON output."""

    parser = JsonArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode",
        choices=("first-record", "compare"),
        help="record a first baseline or compare against --baseline",
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        help="prior raw record; required with --mode compare",
    )
    parser.add_argument(
        "--quick",
        action="store_true",
        help="run only a non-quotable short schedule",
    )
    parser.add_argument(
        "--rounds",
        type=int,
        help="override rounds per workload; fewer than ten remain non-quotable",
    )
    parser.add_argument(
        "--warmup",
        type=int,
        help="warm-up rounds per physical arm",
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=184,
        help="counterbalancing and bootstrap seed",
    )
    parser.add_argument(
        "--max-start-load",
        type=float,
        help="maximum one-minute load average before timing begins",
    )
    parser.add_argument(
        "--max-load-rise",
        type=float,
        default=0.5,
        help="maximum one-minute load-average rise during timing",
    )
    parser.add_argument(
        "--build-only",
        action="store_true",
        help="build, copy, and hash all four binaries without fetching or timing",
    )
    parser.add_argument(
        "--skip-build",
        action="store_true",
        help="reuse target/baseline/bin copies produced by --build-only",
    )
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=DEFAULT_TARGET_DIR,
        help="shared Cargo and copied-artifact directory",
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=DEFAULT_MANIFEST,
        help="pinned workload manifest",
    )
    parser.add_argument(
        "--wasm-tools",
        default="wasm-tools",
        help="wasm-tools executable used for checked-in WAT workloads",
    )
    parser.add_argument(
        "--out",
        type=Path,
        help="also atomically write stdout JSON to this path",
    )
    args = parser.parse_args(argv)
    if args.mode is None and not args.build_only:
        raise BaselineError(
            "missing_mode",
            "--mode is required unless --build-only was requested",
        )
    if args.mode == "compare" and args.baseline is None:
        raise BaselineError(
            "missing_baseline",
            "--mode compare requires --baseline <prior-record.json>",
        )
    if args.mode == "first-record" and args.baseline is not None:
        raise BaselineError(
            "unexpected_baseline",
            "--baseline is only valid with --mode compare",
        )
    if args.build_only and args.skip_build:
        raise BaselineError(
            "invalid_build_mode",
            "--build-only and --skip-build cannot be used together",
        )
    if args.build_only and args.baseline is not None:
        raise BaselineError(
            "unexpected_baseline",
            "--baseline is not valid with --build-only",
        )
    if args.rounds is not None and args.rounds <= 0:
        raise BaselineError("invalid_rounds", "--rounds must be positive")
    if args.rounds is not None and args.rounds < 10 and not args.quick:
        raise BaselineError(
            "non_publishable_rounds_require_quick",
            "--rounds below 10 requires --quick and cannot create a baseline",
        )
    if args.warmup is not None and args.warmup < 0:
        raise BaselineError("invalid_warmup", "--warmup must not be negative")
    if args.max_start_load is not None and args.max_start_load < 0:
        raise BaselineError(
            "invalid_load_threshold", "--max-start-load must be non-negative"
        )
    if args.max_load_rise < 0:
        raise BaselineError(
            "invalid_load_threshold", "--max-load-rise must be non-negative"
        )
    return args


def checked_output(
    command: Sequence[str], env: Optional[Mapping[str, str]] = None
) -> str:
    """Run a command and keep stdout for workload self-validation."""

    try:
        completed = subprocess.run(
            list(command),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=None if env is None else dict(env),
        )
    except OSError as error:
        raise BaselineError(
            "command_unavailable",
            f"could not start {command_text(command)}: {error}",
            command=list(command),
        ) from error
    if completed.returncode:
        detail = completed.stderr.rstrip("\n")
        message = f"{command_text(command)} exited with status {completed.returncode}"
        if detail:
            message = f"{message}\n{detail}"
        raise BaselineError(
            "command_failed",
            message,
            command=list(command),
            returncode=completed.returncode,
            stderr=completed.stderr,
        )
    return completed.stdout


def cargo_build_command(features: Sequence[str], target_dir: Path) -> List[str]:
    """Return one isolated Cargo command for a matrix build."""

    command = [
        "cargo",
        "build",
        "--locked",
        "--release",
        "-p",
        "telomere-cli",
        "--target-dir",
        str(target_dir),
    ]
    if features:
        command.extend(["--features", ",".join(features)])
    return command


def witness_a_command(features: Sequence[str], target_dir: Path) -> List[str]:
    """Return the release call-threading test command for one feature set."""

    command = [
        "cargo",
        "test",
        "--locked",
        "--release",
        "-p",
        "telomere",
        "--target-dir",
        str(target_dir),
    ]
    if features:
        command.extend(["--features", ",".join(features)])
    command.extend(
        [
            "--test",
            "call_threading",
            "release_call_loop_keeps_direct_threading",
        ]
    )
    return command


def atomic_write_json(path: Path, payload: Mapping[str, object]) -> None:
    """Write a small provenance record without exposing a partial manifest."""

    path.parent.mkdir(parents=True, exist_ok=True)
    rendered = json.dumps(payload, indent=2, sort_keys=True)
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        delete=False,
        dir=path.parent,
    ) as temporary:
        temporary.write(rendered)
        temporary.write("\n")
        temporary_path = Path(temporary.name)
    temporary_path.replace(path)


def build_manifest_path(target_dir: Path) -> Path:
    """Return the explicit provenance file required by a skip-build run."""

    return target_dir / BUILD_MANIFEST_NAME


def current_source_commit() -> str:
    """Return the exact source revision a copied binary is allowed to represent."""

    commit = checked_output(["git", "rev-parse", "HEAD"]).strip()
    if not commit:
        raise BaselineError(
            "source_commit_unavailable",
            "git rev-parse HEAD returned no source commit",
        )
    return commit


def require_clean_tracked_worktree() -> None:
    """Refuse to create reusable build provenance from a dirty tracked tree."""

    for command in (
        ["git", "diff", "--quiet", "HEAD"],
        ["git", "diff", "--cached", "--quiet"],
    ):
        try:
            completed = subprocess.run(
                command,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
                text=True,
            )
        except OSError as error:
            raise BaselineError(
                "git_status_unavailable",
                f"could not run {command_text(command)}: {error}",
            ) from error
        if completed.returncode == 1:
            raise BaselineError(
                "dirty_tracked_worktree",
                "build provenance requires a clean tracked worktree",
                command=command,
            )
        if completed.returncode != 0:
            raise BaselineError(
                "git_status_unavailable",
                f"{command_text(command)} exited with {completed.returncode}",
                command=command,
                stderr=completed.stderr,
            )


def invalidate_stale_build_manifest(target_dir: Path) -> None:
    """Remove old provenance before a new build can leave partial state behind."""

    path = build_manifest_path(target_dir)
    if path.exists():
        try:
            path.unlink()
        except OSError as error:
            raise BaselineError(
                "build_manifest_invalidation_failed",
                f"could not invalidate stale build provenance {path}: {error}",
            ) from error


def load_skip_build_manifest(target_dir: Path) -> Mapping[str, object]:
    """Load a prior complete build-only record or fail before any timing starts."""

    path = build_manifest_path(target_dir)
    try:
        with path.open(encoding="utf-8") as source:
            manifest = json.load(source)
    except (OSError, json.JSONDecodeError) as error:
        raise BaselineError(
            "missing_build_manifest",
            f"could not read build provenance {path}: {error}",
        ) from error
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise BaselineError(
            "invalid_build_manifest",
            f"{path} must be a schema_version 1 object",
        )
    return manifest


def verified_skip_build_records(target_dir: Path) -> Dict[str, Dict[str, object]]:
    """Verify binary hashes, feature identities, and Witness A before reuse."""

    manifest = load_skip_build_manifest(target_dir)
    current_commit = current_source_commit()
    if manifest.get("source_commit") != current_commit:
        raise BaselineError(
            "build_manifest_source_commit_mismatch",
            "copied binary provenance is not for the current source commit",
            expected_source_commit=current_commit,
            manifest_source_commit=manifest.get("source_commit"),
        )
    builds = manifest.get("builds")
    if not isinstance(builds, dict) or set(builds) != set(BUILD_CONFIGS):
        raise BaselineError(
            "invalid_build_manifest",
            "build provenance must contain exactly the four baseline builds",
        )
    records: Dict[str, Dict[str, object]] = {}
    for name, config in BUILD_CONFIGS.items():
        entry = builds.get(name)
        features = list(config["features"])
        copied_binary = target_dir / "bin" / name
        if not isinstance(entry, dict):
            raise BaselineError(
                "invalid_build_manifest", f"build provenance for {name} is invalid"
            )
        if entry.get("features") != features:
            raise BaselineError(
                "build_manifest_feature_mismatch",
                f"build provenance features do not match for {name}",
                build=name,
            )
        if entry.get("copied_binary") != str(copied_binary):
            raise BaselineError(
                "build_manifest_binary_path_mismatch",
                f"build provenance path does not match for {name}",
                build=name,
            )
        if not copied_binary.is_file():
            raise BaselineError(
                "missing_skipped_build_binary",
                f"missing copied binary for build {name}: {copied_binary}",
                build=name,
            )
        actual_sha256 = sha256_file(copied_binary)
        if entry.get("sha256") != actual_sha256:
            raise BaselineError(
                "build_manifest_hash_mismatch",
                f"copied binary digest does not match provenance for {name}",
                build=name,
                expected_sha256=entry.get("sha256"),
                actual_sha256=actual_sha256,
            )
        witness_a = entry.get("witness_a")
        if (
            not isinstance(witness_a, dict)
            or witness_a.get("status") != "pass"
            or witness_a.get("features") != features
            or witness_a.get("command")
            != witness_a_command(features, target_dir / "cargo")
        ):
            raise BaselineError(
                "build_manifest_witness_a_missing",
                f"build provenance lacks a passing Witness A for {name}",
                build=name,
            )
        record = dict(entry)
        record["build_action"] = "reused_after_manifest_verification"
        records[name] = record
    return records


def build_binaries(args: argparse.Namespace) -> Dict[str, Dict[str, object]]:
    """Build, copy, hash, and Witness-A check the four binary configurations."""

    require_clean_tracked_worktree()
    if args.skip_build:
        return verified_skip_build_records(args.target_dir)
    source_commit = current_source_commit()
    invalidate_stale_build_manifest(args.target_dir)
    cargo_target = args.target_dir / "cargo"
    copied_dir = args.target_dir / "bin"
    copied_dir.mkdir(parents=True, exist_ok=True)
    source_binary = cargo_target / "release" / "telomere-cli"
    records: Dict[str, Dict[str, object]] = {}
    for name, config in BUILD_CONFIGS.items():
        features = list(config["features"])
        copied_binary = copied_dir / name
        command = cargo_build_command(features, cargo_target)
        record: Dict[str, object] = {
            "build": name,
            "features": features,
            "cargo_command": command,
            "copied_binary": str(copied_binary),
        }
        checked_output(command)
        if not source_binary.is_file():
            raise BaselineError(
                "missing_build_binary",
                f"successful build did not create {source_binary}",
                build=name,
            )
        shutil.copy2(source_binary, copied_binary)
        record["build_action"] = "built_and_copied"
        if not copied_binary.is_file():
            raise BaselineError(
                "missing_copied_binary",
                f"missing copied binary for build {name}: {copied_binary}",
                build=name,
            )
        record["sha256"] = sha256_file(copied_binary)
        witness_command = witness_a_command(features, cargo_target)
        checked_output(witness_command)
        record["witness_a"] = {
            "status": "pass",
            "features": features,
            "command": witness_command,
            "test": "release_call_loop_keeps_direct_threading",
        }
        records[name] = record
    atomic_write_json(
        build_manifest_path(args.target_dir),
        {
            "schema_version": 1,
            "source_commit": source_commit,
            "builds": records,
        },
    )
    return records


def load_manifest(path: Path) -> List[Dict[str, object]]:
    """Read the versioned pinned workload manifest."""

    try:
        with path.open(encoding="utf-8") as source:
            manifest = json.load(source)
    except (OSError, json.JSONDecodeError) as error:
        raise BaselineError(
            "invalid_artifact_manifest",
            f"could not read {path}: {error}",
        ) from error
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise BaselineError(
            "invalid_artifact_manifest",
            f"{path} must be a schema_version 1 object",
        )
    workloads = manifest.get("workloads")
    if not isinstance(workloads, list) or not workloads:
        raise BaselineError(
            "invalid_artifact_manifest",
            f"{path} must contain a non-empty workloads array",
        )
    names = set()
    normalized = []
    for workload in workloads:
        if not isinstance(workload, dict):
            raise BaselineError(
                "invalid_artifact_manifest", "every workload must be an object"
            )
        name = workload.get("name")
        filename = workload.get("filename")
        if (
            not isinstance(name, str)
            or not name
            or not isinstance(filename, str)
            or Path(filename).name != filename
            or name in names
        ):
            raise BaselineError(
                "invalid_artifact_manifest",
                "workloads need unique names and simple output filenames",
            )
        names.add(name)
        normalized.append(dict(workload))
    return normalized


def verified_existing_file(path: Path, expected_sha256: str) -> bool:
    """Return whether an existing cache entry still matches its pinned digest."""

    return path.is_file() and sha256_file(path) == expected_sha256


def fetch_verified_artifact(
    url: str, expected_sha256: str, destination: Path
) -> Dict[str, object]:
    """Fetch a remote workload once and fail closed on any digest mismatch."""

    if destination.exists() and not verified_existing_file(destination, expected_sha256):
        raise BaselineError(
            "artifact_hash_mismatch",
            f"existing artifact {destination} does not match the manifest digest",
            path=str(destination),
            expected_sha256=expected_sha256,
            actual_sha256=sha256_file(destination),
        )
    if not destination.exists():
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary_name: Optional[str] = None
        try:
            with urllib.request.urlopen(url) as response:
                with tempfile.NamedTemporaryFile(
                    mode="wb", delete=False, dir=destination.parent
                ) as temporary:
                    temporary_name = temporary.name
                    shutil.copyfileobj(response, temporary)
            temporary_path = Path(temporary_name)
            actual_sha256 = sha256_file(temporary_path)
            if actual_sha256 != expected_sha256:
                raise BaselineError(
                    "artifact_hash_mismatch",
                    f"downloaded artifact from {url} has the wrong digest",
                    url=url,
                    expected_sha256=expected_sha256,
                    actual_sha256=actual_sha256,
                )
            temporary_path.replace(destination)
        except (OSError, urllib.error.URLError) as error:
            raise BaselineError(
                "artifact_fetch_failed",
                f"could not fetch {url}: {error}",
                url=url,
            ) from error
        finally:
            if temporary_name:
                temporary_path = Path(temporary_name)
                if temporary_path.exists():
                    temporary_path.unlink()

    return {
        "path": str(destination),
        "sha256": sha256_file(destination),
        "source": "remote",
        "url": url,
    }


def compile_wat_workload(
    workload: Mapping[str, object], target_dir: Path, wasm_tools: str
) -> Dict[str, object]:
    """Verify a local WAT source, compile it, and record both source and output."""

    source_value = workload.get("source")
    expected_source_sha256 = workload.get("source_sha256")
    filename = workload.get("filename")
    if not isinstance(source_value, str) or not isinstance(expected_source_sha256, str):
        raise BaselineError(
            "invalid_artifact_manifest",
            f"WAT workload {workload.get('name')} needs source and source_sha256",
        )
    if not isinstance(filename, str):
        raise BaselineError(
            "invalid_artifact_manifest",
            f"WAT workload {workload.get('name')} needs a filename",
        )
    source = REPO_ROOT / source_value
    if not source.is_file():
        raise BaselineError(
            "missing_workload_source",
            f"WAT workload source does not exist: {source}",
        )
    actual_source_sha256 = sha256_file(source)
    if actual_source_sha256 != expected_source_sha256:
        raise BaselineError(
            "workload_source_hash_mismatch",
            f"WAT workload {source} does not match its manifest digest",
            expected_sha256=expected_source_sha256,
            actual_sha256=actual_source_sha256,
        )

    output = target_dir / "workloads" / filename
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_suffix(output.suffix + ".tmp")
    if temporary.exists():
        temporary.unlink()
    try:
        checked_output([wasm_tools, "parse", str(source), "-o", str(temporary)])
        temporary.replace(output)
    except BaselineError:
        if temporary.exists():
            temporary.unlink()
        raise
    return {
        "path": str(output),
        "sha256": sha256_file(output),
        "source": "wat",
        "wat_source": str(source),
        "wat_source_sha256": actual_source_sha256,
        "compiler_command": [
            wasm_tools,
            "parse",
            str(source),
            "-o",
            str(output),
        ],
    }


def materialize_workloads(
    workloads: Sequence[Mapping[str, object]], args: argparse.Namespace
) -> List[Dict[str, object]]:
    """Fetch or compile workloads independently, retaining each failure record."""

    materialized = []
    artifact_dir = args.target_dir / "artifacts"
    for workload in workloads:
        record = dict(workload)
        try:
            kind = workload.get("kind")
            name = workload.get("name")
            filename = workload.get("filename")
            if not isinstance(name, str) or not isinstance(filename, str):
                raise BaselineError(
                    "invalid_artifact_manifest",
                    "workload name and filename are required",
                )
            if kind == "remote_wasm":
                url = workload.get("url")
                expected_sha256 = workload.get("sha256")
                if not isinstance(url, str) or not isinstance(expected_sha256, str):
                    raise BaselineError(
                        "invalid_artifact_manifest",
                        f"remote workload {name} needs url and sha256",
                    )
                identity = fetch_verified_artifact(
                    url, expected_sha256, artifact_dir / filename
                )
            elif kind == "wat":
                identity = compile_wat_workload(
                    workload, args.target_dir, args.wasm_tools
                )
            else:
                raise BaselineError(
                    "invalid_artifact_manifest",
                    f"workload {name} has unsupported kind {kind}",
                )
        except BaselineError as error:
            record.update(
                {
                    "artifact": None,
                    "artifact_status": "invalid",
                    "invalid_reason": error.reason,
                    "error": error.detail,
                    "error_context": error.context,
                }
            )
        except OSError as error:
            record.update(
                {
                    "artifact": None,
                    "artifact_status": "invalid",
                    "invalid_reason": "artifact_materialization_failed",
                    "error": str(error),
                    "error_context": {},
                }
            )
        else:
            record.update(
                {
                    "artifact": identity,
                    "artifact_status": "ready",
                    "invalid_reason": None,
                }
            )
        materialized.append(record)
    return materialized


def measurement_env(requested_optimizer: Optional[str]) -> Dict[str, str]:
    """Build an environment where the requested switch state is unambiguous."""

    environment = dict(os.environ)
    environment.pop("TELOMERE_OPTIMIZER", None)
    if requested_optimizer is not None:
        environment["TELOMERE_OPTIMIZER"] = requested_optimizer
    return environment


def observe_switch_state(
    binary: Path, requested_optimizer: Optional[str], feature_enabled: bool
) -> Dict[str, object]:
    """Query the feature-gated probe rather than trusting a requested env value."""

    if not feature_enabled:
        return {
            "requested_optimizer": None,
            "observed_optimizer": None,
            "probe": "not_applicable",
        }
    expected_state = "off" if requested_optimizer == "off" else "on"
    stdout = checked_output(
        [str(binary), "measure-switches-probe"],
        measurement_env(requested_optimizer),
    )
    expected_stdout = f'{{"state":"{expected_state}"}}\n'
    if stdout != expected_stdout:
        raise BaselineError(
            "switch_probe_mismatch",
            f"switch probe printed {stdout!r}; expected {expected_stdout!r}",
            binary=str(binary),
            requested_optimizer=requested_optimizer,
        )
    try:
        parsed = json.loads(stdout)
    except json.JSONDecodeError as error:
        raise BaselineError(
            "switch_probe_invalid_json",
            f"switch probe emitted invalid JSON: {error}",
            binary=str(binary),
        ) from error
    if parsed != {"state": expected_state}:
        raise BaselineError(
            "switch_probe_mismatch",
            f"switch probe resolved {parsed!r}; expected {expected_state!r}",
            binary=str(binary),
        )
    return {
        "requested_optimizer": requested_optimizer,
        "observed_optimizer": expected_state,
        "probe": "measure-switches-probe",
        "probe_stdout": stdout,
    }


def observe_switches(
    builds: Mapping[str, Mapping[str, object]]
) -> Dict[str, Dict[str, object]]:
    """Observe every logical cell once before timed commands start."""

    records = {}
    for arm, spec in ARM_CONFIGS.items():
        build_name = spec["build"]
        assert isinstance(build_name, str)
        binary = Path(str(builds[build_name]["copied_binary"]))
        record = observe_switch_state(
            binary,
            spec["optimizer"],
            build_name in ("opt", "jit-opt"),
        )
        record["cell"] = spec["cell"]
        record["build"] = build_name
        records[arm] = record
    return records


def load_prior_raw_record(path: Path) -> Mapping[str, object]:
    """Load a completed, publishable raw record before allowing comparison."""

    try:
        with path.open(encoding="utf-8") as source:
            prior = json.load(source)
    except (OSError, json.JSONDecodeError) as error:
        raise BaselineError(
            "baseline_record_unreadable",
            f"could not read comparison baseline {path}: {error}",
        ) from error
    if not isinstance(prior, dict):
        raise BaselineError(
            "baseline_record_invalid", "comparison baseline must be a JSON object"
        )
    if prior.get("status") != "ok" or prior.get("publishable") is not True:
        raise BaselineError(
            "baseline_not_publishable",
            "comparison baseline must have status=ok and publishable=true",
            baseline_status=prior.get("status"),
            baseline_publishable=prior.get("publishable"),
        )
    return prior


def load_prior_build_witnesses(
    path: Path, builds: Mapping[str, Mapping[str, object]]
) -> Mapping[str, Mapping[str, object]]:
    """Load and validate the per-build witness records from a prior raw record."""

    prior = load_prior_raw_record(path)
    witnesses = prior.get("tail_call_witnesses")
    matrix = prior.get("matrix")
    prior_builds = matrix.get("builds") if isinstance(matrix, dict) else None
    if not isinstance(witnesses, dict) or not isinstance(prior_builds, dict):
        raise BaselineError(
            "baseline_witnesses_missing",
            "comparison baseline lacks per-build tail_call_witnesses and matrix builds",
        )
    selected: Dict[str, Mapping[str, object]] = {}
    for name, current_build in builds.items():
        witness = witnesses.get(name)
        prior_build = prior_builds.get(name)
        if (
            not isinstance(witness, dict)
            or not isinstance(witness.get("probes"), list)
            or not isinstance(witness.get("contract_passed"), bool)
            or not isinstance(prior_build, dict)
            or prior_build.get("features") != current_build.get("features")
        ):
            raise BaselineError(
                "baseline_witness_invalid",
                f"comparison baseline has no compatible witness for build {name}",
                build=name,
            )
        selected[name] = witness
    return selected


def artifact_identity_set(
    records: object, source: str
) -> set[Tuple[str, str, str]]:
    """Extract one complete name/layer/SHA-256 identity per workload.

    A comparison must not silently substitute a revised workload corpus.  This
    intentionally accepts no partial identity: absent or duplicate entries are
    an artifact mismatch instead of a best-effort comparison.
    """

    if not isinstance(records, list) or not records:
        raise BaselineError(
            "baseline_artifact_mismatch",
            f"{source} has no non-empty artifacts list",
        )
    identities: set[Tuple[str, str, str]] = set()
    for record in records:
        if not isinstance(record, dict):
            raise BaselineError(
                "baseline_artifact_mismatch",
                f"{source} contains a non-object artifact entry",
            )
        name = record.get("name")
        layer = record.get("layer")
        artifact = record.get("artifact")
        sha256 = artifact.get("sha256") if isinstance(artifact, dict) else None
        if (
            not isinstance(name, str)
            or not name
            or not isinstance(layer, str)
            or not layer
            or not isinstance(sha256, str)
            or len(sha256) != 64
        ):
            raise BaselineError(
                "baseline_artifact_mismatch",
                f"{source} has an incomplete artifact identity",
                artifact_name=name,
                artifact_layer=layer,
                artifact_sha256=sha256,
            )
        identity = (name, layer, sha256)
        if identity in identities:
            raise BaselineError(
                "baseline_artifact_mismatch",
                f"{source} repeats artifact identity {name}/{layer}",
                artifact_name=name,
                artifact_layer=layer,
                artifact_sha256=sha256,
            )
        identities.add(identity)
    return identities


def render_artifact_identities(
    identities: set[Tuple[str, str, str]]
) -> List[Dict[str, str]]:
    """Return sorted artifact identities in JSON-friendly form."""

    return [
        {"name": name, "layer": layer, "sha256": sha256}
        for name, layer, sha256 in sorted(identities)
    ]


def validate_compare_artifacts(
    path: Path, current_records: Sequence[Mapping[str, object]]
) -> Dict[str, object]:
    """Require exactly the same materialized workload identities as the prior run."""

    prior = load_prior_raw_record(path)
    prior_identities = artifact_identity_set(
        prior.get("artifacts"), "comparison baseline"
    )
    current_identities = artifact_identity_set(
        list(current_records), "current materialization"
    )
    if prior_identities != current_identities:
        raise BaselineError(
            "baseline_artifact_mismatch",
            "comparison baseline artifacts do not match the current materialization",
            prior_artifacts=render_artifact_identities(prior_identities),
            current_artifacts=render_artifact_identities(current_identities),
        )
    return {
        "status": "pass",
        "identities": render_artifact_identities(current_identities),
    }


def capture_witnesses(
    builds: Mapping[str, Mapping[str, object]],
    mode: str,
    baseline: Optional[Path],
) -> Tuple[Dict[str, object], List[str]]:
    """Capture the independent path-aware tail-call witness for every binary."""

    try:
        from baseline.tail_call_witness import (
            capture_tail_call_witness,
            witness_is_acceptable,
        )
    except ImportError as error:
        unavailable = {
            "status": "witness_unavailable",
            "contract_passed": False,
            "invalid_reason": "witness_helper_unavailable",
            "detail": str(error),
        }
        return {name: dict(unavailable) for name in builds}, [
            "witness_helper_unavailable"
        ]

    prior_witnesses: Optional[Mapping[str, Mapping[str, object]]] = None
    if mode == "compare":
        assert baseline is not None
        try:
            prior_witnesses = load_prior_build_witnesses(baseline, builds)
        except BaselineError as error:
            unavailable = {
                "status": "witness_unavailable",
                "contract_passed": False,
                "invalid_reason": error.reason,
                "detail": error.detail,
            }
            return {name: dict(unavailable) for name in builds}, [error.reason]

    records: Dict[str, object] = {}
    failures = []
    for name, build in builds.items():
        binary = Path(str(build["copied_binary"]))
        try:
            if prior_witnesses is None:
                record = capture_tail_call_witness(binary, mode=mode)
            else:
                record = capture_tail_call_witness(
                    binary,
                    mode=mode,
                    baseline_witness=prior_witnesses[name],
                )
            acceptable = witness_is_acceptable(record)
        except Exception as error:
            record = {
                "status": "witness_unavailable",
                "contract_passed": False,
                "invalid_reason": "witness_capture_failed",
                "detail": str(error),
            }
            acceptable = False
        records[name] = record
        if not acceptable:
            reason = record.get("invalid_reason") if isinstance(record, dict) else None
            failures.append(str(reason or "tail_call_witness_failed"))
    return records, failures


def effective_rounds(args: argparse.Namespace) -> int:
    """Use at least ten paired contrasts unless quick mode was explicit."""

    if args.rounds is not None:
        return args.rounds
    return 3 if args.quick else 15


def effective_warmup(args: argparse.Namespace) -> int:
    """Keep normal runs warm without making a diagnostic run unnecessarily slow."""

    if args.warmup is not None:
        return args.warmup
    return 0 if args.quick else 1


def default_start_load_threshold() -> float:
    """Scale the quiet-machine admission threshold with logical CPU count."""

    return max(1.0, float(os.cpu_count() or 1) * 0.10)


def workload_command(
    binary: Path, workload: Mapping[str, object], iterations: Optional[int] = None
) -> List[str]:
    """Build the exact telomere-cli command represented by a workload record."""

    artifact = workload.get("artifact")
    if not isinstance(artifact, dict) or not isinstance(artifact.get("path"), str):
        raise BaselineError(
            "invalid_workload",
            f"workload {workload.get('name')} lacks an artifact path",
        )
    command = [str(binary), str(artifact["path"])]
    if workload.get("kind") == "wat":
        export = workload.get("export")
        if not isinstance(export, str) or iterations is None:
            raise BaselineError(
                "invalid_workload",
                f"WAT workload {workload.get('name')} needs export and iterations",
            )
        command.extend([export, str(iterations)])
    return command


def validate_workload_stdout(workload: Mapping[str, object], stdout: str) -> Optional[float]:
    """Refuse every workload result that has not self-validated."""

    validation = workload.get("validation")
    if not isinstance(validation, dict):
        raise BaselineError(
            "invalid_workload",
            f"workload {workload.get('name')} has no validation contract",
        )
    kind = validation.get("kind")
    if kind == "coremark":
        try:
            return coremark_score(stdout)
        except MeasurementError as error:
            raise BaselineError("unparseable_coremark_output", str(error)) from error
    if kind == "exact_stdout":
        expected_template = validation.get("expected_stdout")
        active_iterations = workload.get("active_iterations")
        if not isinstance(expected_template, str) or not isinstance(active_iterations, int):
            raise BaselineError(
                "invalid_workload",
                f"workload {workload.get('name')} has invalid stdout metadata",
            )
        expected_stdout = expected_template.format(n=active_iterations)
        if stdout != expected_stdout:
            raise BaselineError(
                "workload_validation_failed",
                f"workload {workload.get('name')} printed {stdout!r}; "
                f"expected {expected_stdout!r}",
            )
        return None
    raise BaselineError(
        "invalid_workload",
        f"workload {workload.get('name')} has unsupported validation {kind}",
    )


def run_one_sample(
    binary: Path,
    workload: Mapping[str, object],
    requested_optimizer: Optional[str],
    iterations: Optional[int],
) -> Dict[str, object]:
    """Time a single whole CLI process after setting one unambiguous switch."""

    active_workload = dict(workload)
    if iterations is not None:
        active_workload["active_iterations"] = iterations
    command = workload_command(binary, active_workload, iterations)
    load_before = observed_sample_load_average()
    started = time.perf_counter_ns()
    stdout = checked_output(command, measurement_env(requested_optimizer))
    wall_ns = time.perf_counter_ns() - started
    load_after = observed_sample_load_average()
    metric = validate_workload_stdout(active_workload, stdout)
    record: Dict[str, object] = {
        "wall_ns": wall_ns,
        "command": command,
        "load_average_1m_before": load_before,
        "load_average_1m_after": load_after,
    }
    if metric is not None:
        record["metric"] = metric
    return record


def observed_sample_load_average() -> float:
    """Read one finite load observation or fail closed before publication."""

    try:
        value = os.getloadavg()[0]
    except (AttributeError, IndexError, OSError, TypeError) as error:
        raise BaselineError(
            "sample_load_unavailable",
            f"could not read one-minute load average around a sample: {error}",
        ) from error
    if not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        raise BaselineError(
            "sample_load_non_finite",
            "sample one-minute load average must be finite",
            observed_load_average_1m=value,
        )
    return float(value)


def physical_schedule_items(
    workload: Mapping[str, object]
) -> Dict[str, Tuple[str, Optional[int]]]:
    """Create a physical schedule item for each arm and each L2 scale point."""

    items: Dict[str, Tuple[str, Optional[int]]] = {}
    if workload.get("layer") == "L1":
        for arm in ARM_CONFIGS:
            items[arm] = (arm, None)
        return items

    n = workload.get("n")
    if not isinstance(n, int) or n <= 0:
        raise BaselineError(
            "invalid_workload",
            f"L2 workload {workload.get('name')} needs a positive integer n",
        )
    for arm in ARM_CONFIGS:
        for multiplier in (1, 2, 3):
            items[f"{arm}@{multiplier}n"] = (arm, n * multiplier)
    return items


def scale_permutation_key(permutation: Sequence[int]) -> str:
    """Render a scale order in the same notation as physical schedule labels."""

    return ",".join(f"{scale}n" for scale in permutation)


def scale_position_audit(
    permutations: Sequence[Sequence[int]], arms: Sequence[str]
) -> Dict[str, object]:
    """Audit scale positions both per arm and across the L2 block schedule."""

    base_counts = {
        f"{scale}n": {str(position + 1): 0 for position in range(3)}
        for scale in SCALE_MULTIPLIERS
    }
    for permutation in permutations:
        if tuple(permutation) not in SCALE_PERMUTATIONS:
            raise BaselineError(
                "invalid_scale_permutation",
                f"invalid L2 scale permutation {tuple(permutation)!r}",
            )
        for position, scale in enumerate(permutation):
            base_counts[f"{scale}n"][str(position + 1)] += 1
    values = [count for counts in base_counts.values() for count in counts.values()]
    minimum = min(values) if values else 0
    maximum = max(values) if values else 0
    return {
        "per_arm_counts": {
            arm: {
                scale: dict(position_counts)
                for scale, position_counts in base_counts.items()
            }
            for arm in arms
        },
        "per_arm_min": minimum,
        "per_arm_max": maximum,
        "per_arm_imbalance": maximum - minimum,
        "per_arm_exactly_balanced": minimum == maximum,
    }


def l2_scale_permutation_plan(rounds: int, seed: int) -> Dict[str, object]:
    """Plan adjacent L2 scale orders with an honest 12+3 balance record."""

    if rounds <= 0:
        raise BaselineError("invalid_rounds", "L2 schedule rounds must be positive")
    rng = random.Random(seed)
    full_groups, partial_group_rounds = divmod(rounds, 15)
    round_permutations: List[Tuple[int, int, int]] = []
    group_records = []
    group_count = full_groups + (1 if partial_group_rounds else 0)
    for group_index in range(group_count):
        balanced_prefix = list(SCALE_PERMUTATIONS) * 2
        rng.shuffle(balanced_prefix)
        residual_cycle = list(SCALE_RESIDUAL_CYCLES[rng.randrange(len(SCALE_RESIDUAL_CYCLES))])
        rng.shuffle(residual_cycle)
        group = balanced_prefix + residual_cycle
        take = 15 if group_index < full_groups else partial_group_rounds
        start = len(round_permutations)
        selected = group[:take]
        round_permutations.extend(selected)
        prefix_count = min(take, 12)
        selected_prefix = selected[:prefix_count]
        group_records.append(
            {
                "group": group_index,
                "round_indices": list(range(start, start + take)),
                "balanced_prefix_rounds": prefix_count,
                "balanced_prefix_permutation_counts": {
                    scale_permutation_key(permutation): selected_prefix.count(permutation)
                    for permutation in SCALE_PERMUTATIONS
                },
                "balanced_prefix_complete": prefix_count == 12,
                "residual_round_indices": list(
                    range(start + 12, start + take)
                )
                if take > 12
                else [],
                "residual_permutations": [
                    list(permutation) for permutation in selected[12:]
                ],
            }
        )

    permutation_counts = {
        scale_permutation_key(permutation): round_permutations.count(permutation)
        for permutation in SCALE_PERMUTATIONS
    }
    counts = list(permutation_counts.values())
    count_minimum = min(counts) if counts else 0
    count_maximum = max(counts) if counts else 0
    return {
        "permutations": round_permutations,
        "metadata": {
            "method": "twelve_rounds_each_permutation_twice_plus_seeded_three_round_residual",
            "seed": seed,
            "full_fifteen_round_groups": full_groups,
            "partial_group_rounds": partial_group_rounds,
            "balanced_prefix_rounds_per_group": 12,
            "residual_scale_rounds_per_full_group": 3,
            "permutation_counts_per_round": permutation_counts,
            "permutation_count_min": count_minimum,
            "permutation_count_max": count_maximum,
            "permutation_count_imbalance": count_maximum - count_minimum,
            "exact_permutation_balance_claimed": False,
            "groups": group_records,
        },
    }


def l2_block_schedule(rounds: int, seed: int) -> Dict[str, object]:
    """Build Williams-ordered arm blocks with adjacent n/2n/3n samples."""

    arms = list(ARM_CONFIGS)
    arm_plan = williams_schedule(arms, rounds, seed)
    arm_rows = arm_plan["schedule"]
    assert isinstance(arm_rows, list)
    scale_plan = l2_scale_permutation_plan(rounds, seed + 1)
    scale_orders = scale_plan["permutations"]
    assert isinstance(scale_orders, list)
    if len(arm_rows) != len(scale_orders):
        raise BaselineError(
            "schedule_length_mismatch",
            "Williams arm blocks and L2 scale orders have different lengths",
        )
    schedule = [
        [
            f"{arm}@{scale}n"
            for arm in arm_row
            for scale in scale_order
        ]
        for arm_row, scale_order in zip(arm_rows, scale_orders)
    ]
    scale_metadata = dict(scale_plan["metadata"])
    scale_metadata["permutation_counts_across_blocks"] = {
        key: count * len(arms)
        for key, count in scale_metadata["permutation_counts_per_round"].items()
    }
    scale_metadata["per_arm_permutation_counts"] = {
        arm: dict(scale_metadata["permutation_counts_per_round"])
        for arm in arms
    }
    scale_metadata["position_balance"] = scale_position_audit(scale_orders, arms)
    arm_metadata = arm_plan["metadata"]
    assert isinstance(arm_metadata, dict)
    return {
        "schedule": schedule,
        "metadata": {
            "method": "williams_arm_blocks_with_adjacent_scale_orders",
            "full_cycles": arm_metadata["full_cycles"],
            "residual_rounds": arm_metadata["residual_rounds"],
            "carryover_scope": arm_metadata["balance_audit"]["carryover_scope"],
            "balance_audit": arm_metadata["balance_audit"],
            "block_size": 3,
            "blocks_per_round": len(arms),
            "scales_per_block": ["1n", "2n", "3n"],
            "within_block_order": "seeded_permutation",
            "arm_block_order": arm_metadata,
            "scale_order": scale_metadata,
        },
    }


def workload_schedule(
    workload: Mapping[str, object], rounds: int, seed: int
) -> Dict[str, object]:
    """Choose the documented schedule form for one workload layer."""

    if workload.get("layer") == "L1":
        plan = williams_schedule(list(ARM_CONFIGS), rounds, seed)
        return {
            "schedule": plan["schedule"],
            "metadata": plan["metadata"],
        }
    if workload.get("layer") == "L2":
        return l2_block_schedule(rounds, seed)
    raise BaselineError(
        "invalid_workload",
        f"workload {workload.get('name')} has unsupported layer",
    )


def raw_arm_metrics(
    workload: Mapping[str, object],
    samples: Mapping[str, object],
    floor: Optional[float],
) -> Dict[str, Dict[str, object]]:
    """Turn raw sample vectors into L1 scores or L2 paired slopes."""

    metrics: Dict[str, Dict[str, object]] = {}
    if workload.get("layer") == "L1":
        for arm, values in samples.items():
            assert isinstance(values, list)
            scores = [float(sample["metric"]) for sample in values]
            metrics[arm] = {
                "base_metric": "iterations_per_second",
                "values": scores,
                "median": statistics.median(scores),
                "min": min(scores),
                "max": max(scores),
                "invalid_reason": None,
            }
        return metrics

    n = workload.get("n")
    assert isinstance(n, int)
    for arm, vectors in samples.items():
        assert isinstance(vectors, dict)
        metric = paired_slopes(
            n,
            [float(sample["wall_ns"]) for sample in vectors[1]],
            [float(sample["wall_ns"]) for sample in vectors[2]],
            [float(sample["wall_ns"]) for sample in vectors[3]],
            floor,
        )
        metric["base_metric"] = "nanoseconds_per_iteration"
        metrics[arm] = metric
    return metrics


def noise_inputs(
    workload: Mapping[str, object], metrics: Mapping[str, Mapping[str, object]]
) -> Tuple[List[float], List[float]]:
    """Return default-a/default-b samples in the same base metric units."""

    if workload.get("layer") == "L1":
        left = metrics["default-a"].get("values")
        right = metrics["default-b"].get("values")
    else:
        left = metrics["default-a"].get("slopes_n_to_2n")
        right = metrics["default-b"].get("slopes_n_to_2n")
    if not isinstance(left, list) or not isinstance(right, list):
        raise BaselineError(
            "invalid_slope",
            "default A/A arms did not produce usable base-metric samples",
        )
    return [float(value) for value in left], [float(value) for value in right]


def bootstrap_median_interval(values: Sequence[float], seed: int) -> Optional[Dict[str, float]]:
    """Return a two-sided 95 percent bootstrap interval for a paired median."""

    if len(values) < 10:
        return None
    rng = random.Random(seed)
    medians = []
    for _ in range(10_000):
        sample = [values[rng.randrange(len(values))] for _ in values]
        medians.append(statistics.median(sample))
    ordered = sorted(medians)

    def percentile(fraction: float) -> float:
        position = (len(ordered) - 1) * fraction
        lower = math.floor(position)
        upper = math.ceil(position)
        if lower == upper:
            return ordered[lower]
        return ordered[lower] + (ordered[upper] - ordered[lower]) * (
            position - lower
        )

    return {"lower": percentile(0.025), "upper": percentile(0.975)}


def comparison_record(
    name: str,
    left: Sequence[float],
    right: Sequence[float],
    floor: Optional[float],
    quick: bool,
    seed: int,
) -> Dict[str, object]:
    """Apply the exact paired symmetric delta and below-floor reporting rule."""

    try:
        contrasts = paired_contrasts(left, right)
    except MeasurementError as error:
        return {
            "name": name,
            "status": "invalid",
            "invalid_reason": str(error),
        }
    delta = statistics.median(contrasts)
    result: Dict[str, object] = {
        "name": name,
        "left_median": statistics.median(left),
        "right_median": statistics.median(right),
        "paired_contrasts": contrasts,
        "n": len(contrasts),
        "floor": floor,
        "bootstrap_percentile_ci_95": None,
        "invalid_reason": None,
    }
    if floor is None:
        result.update(
            {
                "status": "invalid",
                "invalid_reason": "insufficient_samples_for_interval",
            }
        )
        return result
    if below_noise_floor(delta, floor):
        result["status"] = "below_noise_floor"
        return result
    if quick or len(contrasts) < 10:
        result.update(
            {
                "status": "invalid",
                "invalid_reason": "insufficient_samples_for_interval",
            }
        )
        return result
    result.update(
        {
            "status": "reported_delta",
            "relative_delta": delta,
            "bootstrap_percentile_ci_95": bootstrap_median_interval(contrasts, seed),
        }
    )
    return result


def report_workload(
    workload: Mapping[str, object],
    builds: Mapping[str, Mapping[str, object]],
    rounds: int,
    warmup: int,
    seed: int,
    quick: bool,
) -> Dict[str, object]:
    """Run one L1 or L2 workload through every physical arm and scale point."""

    items = physical_schedule_items(workload)
    schedule_plan = workload_schedule(workload, rounds, seed)
    schedule = schedule_plan["schedule"]
    assert isinstance(schedule, list)
    schedule_metadata = schedule_plan["metadata"]
    if warmup:
        warmup_plan = workload_schedule(workload, warmup, seed + 1)
        warmup_schedule = warmup_plan["schedule"]
        assert isinstance(warmup_schedule, list)
        warmup_schedule_metadata: Optional[object] = warmup_plan["metadata"]
    else:
        warmup_schedule = []
        warmup_schedule_metadata = None
    samples: Dict[str, object]
    if workload.get("layer") == "L1":
        samples = {arm: [] for arm in ARM_CONFIGS}
    else:
        samples = {arm: {1: [], 2: [], 3: []} for arm in ARM_CONFIGS}

    def execute(label: str) -> Dict[str, object]:
        arm, iterations = items[label]
        spec = ARM_CONFIGS[arm]
        build_name = spec["build"]
        assert isinstance(build_name, str)
        binary = Path(str(builds[build_name]["copied_binary"]))
        return run_one_sample(binary, workload, spec["optimizer"], iterations)

    for order in warmup_schedule:
        for label in order:
            execute(label)

    for round_index, order in enumerate(schedule):
        for label in order:
            arm, iterations = items[label]
            sample = execute(label)
            sample["round"] = round_index
            if workload.get("layer") == "L1":
                assert isinstance(samples[arm], list)
                samples[arm].append(sample)
            else:
                assert iterations is not None
                n = workload.get("n")
                assert isinstance(n, int)
                multiplier = iterations // n
                assert isinstance(samples[arm], dict)
                samples[arm][multiplier].append(sample)

    preliminary = raw_arm_metrics(workload, samples, None)
    left, right = noise_inputs(workload, preliminary)
    try:
        aa_contrasts = paired_contrasts(left, right)
        floor_record = noise_floor(aa_contrasts, seed)
    except MeasurementError as error:
        raise BaselineError("noise_floor_failed", str(error)) from error
    floor_value = floor_record["floor"]
    if floor_value is not None:
        floor_value = float(floor_value)
    metrics = raw_arm_metrics(workload, samples, floor_value)

    base_values: Dict[str, List[float]] = {}
    for arm, metric in metrics.items():
        if workload.get("layer") == "L1":
            base_values[arm] = [float(value) for value in metric["values"]]
            metric["bootstrap_percentile_ci_95"] = bootstrap_median_interval(
                base_values[arm], seed
            )
            continue
        metric["slope_bootstrap_percentile_ci_95"] = bootstrap_median_interval(
            [float(value) for value in metric.get("slopes_n_to_2n", [])],
            seed,
        )
        linearity = metric.get("linearity")
        if (
            metric.get("slope") is not None
            and isinstance(linearity, dict)
            and linearity.get("is_linear")
        ):
            slopes = metric.get("slopes_n_to_2n")
            if isinstance(slopes, list):
                base_values[arm] = [float(value) for value in slopes]

    comparisons = []
    for index, (name, left_arm, right_arm) in enumerate(COMPARISONS):
        if left_arm not in base_values or right_arm not in base_values:
            comparisons.append(
                {
                    "name": name,
                    "status": "invalid",
                    "invalid_reason": "linearity_or_metric_unavailable",
                }
            )
            continue
        comparisons.append(
            comparison_record(
                name,
                base_values[left_arm],
                base_values[right_arm],
                floor_value,
                quick,
                seed + index + 1,
            )
        )

    invalid_reasons = [
        str(metric["invalid_reason"])
        for metric in metrics.values()
        if metric.get("invalid_reason")
    ]
    invalid_reasons.extend(
        str(comparison["invalid_reason"])
        for comparison in comparisons
        if comparison.get("invalid_reason")
    )
    control = next(
        comparison
        for comparison in comparisons
        if comparison["name"] == "measure_switches_control"
    )
    controls = [
        comparison
        for comparison in comparisons
        if comparison["name"].startswith("measure_switches_control")
    ]
    if any(control.get("status") == "reported_delta" for control in controls):
        invalid_reasons.append("measure_switches_control_exceeds_noise_floor")

    return {
        "name": workload["name"],
        "layer": workload["layer"],
        "artifact": workload["artifact"],
        "validation": workload["validation"],
        "rounds": rounds,
        "warmup_rounds": warmup,
        "schedule_seed": seed,
        "warmup_schedule": warmup_schedule,
        "warmup_schedule_metadata": warmup_schedule_metadata,
        "schedule": schedule,
        "schedule_metadata": schedule_metadata,
        "samples": samples,
        "base_metrics": metrics,
        "noise_floor": floor_record,
        "comparisons": comparisons,
        "invalid_reason": invalid_reasons[0] if invalid_reasons else None,
        "all_invalid_reasons": invalid_reasons,
    }


def load_average(facts: Mapping[str, object]) -> float:
    """Require a finite one-minute load average for a fail-closed baseline."""

    value = facts.get("load_average_1m")
    if not isinstance(value, (int, float)) or not math.isfinite(float(value)):
        raise BaselineError(
            "load_average_unavailable",
            "could not read a finite one-minute load average",
        )
    return float(value)


def mark_workloads_invalid(
    workloads: Sequence[Mapping[str, object]], reason: str
) -> List[Dict[str, object]]:
    """Keep a failed-with-reason entry for every workload not allowed to run."""

    return [
        {
            "name": workload.get("name"),
            "layer": workload.get("layer"),
            "status": "not_run",
            "invalid_reason": reason,
        }
        for workload in workloads
    ]


def run_baseline(
    args: argparse.Namespace, builds: Mapping[str, Mapping[str, object]]
) -> Tuple[Dict[str, object], bool]:
    """Prepare artifacts, verify witnesses, and run the full measurement matrix."""

    report: Dict[str, object] = {
        "schema_version": 1,
        "tool": "measure-interpreter-baseline",
        "mode": args.mode,
        "status": "running",
        "invalid_reason": None,
        "quick": args.quick,
        "publishable": False,
        "matrix": {
            "builds": builds,
            "arms": ARM_CONFIGS,
            "aa_arms": ["default-a", "default-b"],
        },
        "workloads": [],
    }
    workloads = load_manifest(args.manifest)
    report["switch_states"] = observe_switches(builds)
    witnesses, witness_failures = capture_witnesses(builds, args.mode, args.baseline)
    report["tail_call_witnesses"] = witnesses
    if witness_failures:
        report["workloads"] = mark_workloads_invalid(workloads, witness_failures[0])
        report["status"] = "invalid"
        report["invalid_reason"] = witness_failures[0]
        return report, False

    materialized = materialize_workloads(workloads, args)
    report["artifacts"] = [
        {
            "name": workload["name"],
            "layer": workload["layer"],
            "status": workload["artifact_status"],
            "artifact": workload["artifact"],
            "invalid_reason": workload["invalid_reason"],
            "error": workload.get("error"),
        }
        for workload in materialized
    ]
    if args.mode == "compare":
        assert args.baseline is not None
        try:
            report["comparison_artifact_identity"] = validate_compare_artifacts(
                args.baseline, materialized
            )
        except BaselineError as error:
            report["workloads"] = [
                (
                    {
                        "name": workload["name"],
                        "layer": workload["layer"],
                        "artifact": None,
                        "status": "invalid",
                        "invalid_reason": workload["invalid_reason"],
                        "error": workload.get("error"),
                        "error_context": workload.get("error_context", {}),
                    }
                    if workload["artifact_status"] != "ready"
                    else {
                        "name": workload["name"],
                        "layer": workload["layer"],
                        "status": "not_run",
                        "invalid_reason": error.reason,
                    }
                )
                for workload in materialized
            ]
            report["status"] = "invalid"
            report["invalid_reason"] = error.reason
            report["comparison_artifact_identity"] = {
                "status": "invalid",
                "invalid_reason": error.reason,
                "error": error.detail,
                "error_context": error.context,
            }
            return report, False
    thresholds = {
        "max_start_load_1m": (
            args.max_start_load
            if args.max_start_load is not None
            else default_start_load_threshold()
        ),
        "max_load_rise_1m": args.max_load_rise,
    }
    report["busy_machine_thresholds"] = thresholds
    start_facts = machine_facts()
    report["machine_start"] = start_facts
    start_load = load_average(start_facts)
    if start_load > float(thresholds["max_start_load_1m"]):
        report["workloads"] = [
            (
                {
                    "name": workload["name"],
                    "layer": workload["layer"],
                    "artifact": workload["artifact"],
                    "status": "invalid",
                    "invalid_reason": workload["invalid_reason"],
                    "error": workload.get("error"),
                }
                if workload["artifact_status"] != "ready"
                else {
                    "name": workload["name"],
                    "layer": workload["layer"],
                    "status": "not_run",
                    "invalid_reason": "busy_machine",
                }
            )
            for workload in materialized
        ]
        report["status"] = "invalid"
        report["invalid_reason"] = "busy_machine"
        return report, False

    rounds = effective_rounds(args)
    warmup = effective_warmup(args)
    workload_failures = []
    for index, workload in enumerate(materialized):
        if workload["artifact_status"] != "ready":
            result = {
                "name": workload["name"],
                "layer": workload["layer"],
                "artifact": None,
                "status": "invalid",
                "invalid_reason": workload["invalid_reason"],
                "error": workload.get("error"),
                "error_context": workload.get("error_context", {}),
            }
        else:
            try:
                result = report_workload(
                    workload,
                    builds,
                    rounds,
                    warmup,
                    args.seed + index * 1_000,
                    args.quick,
                )
            except BaselineError as error:
                result = {
                    "name": workload.get("name"),
                    "layer": workload.get("layer"),
                    "artifact": workload.get("artifact"),
                    "status": "invalid",
                    "invalid_reason": error.reason,
                    "error": error.detail,
                    "error_context": error.context,
                }
        report["workloads"].append(result)
        if result.get("invalid_reason"):
            workload_failures.append(str(result["invalid_reason"]))

    end_facts = machine_facts()
    report["machine_end"] = end_facts
    end_load = load_average(end_facts)
    if end_load - start_load > float(thresholds["max_load_rise_1m"]):
        report["timings_reportable"] = False
        report["status"] = "invalid"
        report["invalid_reason"] = "busy_machine"
        return report, False

    report["timings_reportable"] = True
    if workload_failures:
        report["status"] = "invalid"
        report["invalid_reason"] = workload_failures[0]
        return report, False
    report["status"] = "ok"
    report["publishable"] = not args.quick and effective_rounds(args) >= 10
    return report, True


def error_report(reason: str, detail: str, **context: object) -> Dict[str, object]:
    """Return JSON for failures before a full matrix report exists."""

    return {
        "schema_version": 1,
        "tool": "measure-interpreter-baseline",
        "status": "invalid",
        "invalid_reason": reason,
        "publishable": False,
        "error": detail,
        "error_context": context,
    }


def emit_report(report: Mapping[str, object], output_path: Optional[Path]) -> None:
    """Atomically persist and print exactly one JSON document."""

    rendered = json.dumps(report, indent=2, sort_keys=True)
    if output_path is not None:
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(
            mode="w",
            encoding="utf-8",
            delete=False,
            dir=output_path.parent,
        ) as temporary:
            temporary.write(rendered)
            temporary.write("\n")
            temporary_path = Path(temporary.name)
        temporary_path.replace(output_path)
    print(rendered)


def main(argv: Optional[Sequence[str]] = None) -> int:
    """Build a reproducible binary matrix and leave JSON on every error path."""

    output_path: Optional[Path] = None
    try:
        args = parse_args(argv)
        output_path = args.out
        builds = build_binaries(args)
        report: Dict[str, object] = {
            "schema_version": 1,
            "tool": "measure-interpreter-baseline",
            "mode": args.mode,
            "build_only": args.build_only,
            "invalid_reason": None,
            "publishable": False,
            "matrix": {
                "builds": builds,
                "arms": ARM_CONFIGS,
                "aa_arms": ["default-a", "default-b"],
            },
        }
        if args.build_only:
            report["status"] = "ok"
            successful = True
        else:
            report, successful = run_baseline(args, builds)
    except BaselineError as error:
        report = error_report(error.reason, error.detail, **error.context)
        successful = False
    except (MeasurementError, OSError, ValueError) as error:
        report = error_report("measurement_setup_failed", str(error))
        successful = False

    try:
        emit_report(report, output_path)
    except OSError as error:
        print(json.dumps(error_report("output_write_failed", str(error)), indent=2))
        return 1
    return 0 if successful else 1


if __name__ == "__main__":
    raise SystemExit(main())
