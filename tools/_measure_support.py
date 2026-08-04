#!/usr/bin/env python3
"""Shared helpers for process timing and baseline-measurement statistics."""

import hashlib
import math
import os
import platform
import random
import re
import shlex
import statistics
import subprocess
import time
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple, Union


MACOS_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size", re.M)
LINUX_RSS = re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")
COREMARK_ITERATIONS_PER_SECOND = re.compile(
    r"^\s*Iterations/Sec\s*:\s*([0-9][0-9,]*(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)\b",
    re.M,
)

PathLike = Union[str, os.PathLike]


class MeasurementError(RuntimeError):
    """Raised when a measurement tool did not produce usable output."""


def rss_bytes(stderr: str) -> Optional[int]:
    """Return peak RSS in bytes from macOS or GNU time output."""

    macos = MACOS_RSS.search(stderr)
    if macos:
        return int(macos.group(1))

    linux = LINUX_RSS.search(stderr)
    if linux:
        return int(linux.group(1)) * 1024

    return None


def time_argv() -> List[str]:
    """Return the platform-specific command used to collect peak RSS."""

    return ["/usr/bin/time", "-l" if platform.system() == "Darwin" else "-v"]


def run(cmd: Sequence[str], expected_stdout: Optional[str] = None) -> str:
    """Run *cmd*, returning stderr and optionally checking its exact stdout."""

    completed = subprocess.run(
        list(cmd),
        stdout=subprocess.PIPE if expected_stdout is not None else subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    if expected_stdout is not None and completed.stdout != expected_stdout:
        raise MeasurementError(
            f"`{shlex.join(cmd)}` printed {completed.stdout!r}; "
            f"expected {expected_stdout!r}"
        )
    return completed.stderr


def median(values: Sequence[float]) -> float:
    """Return the statistics median with one shared implementation."""

    return statistics.median(values)


def wall_ms_samples(
    cmd: Sequence[str],
    warmup: int,
    runs: int,
    expected_stdout: Optional[str] = None,
) -> List[float]:
    """Warm a process, then collect whole-process wall-clock samples in ms."""

    for _ in range(warmup):
        run(cmd, expected_stdout)

    samples = []
    for _ in range(runs):
        started = time.perf_counter_ns()
        run(cmd, expected_stdout)
        samples.append((time.perf_counter_ns() - started) / 1_000_000)

    return samples


def peak_rss_samples(
    cmd: Sequence[str],
    runs: int,
    expected_stdout: Optional[str] = None,
) -> Tuple[List[int], List[str]]:
    """Collect peak-RSS samples and return them with the timed command."""

    timed_cmd = time_argv() + list(cmd)
    samples = []
    for _ in range(runs):
        stderr = run(timed_cmd, expected_stdout)
        parsed = rss_bytes(stderr)
        if parsed is None:
            raise MeasurementError(
                "could not find peak RSS in the output of "
                f"`{shlex.join(timed_cmd)}`:\n"
                f"{stderr}"
            )
        samples.append(parsed)

    return samples, timed_cmd


def counterbalanced_schedule(
    arms: Sequence[str], rounds: int, seed: int
) -> List[List[str]]:
    """Return the rows of :func:`williams_schedule` for legacy callers.

    The earlier helper independently shuffled each round.  That is unbiased
    only in expectation and can leave a short realised run badly imbalanced.
    Keep the list-of-rows API for existing tools while giving it the Williams
    design used by the interpreter-baseline harness.
    """

    plan = williams_schedule(arms, rounds, seed)
    schedule = plan["schedule"]
    assert isinstance(schedule, list)
    return schedule


def _validated_arms(arms: Sequence[str], rounds: int) -> List[str]:
    """Validate the common input contract for finite treatment schedules."""

    normalized_arms = list(arms)
    if not normalized_arms:
        raise MeasurementError("counterbalanced schedule needs at least one arm")
    if len(normalized_arms) != len(set(normalized_arms)):
        raise MeasurementError("counterbalanced schedule arm labels must be unique")
    if rounds <= 0:
        raise MeasurementError("counterbalanced schedule rounds must be positive")
    return normalized_arms


def _williams_rows(arms: Sequence[str]) -> List[List[str]]:
    """Construct a canonical Williams cycle for an odd or even treatment count.

    The cyclic shifts give a Latin square.  For odd treatment counts, adding
    the reversed shifts gives the standard 2*n Williams cycle and balances
    every *directed* within-row first-order carryover exactly twice.
    """

    normalized_arms = list(arms)
    count = len(normalized_arms)
    if count == 1:
        return [normalized_arms]

    base = [0]
    next_low = 1
    next_high = count - 1
    while len(base) < count:
        base.append(next_low)
        next_low += 1
        if len(base) < count:
            base.append(next_high)
            next_high -= 1

    rows = [
        [normalized_arms[(treatment + shift) % count] for treatment in base]
        for shift in range(count)
    ]
    if count % 2:
        rows.extend([list(reversed(row)) for row in rows])
    return rows


def _empty_directed_counts(
    arms: Sequence[str], include_self: bool
) -> Dict[str, Dict[str, int]]:
    return {
        source: {
            destination: 0
            for destination in arms
            if include_self or destination != source
        }
        for source in arms
    }


def _count_summary(counts: Dict[str, Dict[str, int]]) -> Dict[str, int]:
    values = [count for destinations in counts.values() for count in destinations.values()]
    if not values:
        return {"min": 0, "max": 0, "imbalance": 0}
    lower = min(values)
    upper = max(values)
    return {"min": lower, "max": upper, "imbalance": upper - lower}


def _williams_balance_audit(
    rows: Sequence[Sequence[str]], arms: Sequence[str], residual_start: int
) -> Dict[str, object]:
    """Record exact within-row balance and observational boundary balance.

    Williams balance is a statement about transitions *inside* a design row.
    The harness runs rows sequentially, so it separately records the actual
    between-row transitions without claiming they inherit that exact property.
    """

    positions = {
        arm: {str(position + 1): 0 for position in range(len(arms))}
        for arm in arms
    }
    within_counts = _empty_directed_counts(arms, include_self=False)
    boundary_counts = _empty_directed_counts(arms, include_self=True)

    for row in rows:
        if len(row) != len(arms) or set(row) != set(arms):
            raise MeasurementError("Williams schedule row does not contain each arm once")
        for position, arm in enumerate(row):
            positions[arm][str(position + 1)] += 1
        for source, destination in zip(row, row[1:]):
            within_counts[source][destination] += 1

    boundary_transitions = []
    for round_index, (previous, following) in enumerate(zip(rows, rows[1:])):
        source = previous[-1]
        destination = following[0]
        boundary_counts[source][destination] += 1
        boundary_transitions.append(
            {
                "from_round": round_index,
                "to_round": round_index + 1,
                "from": source,
                "to": destination,
            }
        )

    position_values = [count for values in positions.values() for count in values.values()]
    position_minimum = min(position_values) if position_values else 0
    position_maximum = max(position_values) if position_values else 0
    residual_indices = list(range(residual_start, len(rows)))
    residual_internal = [
        {
            "round": round_index,
            "pairs": [
                {"from": source, "to": destination}
                for source, destination in zip(rows[round_index], rows[round_index][1:])
            ],
        }
        for round_index in residual_indices
    ]
    residual_boundary = [
        transition
        for transition in boundary_transitions
        if transition["from_round"] >= residual_start
        or transition["to_round"] >= residual_start
    ]

    within_summary = _count_summary(within_counts)
    boundary_summary = _count_summary(boundary_counts)
    return {
        "carryover_scope": "within_round",
        "positions": {
            "counts": positions,
            "min": position_minimum,
            "max": position_maximum,
            "imbalance": position_maximum - position_minimum,
            "exactly_balanced": position_minimum == position_maximum,
        },
        "within_round_directed_carryover": {
            "counts": within_counts,
            **within_summary,
            "exactly_balanced": within_summary["imbalance"] == 0,
        },
        "round_boundary_directed_carryover": {
            "counts": boundary_counts,
            "transitions": boundary_transitions,
            **boundary_summary,
            "exact_balance_claimed": False,
        },
        "residual": {
            "round_indices": residual_indices,
            "internal_directed_carryover": residual_internal,
            "round_boundary_directed_carryover": residual_boundary,
        },
    }


def _boundary_score(rows: Sequence[Sequence[str]]) -> Tuple[int, int, int]:
    """Score sequential row boundaries; lower is better and never a claim."""

    counts: Dict[Tuple[str, str], int] = {}
    for previous, following in zip(rows, rows[1:]):
        pair = (previous[-1], following[0])
        counts[pair] = counts.get(pair, 0) + 1
    if not counts:
        return (0, 0, 0)
    duplicate_count = sum(count - 1 for count in counts.values())
    return (duplicate_count, max(counts.values()), -len(counts))


def williams_schedule(
    arms: Sequence[str], rounds: int, seed: int
) -> Dict[str, object]:
    """Return a deterministic Williams schedule plus an honest balance audit.

    The seed selects an order for complete Williams rows and any incomplete
    residual rows.  It is also used to choose among 512 candidate row orders
    with the fewest observed round-boundary duplicate transitions.  That is a
    practical boundary reduction only: the exact carryover guarantee remains
    explicitly scoped to within-row transitions.
    """

    normalized_arms = _validated_arms(arms, rounds)
    cycle = _williams_rows(normalized_arms)
    cycle_rows = len(cycle)
    full_cycles, residual_rounds = divmod(rounds, cycle_rows)
    rng = random.Random(seed)
    candidate_count = 512
    best: Optional[Tuple[Tuple[int, int, int], List[int], List[int]]] = None
    for _ in range(candidate_count):
        row_order = list(range(cycle_rows))
        rng.shuffle(row_order)
        residual_indices = rng.sample(row_order, residual_rounds)
        candidate_indices = row_order * full_cycles + residual_indices
        candidate_rows = [cycle[index] for index in candidate_indices]
        score = _boundary_score(candidate_rows)
        if best is None or score < best[0]:
            best = (score, row_order, residual_indices)

    assert best is not None
    score, row_order, residual_indices = best
    schedule_indices = row_order * full_cycles + residual_indices
    schedule = [list(cycle[index]) for index in schedule_indices]
    residual_start = full_cycles * cycle_rows
    repetitions = cycle_rows // len(normalized_arms)
    balance_audit = _williams_balance_audit(schedule, normalized_arms, residual_start)
    return {
        "schedule": schedule,
        "metadata": {
            "method": "williams_carryover_balanced_latin_square",
            "seed": seed,
            "cycle_rows": cycle_rows,
            "full_cycles": full_cycles,
            "residual_rounds": residual_rounds,
            "cycle_row_order": row_order,
            "residual_row_indices": residual_indices,
            "carryover_scope": balance_audit["carryover_scope"],
            "full_cycle_contract": {
                "position_count_per_arm_position": repetitions,
                "within_round_directed_carryover_count_per_distinct_pair": repetitions,
            },
            "row_ordering": {
                "method": "seeded_minimum_round_boundary_duplicate_search",
                "candidate_count": candidate_count,
                "round_boundary_duplicate_count": score[0],
                "round_boundary_max_count": score[1],
                "round_boundary_distinct_pairs": -score[2],
            },
            "balance_audit": balance_audit,
        },
    }


def coremark_score(stdout: str) -> float:
    """Extract a validated CoreMark ``Iterations/Sec`` score, fail closed."""

    if "Correct operation validated" not in stdout:
        raise MeasurementError(
            "CoreMark did not print `Correct operation validated`; refusing its score"
        )

    matches = COREMARK_ITERATIONS_PER_SECOND.findall(stdout)
    if len(matches) != 1:
        raise MeasurementError(
            "expected exactly one CoreMark `Iterations/Sec` line, found "
            f"{len(matches)}"
        )

    try:
        score = float(matches[0].replace(",", ""))
    except ValueError as error:
        raise MeasurementError("CoreMark `Iterations/Sec` is not numeric") from error
    if not math.isfinite(score) or score <= 0:
        raise MeasurementError("CoreMark `Iterations/Sec` must be finite and positive")
    return score


def symmetric_relative_contrast(left: float, right: float) -> float:
    """Return the signed, symmetric relative contrast of two positive metrics."""

    if not math.isfinite(left) or not math.isfinite(right):
        raise MeasurementError("contrast inputs must be finite")
    denominator = (left + right) / 2
    if denominator == 0:
        raise MeasurementError("contrast denominator must not be zero")
    return (left - right) / denominator


def paired_contrasts(
    left: Sequence[float], right: Sequence[float]
) -> List[float]:
    """Return round-aligned symmetric relative contrasts."""

    if len(left) != len(right):
        raise MeasurementError("paired contrasts need equally sized sample vectors")
    if not left:
        raise MeasurementError("paired contrasts need at least one sample")
    return [
        symmetric_relative_contrast(float(left_value), float(right_value))
        for left_value, right_value in zip(left, right)
    ]


def _percentile(values: Sequence[float], fraction: float) -> float:
    """Return a linearly interpolated percentile without version-dependent APIs."""

    if not values:
        raise MeasurementError("cannot calculate a percentile of no values")
    if not 0 <= fraction <= 1:
        raise MeasurementError("percentile fraction must be between zero and one")
    ordered = sorted(values)
    position = (len(ordered) - 1) * fraction
    lower = math.floor(position)
    upper = math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def noise_floor(
    contrasts: Sequence[float], seed: int, bootstrap_resamples: int = 10_000
) -> Dict[str, object]:
    """Derive the conservative per-metric A/A noise floor from paired contrasts.

    For a publishable run (at least ten pairs), the floor is the larger of the
    upper 95% bootstrap percentile bound of ``median(abs(c_i))`` and the
    empirical 95th percentile of ``abs(c_i)``.  Short runs deliberately use a
    weaker, clearly marked rule so callers cannot turn ``--quick`` output into
    a quotable delta.
    """

    normalized = [float(value) for value in contrasts]
    if any(not math.isfinite(value) for value in normalized):
        raise MeasurementError("noise-floor contrasts must be finite")
    if bootstrap_resamples <= 0:
        raise MeasurementError("bootstrap resamples must be positive")

    absolute = [abs(value) for value in normalized]
    result: Dict[str, object] = {
        "paired_contrasts": normalized,
        "absolute_contrasts": absolute,
        "n": len(normalized),
        "bootstrap_seed": seed,
        "bootstrap_resamples": bootstrap_resamples,
        "publishable": len(normalized) >= 10,
        "invalid_reason": None,
    }
    if len(normalized) < 3:
        result.update(
            {
                "floor": None,
                "bootstrap_percentile_ci_95": None,
                "empirical_p95": None,
                "method": "insufficient_samples",
                "invalid_reason": "insufficient_samples_for_interval",
            }
        )
        return result

    if len(normalized) < 10:
        floor = max(absolute)
        result.update(
            {
                "floor": floor,
                "bootstrap_percentile_ci_95": None,
                "empirical_p95": floor,
                "method": "small_sample_max_abs",
                "sample_count_status": "quick_only",
            }
        )
        return result

    rng = random.Random(seed)
    resampled_medians = []
    for _ in range(bootstrap_resamples):
        sample = [absolute[rng.randrange(len(absolute))] for _ in absolute]
        resampled_medians.append(statistics.median(sample))
    ci_lower = _percentile(resampled_medians, 0.025)
    ci_upper = _percentile(resampled_medians, 0.975)
    empirical_p95 = _percentile(absolute, 0.95)
    result.update(
        {
            "floor": max(ci_upper, empirical_p95),
            "bootstrap_percentile_ci_95": {
                "lower": ci_lower,
                "upper": ci_upper,
            },
            "empirical_p95": empirical_p95,
            "method": "max_bootstrap_upper_and_empirical_p95",
        }
    )
    return result


def below_noise_floor(delta: float, floor: float) -> bool:
    """Implement the reporting predicate exactly: ``abs(delta) <= floor``."""

    if not math.isfinite(delta):
        raise MeasurementError("delta must be finite")
    if not math.isfinite(floor) or floor < 0:
        raise MeasurementError("noise floor must be finite and non-negative")
    return abs(delta) <= floor


def paired_slopes(
    n: int,
    at_n: Sequence[float],
    at_2n: Sequence[float],
    at_3n: Sequence[float],
    linearity_floor: Optional[float] = None,
) -> Dict[str, object]:
    """Calculate paired L2 slopes and its guarded n/2n/3n linearity verdict.

    The samples must be aligned by round and use one time unit consistently.
    The caller supplies nanoseconds to obtain the documented ``ns/iteration``
    base metric, but the calculation itself deliberately remains unit-agnostic.
    """

    if n <= 0:
        raise MeasurementError("slope base iteration count must be positive")
    lengths = {len(at_n), len(at_2n), len(at_3n)}
    if len(lengths) != 1 or not at_n:
        raise MeasurementError("paired slopes need non-empty, equally sized vectors")

    samples = [
        [float(value) for value in at_n],
        [float(value) for value in at_2n],
        [float(value) for value in at_3n],
    ]
    if any(not math.isfinite(value) for vector in samples for value in vector):
        return {
            "n": n,
            "slope": None,
            "constant_term": None,
            "invalid_reason": "non_finite_increment",
            "linearity": {"is_linear": False, "invalid_reason": "non_finite_increment"},
        }

    d12 = [(second - first) / n for first, second in zip(samples[0], samples[1])]
    d23 = [(third - second) / n for second, third in zip(samples[1], samples[2])]
    constant_terms = [
        first - slope * n for first, slope in zip(samples[0], d12)
    ]
    median_d12 = statistics.median(d12)
    median_d23 = statistics.median(d23)
    result: Dict[str, object] = {
        "n": n,
        "t_n": samples[0],
        "t_2n": samples[1],
        "t_3n": samples[2],
        "paired_deltas_n_to_2n": [second - first for first, second in zip(samples[0], samples[1])],
        "paired_deltas_2n_to_3n": [third - second for second, third in zip(samples[1], samples[2])],
        "slopes_n_to_2n": d12,
        "slopes_2n_to_3n": d23,
        "slope": median_d12,
        "constant_terms": constant_terms,
        "constant_term": statistics.median(constant_terms),
        "invalid_reason": None,
    }
    if (
        not math.isfinite(median_d12)
        or not math.isfinite(median_d23)
        or any(not math.isfinite(value) for value in d12 + d23)
    ):
        result["invalid_reason"] = "non_finite_increment"
        result["linearity"] = {
            "is_linear": False,
            "invalid_reason": "non_finite_increment",
        }
        result["slope"] = None
        result["constant_term"] = None
        return result
    if median_d12 <= 0 or median_d23 <= 0:
        result["invalid_reason"] = "non_positive_increment"
        result["linearity"] = {
            "is_linear": False,
            "invalid_reason": "non_positive_increment",
        }
        result["slope"] = None
        result["constant_term"] = None
        return result
    if linearity_floor is None:
        result["linearity"] = {
            "is_linear": None,
            "invalid_reason": "linearity_floor_unavailable",
        }
        return result
    if not math.isfinite(linearity_floor) or linearity_floor < 0:
        raise MeasurementError("linearity floor must be finite and non-negative")

    relative_difference = abs(median_d23 - median_d12) / median_d12
    is_linear = relative_difference <= linearity_floor
    result["linearity"] = {
        "is_linear": is_linear,
        "relative_difference": relative_difference,
        "floor": linearity_floor,
        "invalid_reason": None if is_linear else "linearity_exceeds_noise_floor",
    }
    if not is_linear:
        result["invalid_reason"] = "linearity_exceeds_noise_floor"
        result["slope"] = None
        result["constant_term"] = None
    return result


def sha256_file(path: PathLike) -> str:
    """Return a streaming SHA-256 digest for a measured artifact."""

    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _command_version(command: Sequence[str]) -> Optional[str]:
    try:
        completed = subprocess.run(
            list(command),
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    return completed.stdout.strip() or None


def _cpu_model() -> Optional[str]:
    if platform.system() == "Darwin":
        for key in ("machdep.cpu.brand_string", "hw.model"):
            value = _command_version(["sysctl", "-n", key])
            if value:
                return value
        return None
    try:
        with open("/proc/cpuinfo", encoding="utf-8") as cpuinfo:
            for line in cpuinfo:
                if line.lower().startswith(("model name", "hardware")) and ":" in line:
                    return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return None


def _load_average_1m() -> Optional[float]:
    try:
        return os.getloadavg()[0]
    except (AttributeError, OSError):
        return None


def machine_facts() -> Dict[str, object]:
    """Capture the host facts that make a baseline record auditable."""

    return {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "kernel": platform.release(),
        "cpu_model": _cpu_model(),
        "load_average_1m": _load_average_1m(),
        "commit": _command_version(["git", "rev-parse", "HEAD"]),
        "cargo_version": _command_version(["cargo", "--version"]),
        "rustc_version": _command_version(["rustc", "--version"]),
    }
