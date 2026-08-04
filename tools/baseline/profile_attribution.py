#!/usr/bin/env python3
"""Parse weighted flat profiler output into HandlerLayoutGroup tables.

The interpreter's direct-threaded handlers do not retain a useful caller tree,
so the attribution artifact deliberately uses flat samples only. Darwin's
``sample`` text carries an integer sample count at the start of each call-graph
line; Linux ``perf report --stdio --no-children --sort=symbol`` carries a
percentage Overhead field. Both are weights, not textual occurrence counts.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Dict, Iterable, Mapping, Optional, Sequence


class AttributionParseError(RuntimeError):
    """Raised when a profiler output cannot support a weighted report."""


OPERATOR = re.compile(r"op_[A-Za-z0-9_]+")
DARWIN_WEIGHT = re.compile(r"^\s*(?P<weight>[0-9]+)\s+(?P<body>.+?)\s*$")
PERF_WEIGHT = re.compile(r"^\s*(?P<weight>[0-9]+(?:\.[0-9]+)?)%\s+(?P<body>.+?)\s*$")


def normalise_symbol(text: str) -> Optional[str]:
    """Extract a vm-profile label from a Rust mangled or demangled symbol."""

    match = OPERATOR.search(text)
    if match is None:
        return None
    label = match.group(0)
    return re.sub(r"17h[0-9A-Fa-f]+E?$", "", label)


def _weighted_symbols(lines: Iterable[str], pattern: re.Pattern) -> Dict[str, float]:
    weights: Dict[str, float] = defaultdict(float)
    for line in lines:
        match = pattern.match(line)
        if match is None:
            continue
        label = normalise_symbol(match.group("body"))
        if label is None:
            continue
        weight = float(match.group("weight"))
        if weight > 0:
            weights[label] += weight
    if not weights:
        raise AttributionParseError("no_weighted_handler_symbols")
    return dict(weights)


def parse_darwin_sample(text: str) -> Dict[str, float]:
    """Return handler weights from Darwin ``sample`` call-graph line counts."""

    return _weighted_symbols(text.splitlines(), DARWIN_WEIGHT)


def parse_perf_report(text: str) -> Dict[str, float]:
    """Return handler weights from Linux ``perf report`` Overhead percentages."""

    return _weighted_symbols(text.splitlines(), PERF_WEIGHT)


def handler_layout_group(label: str) -> str:
    """Mirror runtime/vm.rs::handler_descriptor's layout-group precedence."""

    if label == "op_unreachable":
        return "Traps"
    if label.startswith("special_") or label in {
        "op_return",
        "op_end",
        "op_br",
        "op_else",
        "op_br_if",
        "op_br_table",
        "op_loop",
        "op_if",
    }:
        return "Control"
    if (
        label.startswith("op_local_get4_i32_const_add")
        or label.startswith("op_local_get4_local_get4_i32_add")
        or label.startswith("op_local_binop32")
        or label.startswith("op_local_binop64")
        or label.startswith("op_local_cmp32")
        or label.startswith("op_local_cmp64")
        or label.startswith("op_local_unary32")
        or label.startswith("op_local_unary64")
        or label
        in {
            "op_local_get4_br_if",
            "op_local_get4_i32_eqz_br_if",
            "op_local_get4_i32_const_compare_br_if",
            "op_local_get4_local_get4_compare_br_if",
        }
    ):
        return "Superinstructions"
    if label.startswith("op_mem_") or label == "op_data_drop":
        return "BulkMemory"
    if label.startswith("op_atomic"):
        return "Atomics"
    if label.startswith("op_call") or label.startswith("op_return_call"):
        return "Call"
    if "_load" in label or "_store" in label:
        return "Memory"
    if label.startswith("op_local_") or label.startswith("op_select") or label == "op_drop":
        return "Locals"
    if label.startswith("op_global_"):
        return "Globals"
    if label.startswith("op_table_"):
        return "Tables"
    if label.startswith("op_ref_"):
        return "Refs"
    if label.startswith("op_v128") or "x" in label:
        return "Simd"
    if label.startswith("op_i") or label.startswith("op_f"):
        return "Numeric"
    return "Other"


def fold_families(weights: Mapping[str, float]) -> Dict[str, float]:
    """Fold weighted symbols into the runtime's HandlerLayoutGroup vocabulary."""

    families: Dict[str, float] = defaultdict(float)
    for label, weight in weights.items():
        families[handler_layout_group(label)] += weight
    return dict(families)


def _share(weight: float, total: float) -> float:
    return 0.0 if total == 0 else 100.0 * weight / total


def _format_weight(weight: float) -> str:
    return str(int(weight)) if weight.is_integer() else f"{weight:.6f}".rstrip("0").rstrip(".")


def table_rows(weights: Mapping[str, float]) -> Iterable[str]:
    """Yield deterministic TSV rows containing each weight and its share."""

    total = sum(weights.values())
    for name, weight in sorted(weights.items(), key=lambda item: (-item[1], item[0])):
        yield f"{name}\t{_format_weight(weight)}\t{_share(weight, total):.2f}"


def write_tables(
    symbol_weights: Mapping[str, float], symbols_path: Path, families_path: Path
) -> None:
    """Write self-describing weighted symbol and family TSV tables."""

    symbols_path.write_text(
        "symbol\tflat_weight\tshare_percent\n"
        + "\n".join(table_rows(symbol_weights))
        + "\n",
        encoding="utf-8",
    )
    families_path.write_text(
        "family\tflat_weight\tshare_percent\n"
        + "\n".join(table_rows(fold_families(symbol_weights)))
        + "\n",
        encoding="utf-8",
    )


def _parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", required=True, choices=("sample", "perf"))
    parser.add_argument("--input", required=True, type=Path)
    parser.add_argument("--symbols-out", required=True, type=Path)
    parser.add_argument("--families-out", required=True, type=Path)
    return parser.parse_args(argv)


def main(argv: Optional[Sequence[str]] = None) -> int:
    arguments = _parse_args(argv)
    try:
        source = arguments.input.read_text(encoding="utf-8", errors="replace")
        parser = parse_darwin_sample if arguments.format == "sample" else parse_perf_report
        write_tables(parser(source), arguments.symbols_out, arguments.families_out)
    except (OSError, AttributionParseError) as error:
        print(f"attribution parser failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
