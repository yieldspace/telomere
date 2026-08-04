#!/usr/bin/env python3
"""Unit tests for the non-baseline environment-characterization tool."""

from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "characterize-baseline-environment.py"
SPEC = importlib.util.spec_from_file_location("characterize_baseline_environment", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
characterize = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = characterize
SPEC.loader.exec_module(characterize)


class FakeClock:
    def __init__(self) -> None:
        self.value = 0.0
        self.sleeps: list[float] = []

    def monotonic(self) -> float:
        return self.value

    def sleep(self, seconds: float) -> None:
        self.sleeps.append(seconds)
        self.value += seconds


class CharacterizationTests(unittest.TestCase):
    def test_collection_contains_all_load_series_summaries_and_processes(self) -> None:
        clock = FakeClock()
        loads = iter([(1.0, 2.0, 3.0), (4.0, 5.0, 6.0), (2.0, 3.0, 4.0)])
        timestamps = iter(
            [
                "2026-08-04T00:00:00Z",
                "2026-08-04T00:00:01Z",
                "2026-08-04T00:00:02Z",
                "2026-08-04T00:00:03Z",
                "2026-08-04T00:00:04Z",
            ]
        )

        payload = characterize.collect_characterization(
            duration_seconds=2.0,
            interval_seconds=1.0,
            top_count=2,
            now_utc=lambda: next(timestamps),
            monotonic=clock.monotonic,
            sleep=clock.sleep,
            getloadavg=lambda: next(loads),
            process_snapshot=lambda limit: [
                {"pid": 17, "cpu_percent": 97.0, "command": "other-work"}
            ][:limit],
        )

        self.assertEqual(clock.sleeps, [1.0, 1.0])
        self.assertEqual(payload["purpose"], "environment_characterization")
        self.assertEqual(payload["issue"], "yieldspace/telomere#184")
        self.assertFalse(payload["published_baseline_eligible"])
        self.assertFalse(payload["timing_measurement"])
        self.assertFalse(payload["load_gate_applied"])
        self.assertEqual(
            payload["load_gate_bypassed_reason"],
            "characterization_not_timing_measurement",
        )
        self.assertEqual(payload["sample_count"], 3)
        self.assertEqual(
            payload["load_average_series"],
            {
                "one_minute": [1.0, 4.0, 2.0],
                "five_minutes": [2.0, 5.0, 3.0],
                "fifteen_minutes": [3.0, 6.0, 4.0],
            },
        )
        self.assertEqual(
            payload["load_average_summary"]["one_minute"],
            {"minimum": 1.0, "maximum": 4.0, "first": 1.0, "last": 2.0},
        )
        self.assertEqual(
            payload["one_minute_load_summary"],
            payload["load_average_summary"]["one_minute"],
        )
        self.assertEqual(
            payload["samples"][0]["top_processes"],
            [{"pid": 17, "cpu_percent": 97.0, "command": "other-work"}],
        )

    def test_runtime_error_is_saved_as_nonpublishable_json(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "characterization.json"
            exit_status = characterize.run_characterization(
                output=output,
                duration_seconds=1.0,
                interval_seconds=1.0,
                top_count=1,
                now_utc=lambda: "2026-08-04T00:00:00Z",
                getloadavg=lambda: (_ for _ in ()).throw(OSError("load unavailable")),
            )

            payload = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(exit_status, 1)
            self.assertEqual(payload["status"], "invalid")
            self.assertFalse(payload["published_baseline_eligible"])
            self.assertFalse(payload["timing_measurement"])
            self.assertFalse(payload["load_gate_applied"])
            self.assertEqual(
                payload["invalid_reason"], "environment_characterization_error",
            )
            self.assertEqual(payload["error"]["type"], "OSError")

    def test_invalid_arguments_are_rejected_before_observation(self) -> None:
        parser = characterize.build_parser()
        for arguments in (
            ["--duration", "0"],
            ["--interval", "nan"],
            ["--top-count", "0"],
        ):
            with self.subTest(arguments=arguments), contextlib.redirect_stderr(
                io.StringIO()
            ), self.assertRaises(SystemExit):
                parser.parse_args(arguments)

    def test_process_parser_ignores_malformed_rows_and_sorts_cpu_descending(self) -> None:
        rows = characterize.parse_process_rows(
            "  12 1 4.5 0.2 00:01 R /usr/bin/slow\n"
            "malformed\n"
            "  13 1 95.0 1.2 00:02 S /usr/bin/busy\n",
            top_count=1,
        )
        self.assertEqual(rows[0]["pid"], 13)
        self.assertEqual(rows[0]["ppid"], 1)
        self.assertEqual(rows[0]["command"], "/usr/bin/busy")


if __name__ == "__main__":
    unittest.main()
