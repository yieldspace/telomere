#!/usr/bin/env python3
"""Gate the minimal-embedder binary size against a measured baseline.

Issue #139 established the minimal-embedder measurement
(tools/measure-embedder-footprint.py, driven by .github/workflows/footprint.yaml)
but deliberately shipped no regression gate: the proposed threshold had not
been derived from observed run-to-run variance. Issue #164 closes that gap.

The baseline in tools/embedder-size-baseline.json records, for every
configuration and profile the footprint workflow measures, the stripped file
size and the stripped .text size observed on CI. The observed run-to-run
spread for identical code was zero bytes (see docs/benchmarks/footprint.md,
"Size regression gate (#164)"), so each threshold equals its baseline: any
growth is either a regression or an intentional change that must re-baseline
through the procedure documented there. Peak RSS and cold start are
indicative on shared runners and are never gated.

This checker is stdlib-only, in the style of tools/check-feature-wiring.py.
It reads the two JSON files the footprint workflow already produces and
fails when a measured size exceeds its threshold, naming the measured size,
the threshold, and the baseline source in every failure message.
"""

import argparse
import json
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BASELINE = REPO_ROOT / "tools" / "embedder-size-baseline.json"
DEFAULT_MEASUREMENTS = ("footprint-release.json", "footprint-release-size.json")

# Metric name -> path of keys inside one configuration's measurement row.
METRICS = {
    "stripped_file_bytes": ("artifact", "stripped_file_bytes"),
    "stripped_text_bytes": ("artifact", "stripped_text_section", "bytes"),
}


class GateError(RuntimeError):
    """A baseline or measurement file is missing, malformed, or incomplete."""


def load_json(path):
    try:
        text = Path(path).read_text(encoding="utf-8")
    except OSError as error:
        raise GateError(f"could not read {path}: {error}") from error
    try:
        return json.loads(text)
    except json.JSONDecodeError as error:
        raise GateError(f"invalid JSON in {path}: {error}") from error


def measured_value(row, metric):
    """Extract one gated metric from a measurement row, or None if absent."""
    value = row
    for key in METRICS[metric]:
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    if not isinstance(value, int):
        return None
    return value


def index_measurements(measurements):
    """Map (profile, config) -> measurement row from footprint JSON documents."""
    indexed = {}
    for document in measurements:
        if not isinstance(document, dict):
            raise GateError("footprint measurement is not a JSON object")
        profile = document.get("profile")
        configs = document.get("configs")
        if not isinstance(profile, str) or not isinstance(configs, list):
            raise GateError(
                "footprint measurement lacks a profile or configs list; "
                "pass the JSON files produced by tools/measure-embedder-footprint.py"
            )
        for row in configs:
            if not isinstance(row, dict) or not isinstance(row.get("config"), str):
                raise GateError(f"malformed configuration row in {profile} measurement")
            indexed[(profile, row["config"])] = row
    return indexed


def check(baseline, measurements):
    """Return a list of failure strings; an empty list means the gate passes."""
    if not isinstance(baseline, dict):
        raise GateError("size baseline is not a JSON object")
    source = baseline.get("source")
    if not isinstance(source, dict) or not isinstance(source.get("summary"), str):
        raise GateError("size baseline lacks source.summary")
    thresholds = baseline.get("thresholds")
    if not isinstance(thresholds, dict) or not thresholds:
        raise GateError("size baseline lacks thresholds")

    indexed = index_measurements(measurements)
    failures = []
    for profile in sorted(thresholds):
        configs = thresholds[profile]
        if not isinstance(configs, dict):
            raise GateError(f"size baseline thresholds for {profile} are malformed")
        for config in sorted(configs):
            row = indexed.get((profile, config))
            if row is None:
                failures.append(
                    f"size gate: no measurement for {profile}/{config}; "
                    "the footprint run did not produce this configuration"
                )
                continue
            metrics = configs[config]
            if not isinstance(metrics, dict):
                raise GateError(
                    f"size baseline thresholds for {profile}/{config} are malformed"
                )
            for metric in sorted(metrics):
                entry = metrics[metric]
                if (
                    not isinstance(entry, dict)
                    or not isinstance(entry.get("baseline"), int)
                    or not isinstance(entry.get("threshold"), int)
                ):
                    raise GateError(
                        f"size baseline entry for {profile}/{config} {metric} "
                        "lacks integer baseline/threshold"
                    )
                value = measured_value(row, metric)
                if value is None:
                    failures.append(
                        f"size gate: measurement for {profile}/{config} lacks {metric}"
                    )
                    continue
                if value > entry["threshold"]:
                    failures.append(
                        f"size gate: {profile}/{config} {metric} measured "
                        f"{value} B exceeds threshold {entry['threshold']} B "
                        f"(baseline {entry['baseline']} B from {source['summary']}; "
                        "if this growth is intentional, re-baseline as documented "
                        "in docs/benchmarks/footprint.md)"
                    )
    return failures


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="fail when a minimal-embedder size exceeds its baseline threshold"
    )
    parser.add_argument(
        "--baseline",
        default=str(DEFAULT_BASELINE),
        help="size baseline JSON (default: tools/embedder-size-baseline.json)",
    )
    parser.add_argument(
        "measurements",
        nargs="*",
        default=list(DEFAULT_MEASUREMENTS),
        help="footprint measurement JSON files (default: footprint-release.json "
        "footprint-release-size.json in the working directory)",
    )
    return parser.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    try:
        baseline = load_json(args.baseline)
        measurements = [load_json(path) for path in args.measurements]
        failures = check(baseline, measurements)
    except GateError as error:
        print(f"size gate: error: {error}", file=sys.stderr)
        return 1
    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1
    print("size gate: all minimal-embedder sizes are within their thresholds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
