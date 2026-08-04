#!/usr/bin/env python3
"""Fail-closed tail-call-form witness for the direct-threaded interpreter.

The witness deliberately examines the successful dispatch edges of a small,
representative probe set.  It does *not* infer anything from a handler's last
physical instruction: Rust/LLVM commonly append cold panic blocks after the
hot indirect branch.

The module is usable by ``tools/measure-interpreter-baseline.py`` and also has
an intentionally small standalone CLI::

    python3 tools/baseline/tail_call_witness.py \
        --binary target/release/telomere-cli --mode first-record

The JSON record is fail-closed.  ``contract_passed`` is false for an unknown
architecture, a missing disassembler, an unparseable probe, or a non-tail
dispatch edge.  First-record mode checks the absolute contract; comparison
mode additionally refuses a reduction in the number of observed dispatch
exits relative to its named prior record.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, Iterable, List, Mapping, Optional, Sequence, Set, Tuple, Union


SCHEMA_VERSION = 1
PROBES: Tuple[str, ...] = (
    "op_i32_and",
    "op_local_get4_br_if",
    "op_i32_load_const_base",
    "op_call",
)
SUPPORTED_ARCHITECTURES = frozenset(("arm64", "x86_64"))
PANIC_SYMBOLS = (
    "slice_index_fail",
    "panic_bounds_check",
    "core..panicking..",
    "core::panicking::",
    "panic_fmt",
    "panic_in_cleanup",
)


class WitnessUnavailable(RuntimeError):
    """Raised when a binary cannot be inspected without guessing."""


@dataclass(frozen=True)
class Instruction:
    """One disassembled machine instruction."""

    address: int
    raw: str
    mnemonic: str
    operands: str

    @property
    def address_text(self) -> str:
        return f"0x{self.address:x}"


@dataclass
class Function:
    """A symbol and the instructions emitted for it by the disassembler."""

    symbol: str
    instructions: List[Instruction]


MACH_LABEL = re.compile(r"^(?P<symbol>[A-Za-z_.$][^\s:]*):\s*$")
GNU_LABEL = re.compile(
    r"^\s*(?:0x)?[0-9A-Fa-f]+\s+<(?P<symbol>[^>]+)>:\s*$"
)
MACH_INSTRUCTION = re.compile(
    r"^\s*(?:0x)?(?P<address>[0-9A-Fa-f]+)\s+(?P<body>.+?)\s*$"
)
GNU_INSTRUCTION = re.compile(
    r"^\s*(?:0x)?(?P<address>[0-9A-Fa-f]+):\s*(?P<body>.+?)\s*$"
)
ASSEMBLY = re.compile(r"^(?P<mnemonic>[A-Za-z][A-Za-z0-9_.]*)\s*(?P<operands>.*)$")
HEX_BYTES = re.compile(r"^(?:(?:[0-9A-Fa-f]{2,8})\s+)+")
ARM_REGISTER = re.compile(r"\b(x(?:[0-9]|[12][0-9]|30))\b", re.I)
X86_REGISTER = re.compile(r"(%(?:r(?:[0-9]+|[a-z]{2,3})|e[a-z]{2}|[abcd]x|[sd]i|[sb]p))\b", re.I)
# GNU objdump commonly annotates a branch target as ``1050 <return>``.  Require
# numeric-token boundaries so hex-looking letters in that symbol annotation do
# not displace the target when selecting the final numeric operand.
ADDRESS_IN_OPERANDS = re.compile(r"(?<![0-9A-Za-z_])(?:0x)?([0-9A-Fa-f]+)(?![0-9A-Za-z_])")


def _format_address(address: int) -> str:
    return f"0x{address:x}"


def _normalise_architecture(value: str) -> Optional[str]:
    value = value.lower()
    if "arm64" in value or "aarch64" in value:
        return "arm64"
    if "x86-64" in value or "x86_64" in value or "amd64" in value:
        return "x86_64"
    return None


def detect_binary_architecture(binary: Path) -> str:
    """Return a supported binary architecture, or raise without guessing."""

    file_tool = shutil.which("file")
    if file_tool is None:
        raise WitnessUnavailable("file_tool_unavailable")
    completed = subprocess.run(
        [file_tool, "-b", str(binary)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise WitnessUnavailable("binary_architecture_unavailable")
    architecture = _normalise_architecture(completed.stdout)
    if architecture not in SUPPORTED_ARCHITECTURES:
        raise WitnessUnavailable("unsupported_architecture")
    return architecture


def _disassembler_command(binary: Path) -> List[str]:
    system = platform.system()
    if system == "Darwin":
        tool = shutil.which("otool")
        if tool is None:
            raise WitnessUnavailable("otool_unavailable")
        return [tool, "-tvV", str(binary)]
    if system == "Linux":
        tool = os.environ.get("TELOMERE_OBJDUMP") or shutil.which("objdump")
        if not tool:
            raise WitnessUnavailable("objdump_unavailable")
        return [tool, "-d", "--demangle=rust", str(binary)]
    raise WitnessUnavailable("unsupported_host_platform")


def disassemble_binary(binary: Path) -> Tuple[str, List[str]]:
    """Disassemble *binary* with the host's supported native tool."""

    command = _disassembler_command(binary)
    completed = subprocess.run(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise WitnessUnavailable("disassembler_failed")
    if not completed.stdout.strip():
        raise WitnessUnavailable("empty_disassembly")
    return completed.stdout, command


def _parse_label(line: str) -> Optional[str]:
    gnu = GNU_LABEL.match(line)
    if gnu:
        return gnu.group("symbol")
    mach = MACH_LABEL.match(line)
    if mach:
        return mach.group("symbol")
    return None


def _parse_instruction(line: str) -> Optional[Instruction]:
    gnu = GNU_INSTRUCTION.match(line)
    match = gnu or MACH_INSTRUCTION.match(line)
    if match is None:
        return None

    body = match.group("body").strip()
    if gnu:
        body = HEX_BYTES.sub("", body).strip()
    assembly = ASSEMBLY.match(body)
    if assembly is None:
        return None
    return Instruction(
        address=int(match.group("address"), 16),
        raw=line.strip(),
        mnemonic=assembly.group("mnemonic").lower(),
        operands=assembly.group("operands").strip(),
    )


def parse_functions(disassembly: str) -> List[Function]:
    """Parse Mach-O ``otool`` or GNU ``objdump`` function boundaries."""

    functions: List[Function] = []
    current: Optional[Function] = None
    for line in disassembly.splitlines():
        symbol = _parse_label(line)
        if symbol is not None:
            if current is not None:
                functions.append(current)
            current = Function(symbol=symbol, instructions=[])
            continue
        if current is not None:
            instruction = _parse_instruction(line)
            if instruction is not None:
                current.instructions.append(instruction)
    if current is not None:
        functions.append(current)
    return functions


def _symbol_matches_probe(symbol: str, probe: str) -> bool:
    """Match Rust mangled or demangled symbols without accepting a prefix."""

    return re.search(
        # In Rust's v0-like length-prefixed spelling the probe is preceded by
        # its decimal component length (``7op_call``).  Requiring that digit
        # also prevents ``internal_op_call`` from being mistaken for the
        # public ``op_call`` probe.  Demangled output instead has ``::``.
        rf"(?:^|::|[0-9]){re.escape(probe)}(?:17h[0-9A-Fa-f]+E|::h[0-9A-Fa-f]+|$)",
        symbol,
    ) is not None


def _find_probe_function(functions: Sequence[Function], probe: str) -> Optional[Function]:
    matches = [function for function in functions if _symbol_matches_probe(function.symbol, probe)]
    if len(matches) != 1:
        return None
    return matches[0]


def _address_target(operands: str) -> Optional[int]:
    matches = ADDRESS_IN_OPERANDS.findall(operands)
    if not matches:
        return None
    # Conditional arm64 branches include a condition register before their
    # target (for example ``cbz w8, 0x1000``).  The branch target is the final
    # numeric token, not the register's trailing digit.
    return int(matches[-1], 16)


def _is_arm_conditional_branch(mnemonic: str) -> bool:
    return mnemonic.startswith("b.") or mnemonic in {
        "cbz",
        "cbnz",
        "tbz",
        "tbnz",
    }


def _is_x86_conditional_branch(mnemonic: str) -> bool:
    return mnemonic.startswith("j") and mnemonic not in {"jmp", "jmpq"}


def _successor_indices(
    instructions: Sequence[Instruction], architecture: str
) -> Dict[int, List[int]]:
    by_address = {instruction.address: index for index, instruction in enumerate(instructions)}
    successors: Dict[int, List[int]] = {}

    for index, instruction in enumerate(instructions):
        following = [index + 1] if index + 1 < len(instructions) else []
        mnemonic = instruction.mnemonic
        target = _address_target(instruction.operands)
        target_index = by_address.get(target) if target is not None else None

        if mnemonic.startswith("ret"):
            successors[index] = []
        elif architecture == "arm64" and mnemonic == "br":
            transfer = _arm_transfer(instruction)
            if transfer is not None and _has_recent_dispatch_load(
                instructions, index, architecture, transfer[1]
            ):
                # This is the direct-threaded handoff itself.  It leaves this
                # handler, so there is no in-function successor.
                successors[index] = []
            else:
                # LLVM also emits ``br xN`` for local jump tables (notably in
                # ``op_call``).  Their table targets are local blocks and are
                # not printed by otool as CFG edges.  Keeping the lexical
                # successor is conservative: it lets us inspect every local
                # success arm instead of declaring the real tail edge absent.
                successors[index] = following
        elif architecture == "arm64" and mnemonic == "b":
            successors[index] = [target_index] if target_index is not None else []
        elif architecture == "arm64" and _is_arm_conditional_branch(mnemonic):
            successors[index] = following + ([target_index] if target_index is not None else [])
        elif architecture == "x86_64" and mnemonic in {"jmp", "jmpq"}:
            if instruction.operands.lstrip().startswith("*"):
                successors[index] = []
            else:
                successors[index] = [target_index] if target_index is not None else []
        elif architecture == "x86_64" and _is_x86_conditional_branch(mnemonic):
            successors[index] = following + ([target_index] if target_index is not None else [])
        else:
            successors[index] = following
    return successors


def _reachable_indices(successors: Mapping[int, Sequence[int]], length: int) -> Set[int]:
    if length == 0:
        return set()
    reachable: Set[int] = set()
    pending: deque[int] = deque((0,))
    while pending:
        index = pending.popleft()
        if index in reachable or index < 0 or index >= length:
            continue
        reachable.add(index)
        pending.extend(successors.get(index, ()))
    return reachable


def _arm_transfer(instruction: Instruction) -> Optional[Tuple[str, str]]:
    if instruction.mnemonic not in {"br", "blr"}:
        return None
    match = ARM_REGISTER.search(instruction.operands)
    if match is None:
        return None
    return instruction.mnemonic, match.group(1).lower()


def _x86_transfer(instruction: Instruction) -> Optional[Tuple[str, str]]:
    if instruction.mnemonic not in {"jmp", "jmpq", "call", "callq"}:
        return None
    if not instruction.operands.lstrip().startswith("*"):
        return None
    match = X86_REGISTER.search(instruction.operands)
    if match is None:
        return None
    return instruction.mnemonic, match.group(1).lower()


def _instruction_loads_register(
    instruction: Instruction, architecture: str, register: str
) -> bool:
    operands = instruction.operands.lower()
    if architecture == "arm64":
        if instruction.mnemonic not in {"ldr", "ldur", "ldp"}:
            return False
        escaped = re.escape(register)
        return re.match(rf"\s*{escaped}\s*,.*\[", operands) is not None

    if instruction.mnemonic not in {"mov", "movq", "movl", "lea", "leaq"}:
        return False
    escaped = re.escape(register)
    att = re.search(rf"\([^)]*\)\s*,\s*{escaped}\b", operands) is not None
    intel = re.match(rf"\s*{escaped}\s*,.*\[", operands) is not None
    return att or intel


def _has_recent_dispatch_load(
    instructions: Sequence[Instruction], index: int, architecture: str, register: str
) -> bool:
    """Recognise an instruction-pointer load feeding an indirect transfer.

    The fixed window is intentional.  It excludes long-lived function-pointer
    registers (for example host callbacks in ``op_call``), while accepting the
    short epilogue between ``ldr x2, [tail_code]`` and ``br x2`` emitted by the
    release compiler.  It is a candidate filter rather than conclusive proof:
    a short-lived returning helper is distinguished later by its post-call
    CFG path to a separately recognised tail branch.
    """

    lower = max(0, index - 12)
    for candidate in reversed(instructions[lower:index]):
        if _instruction_loads_register(candidate, architecture, register):
            return True
        if candidate.mnemonic.startswith("ret"):
            break
    return False


def _ret_reachable_after(
    start: int, instructions: Sequence[Instruction], successors: Mapping[int, Sequence[int]]
) -> bool:
    pending: deque[int] = deque(successors.get(start, ()))
    visited: Set[int] = set()
    while pending:
        index = pending.popleft()
        if index in visited or index < 0 or index >= len(instructions):
            continue
        visited.add(index)
        if instructions[index].mnemonic.startswith("ret"):
            return True
        pending.extend(successors.get(index, ()))
    return False


def _tail_dispatch_reachable_after(
    start: int,
    tail_dispatch_indices: Set[int],
    instructions: Sequence[Instruction],
    successors: Mapping[int, Sequence[int]],
) -> Optional[int]:
    """Return a tail-dispatch index reachable after an indirect call.

    A short-lived function pointer can look exactly like a dispatch pointer:
    LLVM's JIT instrumentation emits ``ldr xN`` followed by ``blr xN`` before
    resuming the handler and eventually performing the real ``br xM``
    dispatch.  That ``blr`` must not be counted as an exit merely because the
    loaded register resembles one.  The distinction is intentionally CFG
    based rather than based on physical instruction order: starting at the
    call's fallthrough, we require a separately recognised direct tail branch
    to be reachable.  Without that evidence, the indirect call remains a
    non-tail dispatch exit and fails the absolute contract.
    """

    pending: deque[int] = deque(successors.get(start, ()))
    visited: Set[int] = set()
    while pending:
        index = pending.popleft()
        if index in visited or index < 0 or index >= len(instructions):
            continue
        visited.add(index)
        if index in tail_dispatch_indices:
            return index
        pending.extend(successors.get(index, ()))
    return None


def _is_panic_call(instruction: Instruction) -> bool:
    if instruction.mnemonic not in {"bl", "blr", "call", "callq"}:
        return False
    lowered = instruction.raw.lower()
    return any(symbol in lowered for symbol in PANIC_SYMBOLS)


def _excluded_blocks(function: Function, reachable: Set[int]) -> List[Dict[str, Any]]:
    exclusions: List[Dict[str, Any]] = []
    for index, instruction in enumerate(function.instructions):
        if _is_panic_call(instruction):
            exclusions.append(
                {
                    "address": instruction.address_text,
                    "reason": "known_panic_symbol",
                    "instruction": instruction.raw,
                    "reachable_from_entry": index in reachable,
                }
            )
        elif instruction.mnemonic.startswith("ret"):
            exclusions.append(
                {
                    "address": instruction.address_text,
                    "reason": "non_dispatch_return",
                    "instruction": instruction.raw,
                    "reachable_from_entry": index in reachable,
                }
            )
    return exclusions


def _analyse_probe(function: Optional[Function], probe: str, architecture: str) -> Dict[str, Any]:
    if function is None:
        return {
            "probe": probe,
            "symbol": None,
            "status": "witness_unavailable",
            "dispatch_exits": [],
            "excluded_dispatch_transfers": [],
            "excluded_blocks": [],
            "invalid_reason": "probe_symbol_not_found_or_ambiguous",
        }
    if not function.instructions:
        return {
            "probe": probe,
            "symbol": function.symbol,
            "status": "witness_unavailable",
            "dispatch_exits": [],
            "excluded_dispatch_transfers": [],
            "excluded_blocks": [],
            "invalid_reason": "probe_has_no_instructions",
        }

    successors = _successor_indices(function.instructions, architecture)
    reachable = _reachable_indices(successors, len(function.instructions))
    transfers: List[Dict[str, Any]] = []
    for index in sorted(reachable):
        instruction = function.instructions[index]
        transfer = (
            _arm_transfer(instruction)
            if architecture == "arm64"
            else _x86_transfer(instruction)
        )
        if transfer is None:
            continue
        mnemonic, register = transfer
        if not _has_recent_dispatch_load(function.instructions, index, architecture, register):
            continue
        tail = mnemonic in {"br", "jmp", "jmpq"}
        transfers.append(
            {
                "index": index,
                "address": instruction.address_text,
                "kind": "tail_branch" if tail else "indirect_call",
                "dispatch_register": register,
                "instruction": instruction.raw,
                "ret_reachable_after_transfer": _ret_reachable_after(
                    index, function.instructions, successors
                ),
            }
        )

    tail_dispatch_indices = {
        transfer["index"] for transfer in transfers if transfer["kind"] == "tail_branch"
    }
    exits: List[Dict[str, Any]] = []
    excluded_dispatch_transfers: List[Dict[str, Any]] = []
    for transfer in transfers:
        tail_index = (
            _tail_dispatch_reachable_after(
                transfer["index"],
                tail_dispatch_indices,
                function.instructions,
                successors,
            )
            if transfer["kind"] == "indirect_call"
            else None
        )
        if tail_index is not None:
            tail_instruction = function.instructions[tail_index]
            excluded_dispatch_transfers.append(
                {
                    "address": transfer["address"],
                    "reason": "returning_indirect_helper_before_tail_dispatch",
                    "instruction": transfer["instruction"],
                    "dispatch_register": transfer["dispatch_register"],
                    "tail_dispatch_address": tail_instruction.address_text,
                    "tail_dispatch_instruction": tail_instruction.raw,
                    "ret_reachable_after_transfer": transfer[
                        "ret_reachable_after_transfer"
                    ],
                }
            )
            continue
        exits.append(
            {
                key: value
                for key, value in transfer.items()
                if key != "index"
            }
        )

    exclusions = _excluded_blocks(function, reachable)
    if not exits:
        status = "witness_unavailable"
        invalid_reason = "no_reachable_non_error_dispatch_exit"
    elif any(exit_["kind"] != "tail_branch" for exit_ in exits):
        status = "fail"
        invalid_reason = "non_tail_dispatch_exit"
    else:
        status = "pass"
        invalid_reason = None
    return {
        "probe": probe,
        "symbol": function.symbol,
        "status": status,
        "dispatch_exits": exits,
        "excluded_dispatch_transfers": excluded_dispatch_transfers,
        "excluded_blocks": exclusions,
        "invalid_reason": invalid_reason,
    }


def _coverage_text(probes: Iterable[Mapping[str, Any]]) -> str:
    verified = sum(1 for probe in probes if probe.get("status") == "pass")
    return f"{verified} of {len(PROBES)} probes verified"


def _record_from_probes(
    probes: List[Dict[str, Any]], architecture: str, mode: str
) -> Dict[str, Any]:
    unavailable = any(probe["status"] == "witness_unavailable" for probe in probes)
    failed = any(probe["status"] == "fail" for probe in probes)
    if unavailable:
        status = "witness_unavailable"
        invalid_reason: Optional[str] = "witness_unavailable"
    elif failed:
        status = "fail"
        invalid_reason = "absolute_tail_call_contract_failed"
    else:
        status = "pass"
        invalid_reason = None
    absolute_passed = status == "pass"
    return {
        "schema_version": SCHEMA_VERSION,
        "witness": "tail_call_form",
        "mode": mode,
        "architecture": architecture,
        "status": status,
        "absolute_contract_passed": absolute_passed,
        "contract_passed": absolute_passed,
        "invalid_reason": invalid_reason,
        "probe_coverage": _coverage_text(probes),
        "probes": probes,
        "relative_comparison": {"status": "not_requested", "regressions": []},
    }


def analyse_disassembly(
    disassembly: str, architecture: str, mode: str = "first-record"
) -> Dict[str, Any]:
    """Analyse an assembly text fixture or native disassembler output.

    This function has no subprocess dependency and is the parser-test entry
    point.  It intentionally treats missing/ambiguous symbols as unavailable.
    """

    if architecture not in SUPPORTED_ARCHITECTURES:
        return _unavailable_record(mode, "unsupported_architecture", architecture)
    functions = parse_functions(disassembly)
    probes = [
        _analyse_probe(_find_probe_function(functions, probe), probe, architecture)
        for probe in PROBES
    ]
    return _record_from_probes(probes, architecture, mode)


def _unavailable_record(
    mode: str, reason: str, architecture: Optional[str] = None
) -> Dict[str, Any]:
    probes = [
        {
            "probe": probe,
            "symbol": None,
            "status": "witness_unavailable",
            "dispatch_exits": [],
            "excluded_dispatch_transfers": [],
            "excluded_blocks": [],
            "invalid_reason": reason,
        }
        for probe in PROBES
    ]
    return {
        "schema_version": SCHEMA_VERSION,
        "witness": "tail_call_form",
        "mode": mode,
        "architecture": architecture,
        "status": "witness_unavailable",
        "absolute_contract_passed": False,
        "contract_passed": False,
        "invalid_reason": reason,
        "probe_coverage": _coverage_text(probes),
        "probes": probes,
        "relative_comparison": {"status": "not_requested", "regressions": []},
    }


def _load_baseline_witness(path: Path) -> Mapping[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise WitnessUnavailable("baseline_witness_unreadable") from error
    if not isinstance(payload, Mapping):
        raise WitnessUnavailable("baseline_witness_invalid")
    for key in ("tail_call_witness", "witness"):
        nested = payload.get(key)
        if isinstance(nested, Mapping) and "probes" in nested:
            return nested
    if "probes" in payload:
        return payload
    raise WitnessUnavailable("baseline_witness_missing")


def _apply_relative_comparison(record: Dict[str, Any], baseline: Mapping[str, Any]) -> None:
    """Reject a coverage regression in a comparison record.

    Absolute form failures are already reflected in ``record``.  The relative
    component intentionally checks what an absolute form check cannot see: a
    parser/toolchain change that causes one of a prior record's distinct
    success exits to disappear from the current coverage.
    """

    baseline_probes = baseline.get("probes")
    if not isinstance(baseline_probes, list):
        record["relative_comparison"] = {
            "status": "fail",
            "regressions": ["baseline_witness_invalid"],
        }
        record["contract_passed"] = False
        if record["absolute_contract_passed"]:
            record["status"] = "fail"
            record["invalid_reason"] = "baseline_witness_invalid"
        return
    by_probe = {
        item.get("probe"): item
        for item in baseline_probes
        if isinstance(item, Mapping) and isinstance(item.get("probe"), str)
    }
    regressions: List[str] = []
    for current in record["probes"]:
        prior = by_probe.get(current["probe"])
        if not isinstance(prior, Mapping) or prior.get("status") != "pass":
            regressions.append(f"baseline_probe_invalid:{current['probe']}")
            continue
        prior_exits = prior.get("dispatch_exits")
        if not isinstance(prior_exits, list):
            regressions.append(f"baseline_probe_invalid:{current['probe']}")
            continue
        if len(current["dispatch_exits"]) < len(prior_exits):
            regressions.append(f"dispatch_exit_count_regression:{current['probe']}")

    record["relative_comparison"] = {
        "status": "pass" if not regressions else "fail",
        "regressions": regressions,
    }
    record["contract_passed"] = record["absolute_contract_passed"] and not regressions
    if regressions and record["absolute_contract_passed"]:
        record["status"] = "fail"
        record["invalid_reason"] = "relative_witness_regression"


def capture_tail_call_witness(
    binary_path: Union[os.PathLike, str],
    mode: str = "first-record",
    baseline_path: Optional[Union[os.PathLike, str]] = None,
    *,
    baseline_witness: Optional[Mapping[str, Any]] = None,
) -> Dict[str, Any]:
    """Capture one machine-readable witness for *binary_path*.

    The function never turns an uninspectable binary into a passing record.
    Callers should block result publication whenever ``contract_passed`` is
    false (or use :func:`witness_is_acceptable`).  Compare callers that have
    already selected the prior record for this binary should pass that exact
    mapping as ``baseline_witness``.  ``baseline_path`` remains for the
    standalone CLI and accepts one direct/legacy witness JSON record.
    """

    if mode not in {"first-record", "compare"}:
        raise ValueError("mode must be 'first-record' or 'compare'")
    if mode == "compare" and (baseline_path is None) == (baseline_witness is None):
        raise ValueError("compare mode requires exactly one baseline source")
    if mode == "first-record" and (
        baseline_path is not None or baseline_witness is not None
    ):
        raise ValueError("first-record mode does not accept a baseline source")

    binary = Path(binary_path)
    if not binary.is_file():
        return _unavailable_record(mode, "binary_not_found")
    try:
        architecture = detect_binary_architecture(binary)
        disassembly, command = disassemble_binary(binary)
        record = analyse_disassembly(disassembly, architecture, mode)
        record["binary"] = str(binary)
        record["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        record["disassembler"] = {"command": command}
        if mode == "compare":
            prior = (
                baseline_witness
                if baseline_witness is not None
                else _load_baseline_witness(Path(baseline_path))
            )
            _apply_relative_comparison(record, prior)
        return record
    except WitnessUnavailable as error:
        record = _unavailable_record(mode, str(error))
        record["binary"] = str(binary)
        return record
    except OSError:
        record = _unavailable_record(mode, "witness_io_error")
        record["binary"] = str(binary)
        return record


def witness_is_acceptable(record: Mapping[str, Any]) -> bool:
    """Return whether a baseline number may be published beside *record*."""

    return record.get("contract_passed") is True


def _parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--mode", required=True, choices=("first-record", "compare"))
    parser.add_argument("--baseline", type=Path)
    arguments = parser.parse_args(argv)
    if arguments.mode == "compare" and arguments.baseline is None:
        parser.error("--mode compare requires --baseline")
    if arguments.mode == "first-record" and arguments.baseline is not None:
        parser.error("--baseline is only valid with --mode compare")
    return arguments


def main(argv: Optional[Sequence[str]] = None) -> int:
    arguments = _parse_args(argv)
    record = capture_tail_call_witness(
        arguments.binary, mode=arguments.mode, baseline_path=arguments.baseline
    )
    json.dump(record, sys.stdout, indent=2, sort_keys=True)
    print()
    return 0 if witness_is_acceptable(record) else 1


if __name__ == "__main__":
    raise SystemExit(main())
