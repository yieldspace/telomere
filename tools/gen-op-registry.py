#!/usr/bin/env python3
"""Generate the stable op-ID registry for the precompiled-module codec (#196).

The codec serializes interpreter handlers (`Op` fn pointers) as stable u16
ids. This script extracts every handler the parser/optimizer can materialize
into a `LoweredFunction` from the sources that define them:

- direct `fn` definitions with the exact `Op` signature in
  `crates/telomere/src/runtime/vm.rs` and `crates/telomere/src/runtime/vm/*.rs`;
- handler names passed to handler-generating macros (macro bodies that
  contain `fn $<ident>`) in those files;
- the JIT baseline decoder's `OP_SPECS` table
  (`crates/telomere/src/runtime/jit/backend/ops.rs`);
- the `diagnostic_op_label` table in `runtime/vm.rs`.

The union is assigned ids in sorted-name order, so an id names the same
handler in every feature configuration; handlers that only exist under a
feature (`simd`, `threads`, `aot-load`, ...) are emitted into cfg-gated
sub-tables carrying the cfg detected at their definition site.

Output: `crates/telomere/src/common/op_registry.rs` (committed; regenerate
after adding or renaming a handler). A handler with no registry entry makes
serialization fail closed with `PrecompiledError::UnregisteredOp`, and the
wast round-trip CI lane catches a missing entry over the spec corpus.

Each entry also carries the handler's operand shape (#301), derived from
the handler's own dispatch advance by the ShapeDeriver below; the codec
validates decoded lowered functions against it. A handler whose shape
cannot be derived makes the generator fail, so the table cannot silently
go incomplete.

Usage: python3 tools/gen-op-registry.py [--check]
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VM_DIR = ROOT / "crates" / "telomere" / "src" / "runtime" / "vm"
VM_RS = ROOT / "crates" / "telomere" / "src" / "runtime" / "vm.rs"
OPS_RS = ROOT / "crates" / "telomere" / "src" / "runtime" / "jit" / "backend" / "ops.rs"
OUT = ROOT / "crates" / "telomere" / "src" / "common" / "op_registry.rs"

HANDLER_NAME = re.compile(r"^(?:op|special)_[a-z0-9_]+$")
FN_DEF = re.compile(
    r"pub(?:\([^)]*\))?\s+(?:unsafe\s+)?fn\s+([a-z_][a-z0-9_]*)\s*\(([^;{}]*)\)\s*->\s*VMResult<\(\)>",
    re.DOTALL,
)
CFG_ATTR = re.compile(r"#\[cfg\(([^]]*)\)\]")


def preceding_cfg(text: str, pos: int) -> str:
    """Return the cfg(...) condition of an attribute directly above `pos`."""
    head = text[:pos].rstrip()
    # Walk back over attribute lines and doc comments.
    lines = head.split("\n")
    cfg = ""
    for line in reversed(lines[-6:]):
        stripped = line.strip()
        m = CFG_ATTR.fullmatch(stripped)
        if m:
            cfg = m.group(1).strip()
            continue
        if stripped.startswith("#[") or stripped.startswith("///") or stripped.startswith("//"):
            continue
        break
    return cfg


def balanced(text: str, start: int) -> tuple[str, int]:
    """Extract the balanced (), {} or [] group starting at `start`."""
    pairs = {"(": ")", "{": "}", "[": "]"}
    open_ch = text[start]
    close_ch = pairs[open_ch]
    depth = 0
    i = start
    while i < len(text):
        ch = text[i]
        if ch == open_ch:
            depth += 1
        elif ch == close_ch:
            depth -= 1
            if depth == 0:
                return text[start + 1 : i], i + 1
        i += 1
    raise ValueError("unbalanced group")


def collect_from_vm_file(path: Path, names: dict[str, dict]) -> None:
    text = path.read_text()
    rel = path.name

    def record(name: str, cfg: str, origin: str, strict_name: bool = True) -> None:
        # Macro-argument idents are noisy, so they must carry the
        # conventional op_/special_ prefix. Names arriving from a real
        # definition with the exact handler signature, from OP_SPECS, or
        # from the diagnostic label table are accepted as-is: some
        # handlers (e.g. simd's `v128_const`, `i8x16_shuffle`) have no
        # prefix.
        if strict_name and not HANDLER_NAME.match(name):
            return
        if not re.fullmatch(r"[a-z_][a-z0-9_]*", name):
            return
        entry = names.setdefault(name, {"cfg": "", "module": "", "origins": set()})
        entry["origins"].add(origin)
        if rel == "simd.rs":
            entry["module"] = "simd"
            if not entry["cfg"]:
                entry["cfg"] = 'feature = "simd"'
        elif rel == "atomics.rs" and not entry["cfg"]:
            entry["cfg"] = 'feature = "threads"'
        if cfg and not entry["cfg"]:
            entry["cfg"] = cfg

    # 1. Direct fn definitions with the exact Op signature.
    for m in FN_DEF.finditer(text):
        name, params = m.group(1), m.group(2)
        # The Op signature has exactly two parameters; helpers like
        # `call_next` (extra offset parameter) only resemble it.
        parts = [p.strip() for p in params.split(",") if p.strip()]
        if len(parts) != 2:
            continue
        if "*const Instr" not in parts[0] or "&mut ExecuteContext" not in parts[1]:
            continue
        # Generic handlers (e.g. jit_call_direct<const bool>) cannot be
        # named as a bare Op; they are JIT-internal and never materialized
        # by the parser, so they stay out of the registry on purpose.
        record(name, preceding_cfg(text, m.start()), f"def:{rel}", strict_name=False)

    # 2. Handler-generating macros: macro_rules bodies containing `fn $`.
    gen_macros = set()
    for m in re.finditer(r"macro_rules!\s+([a-z_][a-z0-9_]*)", text):
        i = m.end()
        while i < len(text) and text[i] not in "({[":
            i += 1
        if i >= len(text):
            continue
        body, _ = balanced(text, i)
        if re.search(r"fn\s+\$", body):
            gen_macros.add(m.group(1))
    for macro in gen_macros:
        for m in re.finditer(re.escape(macro) + r"\s*!\s*", text):
            i = m.end()
            if i >= len(text) or text[i] not in "({[":
                continue
            args, _ = balanced(text, i)
            cfg = preceding_cfg(text, m.start())
            for ident in re.findall(r"[a-z_][a-z0-9_]*", args):
                record(ident, cfg, f"macro:{macro}@{rel}")


PARSER_DIR = ROOT / "crates" / "telomere" / "src" / "parser"
SRC_DIR = ROOT / "crates" / "telomere" / "src"


def collect_from_parser(names: dict[str, dict]) -> None:
    """Collect handlers referenced from the parser/optimizer sources.

    Some handlers are generated by proc macros (the SIMD operation
    family in `runtime/vm/simd.rs` is built by
    `telomere_macros::define_simd_operation`), so their names never
    appear textually at the definition site. The parser is the ground
    truth for what can be materialized: it names handlers directly
    (`vm::simd::i32x4_all_true`) or passes their names to its own
    instruction macros (`unary_op_simd_parser!(i32x4_add, 174)` whose
    body pushes `vm::simd::$name`).
    """

    # Names that appear as `vm::NAME` in sources but are not handlers:
    # dispatch/entry helpers, linear-memory accessors, and the generic
    # JIT call helpers (const-generic; cannot be named as a bare Op).
    not_handlers = {
        # vm submodule names (also mentioned in doc comments).
        "atomics", "bulk_memory", "call", "control", "globals", "locals",
        "memory", "numeric", "refs", "simd", "superinstructions", "tables",
        "call_next", "charge_n", "checkpoint", "checkpoint_fields",
        "compute_memory_offset", "core_state_benchmark_crc", "crc16_masked",
        "crc16_update16_bits", "diagnostic_op_label", "get_global",
        "i32_numeric_token_state_transition_value", "instr_index_from_base",
        "jit_call_direct", "jit_call_direct_wasm_fast", "jit_call_indirect",
        "list_crc_summary_value", "read_u16_linear", "read_u32_linear",
        "run_module_function", "run_module_function_result",
        "run_module_function_sync_with_runtime",
        "run_module_function_with_driver",
        "run_module_function_with_driver_result", "wasm_entry_pc",
        "write_u16_linear", "write_u32_linear",
    }

    def record(name: str, module: str, cfg: str, origin: str) -> None:
        if not re.fullmatch(r"[a-z_][a-z0-9_]*", name):
            return
        if name in not_handlers:
            return
        entry = names.setdefault(name, {"cfg": "", "module": "", "origins": set()})
        entry["origins"].add(origin)
        if module and not entry["module"]:
            entry["module"] = module
        if cfg and not entry["cfg"]:
            entry["cfg"] = cfg

    # Direct `vm::...` references anywhere in the crate sources. The
    # trailing lookahead keeps submodule paths (`vm::call::...`) out.
    for path in sorted(SRC_DIR.rglob("*.rs")):
        text = path.read_text()
        for m in re.finditer(r"vm::simd::([a-z_][a-z0-9_]*)(?![\w:])", text):
            record(m.group(1), "simd", 'feature = "simd"', "parser-ref")
        for m in re.finditer(r"vm::(?!simd::)([a-z_][a-z0-9_]*)(?![\w:])", text):
            record(m.group(1), "", "", "parser-ref")

    # Parser instruction macros whose bodies push `vm::$name` or
    # `vm::simd::$name`: the first invocation argument is the handler.
    for path in sorted(PARSER_DIR.rglob("*.rs")):
        text = path.read_text()
        gen_macros: dict[str, str] = {}
        for m in re.finditer(r"macro_rules!\s+([a-z_][a-z0-9_]*)", text):
            i = m.end()
            while i < len(text) and text[i] not in "({[":
                i += 1
            if i >= len(text):
                continue
            body, _ = balanced(text, i)
            mm = re.search(r"vm::(simd::)?\$", body)
            if mm:
                gen_macros[m.group(1)] = "simd" if mm.group(1) else ""
        for macro, module in gen_macros.items():
            for m in re.finditer(re.escape(macro) + r"\s*!\s*", text):
                i = m.end()
                if i >= len(text) or text[i] not in "({[":
                    continue
                args, _ = balanced(text, i)
                # These macros take handler names as bare ident
                # arguments (sometimes several: base/shared/indexed
                # variants) and numbers as the remaining arguments, so
                # every bare-ident argument is a handler name.
                for arg in args.split(","):
                    ident = re.fullmatch(r"\s*([a-z_][a-z0-9_]*)\s*", arg)
                    if ident:
                        cfg = 'feature = "simd"' if module == "simd" else ""
                        record(ident.group(1), module, cfg, f"parser-macro:{macro}")


# ---------------------------------------------------------------------------
# Operand-shape derivation (#301)
#
# Every registered handler also carries an operand shape used by the
# precompiled codec to validate decoded lowered functions. The shape is
# derived here, from the handler's own implementation, never maintained
# by hand in the generated table:
#
# - A handler's operand count is its dispatch advance: the literal in
#   `call_next(tail_code, N, ctx)`, or, for handlers that compute their
#   continuation directly, the literal in `tail_code.offset(N)`.
# - `call_next(tail_code, if INDEXED { a } else { b }, ctx)` yields an
#   AnyOf(a, b) shape (the indexed addressing variant carries one extra
#   operand).
# - A non-literal advance (`skip`, `skip_slots`, an encoded count, ...)
#   means the operand count is data-dependent: Variadic(min), where min
#   is one past the largest fixed operand slot the handler reads.
# - Handler-generating macros are resolved per generated function: the
#   macro pattern's `$var` order maps invocation arguments to the
#   `fn $var` segments of the macro body, and a segment that delegates
#   to a `$impl` argument inherits that function's shape.
# - `pub(crate) use X as Y` aliases inherit X's shape.
# - SIMD handlers generated by the `define_simd_operation` proc macro
#   delegate to stack helpers whose advance is 0; the pure stack SIMD
#   family (lane-typed names) is Exact(0). SIMD handlers with operands
#   (loads, stores, const, shuffle, ...) have their own definitions or
#   generating macros in simd.rs and are derived through those.
# - When a body contains several distinct literal advances, the smaller
#   ones are sub-dispatches into the handler's own operand stream (a
#   helper operating on a suffix, or a fallthrough into an embedded
#   sequence); the operand count is the largest advance, since a
#   smaller count would make that dispatch skip into the next op.
#
# A handler whose shape cannot be derived and is not listed in
# SHAPE_OVERRIDES below makes the generator fail: the registry (and
# therefore the codec's validation table) is incomplete by construction
# until the derivation or the override list is extended.

# Handlers whose operand count cannot be read off a dispatch advance
# because they never tail-dispatch (terminal traps, frame-entry
# trampolines, and stubs). Each entry states the ground truth the
# override rests on; the wasm instruction's shape, where one exists, is
# the same one its dispatching sibling implementation validates.
SHAPE_OVERRIDES: dict[str, tuple] = {
    # The dispatch trampoline itself: it forwards the instruction at
    # its operand pointer and consumes no operands of its own.
    "call_code": ("exact", 0),
    # Terminal trap: `_tail_code` is unused and it never dispatches.
    "op_unreachable": ("exact", 0),
    # Frame-entry trampolines: `_tail_code` is unused; they are
    # installed in place of an op, not lowered from a wasm instruction
    # with operands.
    "special_function_vm_end": ("exact", 0),
    "special_start_jit_function_call": ("exact", 0),
    "special_start_function_call": ("exact", 0),
    # The unshared-memory wait stubs return InvalidOperand without
    # dispatching, but they decode the same instruction as the shared
    # implementations, which advance past exactly one memarg operand
    # (`atomic_start` reads `(*tail_code).operand.memarg`).
    "op_memory_atomic_wait32": ("exact", 1),
    "op_memory_atomic_wait64": ("exact", 1),
    # `br_table`'s stream is a count operand followed by that many
    # target operands plus a default target; the count is encoded in
    # operand 0, so no literal advance exists to derive. The minimum
    # is the count operand plus the mandatory default target.
    "op_br_table": ("variadic", 2),
}

# Canonical control handlers whose operands are jump targets even
# though the target is read through a helper in some of them.
CANONICAL_BRANCH_OPS = {"op_br", "op_br_if", "op_else", "op_if", "op_br_table", "op_return"}

_CN_LIT = re.compile(r"call_next\(\s*tail_code\s*,\s*(\d+)\s*,\s*ctx\s*\)")
_CN_ANYOF = re.compile(
    r"call_next\(\s*tail_code\s*,\s*if\s+INDEXED\s*\{\s*(\d+)\s*\}\s*else\s*\{\s*(\d+)\s*\}\s*,\s*ctx\s*\)"
)
_CN_ANY = re.compile(r"call_next\(\s*tail_code\s*,\s*([^,]+?)\s*,\s*ctx\s*\)")
_T_OFF = re.compile(r"tail_code\.offset\(\s*(\d+)\s*\)")
_T_ADD = re.compile(r"tail_code\.add\(\s*(\d+)\s*\)")
_FN_DEF_ANY = re.compile(
    r"(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?fn\s+([a-z_][a-z0-9_]*)\s*(?:<[^>]*>)?\s*\("
)
_USE_AS = re.compile(r"use\s+(?:[a-z_][a-z0-9_:]*::)?([a-z_][a-z0-9_]*)\s+as\s+([a-z_][a-z0-9_]*)\s*;")
_SIMD_STACK_NAME = re.compile(r"^(?:i|u|f)(?:8|16|32|64)x(?:2|4|8|16|32)_|^v128_")


def _fn_bodies(text: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for m in _FN_DEF_ANY.finditer(text):
        name = m.group(1)
        try:
            _, j = balanced(text, m.end() - 1)
        except ValueError:
            continue
        k = j
        while k < len(text) and text[k] not in "{;":
            k += 1
        if k >= len(text) or text[k] == ";":
            continue
        try:
            body, _ = balanced(text, k)
        except ValueError:
            continue
        out.setdefault(name, body)
    return out


def _body_info(body: str) -> dict:
    # A rebinding of `tail_code` (`let tail_code = ...` or a match
    # binding like `Some(tail_code) =>`) replaces the operand pointer
    # with a jump/continuation destination; `call_next` on the rebound
    # pointer is a jump, not an advance, so dispatch matches after the
    # first rebinding are not this handler's operand count.
    rebind = min(
        (i for i in (body.find("let tail_code ="), body.find("Some(tail_code)")) if i >= 0),
        default=-1,
    )
    if rebind >= 0:
        body = body[:rebind]
    lits = {int(x) for x in _CN_LIT.findall(body)}
    # Fused branch handlers that delegate to a layout-driven helper
    # declare their stream length as `fallthrough_advance: N` in the
    # layout struct they pass to it; that is their dispatch advance.
    lits |= {int(x) for x in re.findall(r"fallthrough_advance:\s*(\d+)", body)}
    anyof = {(int(a), int(b)) for a, b in _CN_ANYOF.findall(body)}
    var = False
    for m in _CN_ANY.finditer(body):
        expr = m.group(1).strip()
        if not re.fullmatch(r"\d+", expr) and not expr.startswith("if INDEXED"):
            var = True
    adds = [int(x) for x in _T_ADD.findall(body)]
    # `br_if_ptr(tail_code, target_offset, fallthrough_advance, cond,
    # ctx)` computes the continuation for a fused branch: the advance
    # is the handler's operand count and its target operand is a jump
    # target.
    lits |= {int(x) for x in re.findall(r"br_if_ptr\(\s*tail_code\s*,\s*\d+\s*,\s*(\d+)\s*,", body)}
    # `br_table_target_ptr(tail_code, count_offset, first_target_offset,
    # index, ctx)` (and the `_at` variant): the stream is a fixed prefix,
    # a count operand, and count+1 target operands, so the minimum is
    # first_target_offset + 1 and the count is data-dependent.
    table_mins = {
        int(x) + 1
        for x in re.findall(
            r"br_table_target_ptr(?:_at)?\(\s*tail_code\s*,\s*\d+\s*,\s*(\d+)\s*,", body
        )
    }
    return {
        "lits": lits,
        "anyof": anyof,
        "var": var,
        "max_add": max(adds) if adds else -1,
        "jump": "jump_addr" in body or "br_if_ptr(" in body or bool(table_mins),
        "reads0": "(*tail_code).operand" in body,
        "table_min": min(table_mins) if table_mins else None,
    }


def _split_top_args(args: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    cur = ""
    for ch in args:
        if ch in "({[":
            depth += 1
        elif ch in ")}]":
            depth -= 1
        if ch == "," and depth == 0:
            parts.append(cur)
            cur = ""
        else:
            cur += ch
    if cur.strip():
        parts.append(cur)
    return [p.strip() for p in parts]


class ShapeDeriver:
    """Derives (shape, uses_jump_target) for every handler name."""

    def __init__(self) -> None:
        self.files = [VM_RS] + sorted(VM_DIR.glob("*.rs"))
        self.bodies: dict[str, str] = {}
        for path in self.files:
            for n, b in _fn_bodies(path.read_text()).items():
                self.bodies.setdefault(n, b)
        self.aliases: dict[str, str] = {}
        for path in self.files:
            for m in _USE_AS.finditer(path.read_text()):
                self.aliases[m.group(2)] = m.group(1)
        # Const-generic parameter names per fn, in declaration order,
        # so a call site `helper::<false, true>(tail_code, ...)` can be
        # matched against the helper's `if INDEXED {..} else {..}`
        # advance.
        self.const_params: dict[str, list[str]] = {}
        for path in self.files:
            text = path.read_text()
            for m in re.finditer(r"fn\s+([a-z_][a-z0-9_]*)\s*<([^>]*)>", text):
                params = re.findall(r"const\s+([A-Z_][A-Z0-9_]*)\s*:", m.group(2))
                if params:
                    self.const_params.setdefault(m.group(1), params)
        self.macro_shapes: dict[str, tuple] = {}
        self.macro_shapes = self._macro_shapes()

    def _indexed_pick(self, name: str, indexed: bool, depth: int = 0):
        """Exact shape of a generic helper instantiated with a known
        INDEXED value, when its only dispatch signal is the
        `if INDEXED {a} else {b}` advance."""
        body = self.bodies.get(name)
        if body is None or depth > 4:
            return None
        info = _body_info(body)
        if len(info["anyof"]) == 1 and not info["lits"] and not info["var"]:
            if_val, else_val = next(iter(info["anyof"]))
            return (("exact", if_val if indexed else else_val), info["jump"])
        # The helper may forward its own INDEXED parameter positionally
        # to the helper that owns the conditional advance
        # (`load_lane_internal::<1, SHARED, INDEXED>(tail_code, ...)`).
        own_params = self.const_params.get(name, [])
        if "INDEXED" not in own_params:
            return None
        own_pos = own_params.index("INDEXED")
        for callee in self._callees(body):
            m = re.search(re.escape(callee) + r"\s*::<([^>]*)>\s*\(\s*tail_code", body)
            if not m:
                continue
            args = [a.strip() for a in m.group(1).split(",")]
            callee_params = self.const_params.get(callee, [])
            if "INDEXED" not in callee_params:
                continue
            callee_pos = callee_params.index("INDEXED")
            if callee_pos < len(args) and args[callee_pos] == "INDEXED":
                return self._indexed_pick(callee, indexed, depth + 1)
            if callee_pos < len(args) and args[callee_pos] in ("true", "false"):
                return self._indexed_pick(callee, args[callee_pos] == "true", depth + 1)
        return None

    def _const_call_pick(self, body: str, callee: str):
        """If `body` calls `callee::<...>(tail_code, ...)` with a bool
        literal in the INDEXED position, return the picked shape."""
        m = re.search(re.escape(callee) + r"\s*::<([^>]*)>\s*\(\s*tail_code", body)
        if not m:
            return None
        params = self.const_params.get(callee, [])
        if "INDEXED" not in params:
            return None
        args = [a.strip() for a in m.group(1).split(",")]
        pos = params.index("INDEXED")
        if pos >= len(args) or args[pos] not in ("true", "false"):
            return None
        return self._indexed_pick(callee, args[pos] == "true")

    def _callees(self, body: str) -> list[str]:
        out: list[str] = []
        for m in re.finditer(r"([a-z_][a-z0-9_]*)\s*(?:::|<|\()", body):
            n = m.group(1)
            # Dispatch primitives are not sub-handlers: absorbing the
            # trampoline's shape would poison every handler that jumps.
            if n in ("call_code", "call_next"):
                continue
            if n in self.bodies and n not in out:
                out.append(n)
        return out

    def resolve(self, name: str, depth: int = 0, seen: frozenset = frozenset()):
        """Return ((kind, ...), jump) or None when underivable."""
        if name in SHAPE_OVERRIDES:
            return (SHAPE_OVERRIDES[name], name in CANONICAL_BRANCH_OPS)
        if name in seen or depth > 6:
            return None
        seen = seen | {name}
        if name in self.aliases:
            return self.resolve(self.aliases[name], depth + 1, seen)
        body = self.bodies.get(name)
        if body is not None:
            info = _body_info(body)
            jump = info["jump"] or name in CANONICAL_BRANCH_OPS
            if not jump:
                # A handler whose jump target is read by a helper it
                # delegates to still carries JumpTarget operands.
                for callee in self._callees(body):
                    r = self.resolve(callee, depth + 1, seen)
                    if r is not None and r[1]:
                        jump = True
                        break
            lits = set(info["lits"])
            anyof = set(info["anyof"])
            var = info["var"]
            # The handler's own dispatch signals come first: a literal
            # advance or a continuation offset in its own body is the
            # count, regardless of what its helpers do on suffixes.
            if len(lits) == 1 and not anyof and not var:
                return (("exact", next(iter(lits))), jump)
            if anyof and not lits and not var:
                a, b = next(iter(anyof))
                return (("anyof", min(a, b), max(a, b)), jump)
            if var:
                minimum = info["max_add"] + 1 if info["max_add"] >= 0 else (1 if info["reads0"] else 0)
                return (("variadic", minimum), jump)
            if info["table_min"] is not None and not lits and not anyof:
                return (("variadic", info["table_min"]), jump)
            if len(lits) > 1:
                # Smaller advances are sub-dispatches into this op's
                # own operand stream; the count is the largest advance.
                return (("exact", max(lits)), jump)
            offs = [int(x) for x in _T_OFF.findall(body)]
            if offs:
                return (("exact", max(offs)), jump)
            # No own advance: the handler delegates its whole stream to
            # a helper (wrapper fns, macro-generated forwarders).
            for callee in self._callees(body):
                r = self._const_call_pick(body, callee) or self.resolve(callee, depth + 1, seen)
                if r is None:
                    continue
                shape, j = r
                jump = jump or j
                if shape[0] == "exact":
                    lits.add(shape[1])
                elif shape[0] == "anyof":
                    anyof.add((shape[1], shape[2]))
                elif shape[0] == "variadic":
                    var = True
            if len(lits) == 1 and not anyof and not var:
                return (("exact", next(iter(lits))), jump)
            if anyof and not lits and not var:
                a, b = next(iter(anyof))
                return (("anyof", min(a, b), max(a, b)), jump)
            if var:
                minimum = info["max_add"] + 1 if info["max_add"] >= 0 else (1 if info["reads0"] else 0)
                return (("variadic", minimum), jump)
            if len(lits) > 1:
                return (("exact", max(lits)), jump)
            if info["max_add"] >= 0:
                return (("exact", info["max_add"] + 1), jump)
            if info["reads0"]:
                # A terminal/jump handler that reads exactly the
                # operand at its pointer and never advances past a
                # wider stream (e.g. `op_br` reads its jump target).
                return (("exact", 1), jump)
            return None
        if name in self.macro_shapes:
            return self.macro_shapes[name]
        if _SIMD_STACK_NAME.match(name):
            # Pure stack SIMD ops generated by the define_simd_operation
            # proc macro: they delegate to helpers advancing by 0.
            return (("exact", 0), False)
        return None

    def _segment_shape(self, seg: str, argmap: dict[str, str]):
        info = _body_info(seg)
        jump = info["jump"]
        lits = set(info["lits"])
        anyof = set(info["anyof"])
        var = info["var"]

        def absorb(r) -> None:
            nonlocal jump, var
            if r is None:
                return
            shape, j = r
            jump = jump or j
            if shape[0] == "exact":
                lits.add(shape[1])
            elif shape[0] == "anyof":
                anyof.add((shape[1], shape[2]))
            elif shape[0] == "variadic":
                var = True

        if not (len(lits) == 1 and not anyof and not var):
            for var_name in re.findall(r"\$([a-z_][a-z0-9_]*)\s*(?:::|<|\()", seg):
                target = argmap.get(var_name)
                if target and re.fullmatch(r"[a-z_][a-z0-9_]*", target):
                    picked = None
                    m = re.search(
                        r"\$" + re.escape(var_name) + r"\s*::<([^>]*)>\s*\(\s*tail_code", seg
                    )
                    if m:
                        params = self.const_params.get(target, [])
                        if "INDEXED" in params:
                            args = [a.strip() for a in m.group(1).split(",")]
                            pos = params.index("INDEXED")
                            if pos < len(args) and args[pos] in ("true", "false"):
                                picked = self._indexed_pick(target, args[pos] == "true")
                    absorb(picked or self.resolve(target))
            for callee in self._callees(seg):
                absorb(self._const_call_pick(seg, callee) or self.resolve(callee))
        if len(lits) == 1 and not anyof and not var:
            return (("exact", next(iter(lits))), jump)
        if anyof and not lits and not var:
            a, b = next(iter(anyof))
            return (("anyof", min(a, b), max(a, b)), jump)
        if var:
            minimum = info["max_add"] + 1 if info["max_add"] >= 0 else (1 if info["reads0"] else 0)
            return (("variadic", minimum), jump)
        if info["table_min"] is not None and not lits and not anyof:
            return (("variadic", info["table_min"]), jump)
        if len(lits) > 1:
            return (("exact", max(lits)), jump)
        offs = [int(x) for x in _T_OFF.findall(seg)]
        if offs:
            return (("exact", max(offs)), jump)
        if info["max_add"] >= 0:
            return (("exact", info["max_add"] + 1), jump)
        return None

    def _macro_shapes(self) -> dict[str, tuple]:
        # Parse generating macros: arms of (pattern $vars, body).
        macro_arms: dict[str, list[tuple[list[str], str]]] = {}
        for path in self.files:
            text = path.read_text()
            for m in re.finditer(r"macro_rules!\s+([a-z_][a-z0-9_]*)", text):
                i = m.end()
                while i < len(text) and text[i] not in "({[":
                    i += 1
                if i >= len(text):
                    continue
                whole, _ = balanced(text, i)
                if "fn $" not in whole:
                    continue
                arms: list[tuple[list[str], str]] = []
                j = 0
                while j < len(whole):
                    while j < len(whole) and whole[j] not in "({[":
                        j += 1
                    if j >= len(whole):
                        break
                    try:
                        pat, j2 = balanced(whole, j)
                    except ValueError:
                        break
                    k = j2
                    while k < len(whole) and not (
                        whole[k] == "=" and k + 1 < len(whole) and whole[k + 1] == ">"
                    ):
                        if whole[k] in "({[":
                            break
                        k += 1
                    if k >= len(whole) or whole[k] != "=":
                        j = j2
                        continue
                    k += 2
                    while k < len(whole) and whole[k] not in "({[":
                        k += 1
                    if k >= len(whole):
                        break
                    try:
                        body, j = balanced(whole, k)
                    except ValueError:
                        break
                    arms.append((re.findall(r"\$([a-z_][a-z0-9_]*)\s*:", pat), body))
                if arms:
                    macro_arms.setdefault(m.group(1), arms)
        shapes: dict[str, tuple] = {}
        for path in self.files:
            text = path.read_text()
            for macro, arms in macro_arms.items():
                for m in re.finditer(re.escape(macro) + r"\s*!\s*", text):
                    i = m.end()
                    if i >= len(text) or text[i] not in "({[":
                        continue
                    try:
                        args, _ = balanced(text, i)
                    except ValueError:
                        continue
                    arglist = _split_top_args(args)
                    for vars_, body in arms:
                        if len(vars_) > len(arglist):
                            continue
                        argmap = {
                            v: arglist[idx] for idx, v in enumerate(vars_) if idx < len(arglist)
                        }
                        fnvars = [
                            (mm.start(), mm.group(1))
                            for mm in re.finditer(r"fn\s+\$([a-z_][a-z0-9_]*)", body)
                        ]
                        for si, (pos, var) in enumerate(fnvars):
                            end = fnvars[si + 1][0] if si + 1 < len(fnvars) else len(body)
                            hname = argmap.get(var)
                            if hname and re.fullmatch(r"[a-z_][a-z0-9_]*", hname):
                                r = self._segment_shape(body[pos:end], argmap)
                                if r is not None:
                                    shapes.setdefault(hname, r)
                        break
        return shapes


def derive_shapes(names: dict) -> dict[str, tuple]:
    """Derive the operand shape of every collected handler, or fail."""
    deriver = ShapeDeriver()
    shapes: dict[str, tuple] = {}
    missing: list[str] = []
    for name in names:
        r = deriver.resolve(name)
        if r is None:
            missing.append(name)
        else:
            shapes[name] = r
    if missing:
        print(
            "operand shape underivable for "
            f"{len(missing)} handler(s): {', '.join(sorted(missing))}\n"
            "extend the derivation in tools/gen-op-registry.py or add a "
            "justified SHAPE_OVERRIDES entry; the registry is incomplete "
            "until every handler has a shape",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return shapes


def main() -> int:
    names: dict[str, dict] = {}
    collect_from_vm_file(VM_RS, names)
    for path in sorted(VM_DIR.glob("*.rs")):
        collect_from_vm_file(path, names)
    collect_from_parser(names)

    # 3. OP_SPECS entries (JIT baseline decoder coverage).
    ops_text = OPS_RS.read_text()
    simd_imports = set()
    for m in re.finditer(r"use crate::runtime::vm::simd::\{([^}]*)\}", ops_text, re.DOTALL):
        simd_imports.update(re.findall(r"[a-z_][a-z0-9_]*", m.group(1)))
    for m in re.finditer(r"op:\s*([a-z_][a-z0-9_]*)\s+as\s+Op", ops_text):
        name = m.group(1)
        entry = names.setdefault(name, {"cfg": "", "module": "", "origins": set()})
        entry["origins"].add("OP_SPECS")
        if name in simd_imports:
            entry["module"] = "simd"
            if not entry["cfg"]:
                entry["cfg"] = 'feature = "simd"'

    # 4. diagnostic_op_label entries in vm.rs.
    vm_text = VM_RS.read_text()
    for m in re.finditer(r"label!\(\s*([a-z_][a-z0-9_]*)\s*,", vm_text):
        name = m.group(1)
        entry = names.setdefault(name, {"cfg": "", "module": "", "origins": set()})
        entry["origins"].add("diagnostic_op_label")
        cfg = preceding_cfg(vm_text, m.start())
        if cfg and not entry["cfg"]:
            entry["cfg"] = cfg

    # Manual cfg overrides for handlers whose cfg gate the scanner cannot
    # see (gated by a block-level attribute in the defining file).
    cfg_overrides = {
        "special_start_jit_function_call": 'feature = "aot-load"',
        # `pub(crate) use` aliases inside the threads-gated atomics module;
        # the scanner sees them only via OP_SPECS, without the gate.
        "op_memory_atomic_notify_unshared": 'feature = "threads"',
        "op_memory_atomic_wait32_unshared": 'feature = "threads"',
        "op_memory_atomic_wait64_unshared": 'feature = "threads"',
        # The `*_local` aliases from the same block (parser references
        # reach them without the gate visible to the scanner).
        "op_atomic_fence_local": 'feature = "threads"',
        "op_i32_atomic_load_local": 'feature = "threads"',
        "op_i32_atomic_load16_u_local": 'feature = "threads"',
        "op_i32_atomic_load8_u_local": 'feature = "threads"',
        "op_i32_atomic_store_local": 'feature = "threads"',
        "op_i32_atomic_store16_local": 'feature = "threads"',
        "op_i32_atomic_store8_local": 'feature = "threads"',
        "op_i64_atomic_load_local": 'feature = "threads"',
        "op_i64_atomic_load16_u_local": 'feature = "threads"',
        "op_i64_atomic_load32_u_local": 'feature = "threads"',
        "op_i64_atomic_load8_u_local": 'feature = "threads"',
        "op_i64_atomic_store_local": 'feature = "threads"',
        "op_i64_atomic_store16_local": 'feature = "threads"',
        "op_i64_atomic_store32_local": 'feature = "threads"',
        "op_i64_atomic_store8_local": 'feature = "threads"',
    }
    for name, cfg in cfg_overrides.items():
        if name in names:
            names[name]["cfg"] = cfg

    # `*_impl` idents collected from macro arguments are generic helper
    # functions the generated handlers delegate to; they are never
    # materialized as ops themselves. (A real definition with the exact Op
    # signature would arrive via a `def:` origin and is kept.)
    names = {
        name: entry
        for name, entry in names.items()
        if not (
            name.endswith("_impl")
            and not any(o.startswith("def:") for o in entry["origins"])
            and "OP_SPECS" not in entry["origins"]
        )
    }

    shapes = derive_shapes(names)

    ordered = sorted(names)
    ids = {name: idx for idx, name in enumerate(ordered)}
    if len(ordered) > 0x1_0000:
        print(f"registry overflow: {len(ordered)} handlers exceed u16 ids", file=sys.stderr)
        return 1

    groups: dict[str, list[str]] = {}
    for name in ordered:
        groups.setdefault(names[name]["cfg"], []).append(name)

    def static_name(cfg: str) -> str:
        if not cfg:
            return "OP_REGISTRY"
        slug = re.sub(r"[^a-z0-9]+", "_", cfg.lower()).strip("_").upper()
        return f"OP_REGISTRY_{slug}"

    out = []
    out.append("// @generated by tools/gen-op-registry.py -- do not edit by hand.")
    out.append("// Regenerate with: python3 tools/gen-op-registry.py")
    out.append("//")
    out.append("// Stable op-ID registry for the precompiled-module codec (#196).")
    out.append("// Ids are assigned in sorted-name order over every handler the")
    out.append("// generator can find, so an id names the same handler in every")
    out.append("// feature configuration; feature-gated handlers live in cfg-gated")
    out.append("// sub-tables. Serialization resolves an `Op` by fn-pointer")
    out.append("// identity against these tables and fails closed")
    out.append("// (`PrecompiledError::UnregisteredOp`) for a handler with no")
    out.append("// entry, so a new handler must be registered here -- by")
    out.append("// regenerating this file -- before modules using it can be")
    out.append("// serialized.")
    out.append("")
    out.append("use crate::common::Op;")
    out.append("")
    out.append("/// Operand shape of a handler in lowered form (#301), derived by")
    out.append("/// the registry generator from the handler's own dispatch advance")
    out.append("/// (`call_next(tail_code, N, ctx)` / continuation offsets in its")
    out.append("/// implementation) -- never maintained by hand. The precompiled")
    out.append("/// codec validates every decoded op against this table before")
    out.append("/// materialization.")
    out.append("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    out.append("// A variant may have no entry in some feature configurations")
    out.append("// (e.g. `AnyOf` exists only where an INDEXED handler variant is")
    out.append("// compiled in), so construction is cfg-dependent by design.")
    out.append("#[allow(dead_code)]")
    out.append("pub(crate) enum OpShape {")
    out.append("    /// Exactly this many operands.")
    out.append("    Exact(u16),")
    out.append("    /// One of two counts (the `INDEXED` addressing variant of a")
    out.append("    /// handler carries one extra operand).")
    out.append("    AnyOf(u16, u16),")
    out.append("    /// A data-dependent count (encoded in the operand stream")
    out.append("    /// itself, e.g. a skip/count operand or a branch table), at")
    out.append("    /// least this many operands.")
    out.append("    Variadic(u16),")
    out.append("}")
    out.append("")
    out.append("/// One registered interpreter handler and its stable codec id.")
    out.append("pub(crate) struct OpEntry {")
    out.append("    /// Stable id written into precompiled artifacts.")
    out.append("    pub id: u16,")
    out.append("    /// Handler name, for diagnostics and the table hash.")
    out.append("    pub name: &'static str,")
    out.append("    /// The handler itself.")
    out.append("    pub op: Op,")
    out.append("    /// Operand shape the codec enforces for this handler.")
    out.append("    pub shape: OpShape,")
    out.append("    /// Whether this handler interprets an operand as a jump")
    out.append("    /// target (derived from `jump_addr` operand reads in its")
    out.append("    /// implementation); only such handlers may carry")
    out.append("    /// `JumpTarget` operands in a decoded artifact.")
    out.append("    pub jump_target: bool,")
    out.append("}")
    out.append("")

    for cfg in sorted(groups, key=lambda c: (c != "", c)):
        table = static_name(cfg)
        if cfg:
            out.append(f'#[cfg({cfg})]')
        out.append(f"pub(crate) static {table}: &[OpEntry] = &[")
        for name in groups[cfg]:
            path = (
                f"crate::runtime::vm::simd::{name}"
                if names[name]["module"] == "simd"
                else f"crate::runtime::vm::{name}"
            )
            shape, jump = shapes[name]
            if shape[0] == "exact":
                shape_rs = f"OpShape::Exact({shape[1]})"
            elif shape[0] == "anyof":
                shape_rs = f"OpShape::AnyOf({shape[1]}, {shape[2]})"
            else:
                shape_rs = f"OpShape::Variadic({shape[1]})"
            out.append(
                f'    OpEntry {{ id: {ids[name]}, name: "{name}", op: {path} as Op, '
                f"shape: {shape_rs}, jump_target: {str(jump).lower()} }},"
            )
        out.append("];")
        out.append("")

    out.append("/// Calls `f` on every registry entry compiled into this build.")
    out.append("pub(crate) fn for_each_op_entry(mut f: impl FnMut(&OpEntry)) {")
    for cfg in sorted(groups, key=lambda c: (c != "", c)):
        table = static_name(cfg)
        if cfg:
            out.append(f"    #[cfg({cfg})]")
            out.append("    {")
            out.append(f"        for entry in {table} {{")
            out.append("            f(entry);")
            out.append("        }")
            out.append("    }")
            if cfg:
                pass
        else:
            out.append(f"    for entry in {table} {{")
            out.append("        f(entry);")
            out.append("    }")
    out.append("}")
    out.append("")
    out.append("const fn hash_op_table(mut hash: u64, table: &[OpEntry]) -> u64 {")
    out.append("    let mut i = 0;")
    out.append("    while i < table.len() {")
    out.append("        let name = table[i].name.as_bytes();")
    out.append("        let mut j = 0;")
    out.append("        while j < name.len() {")
    out.append("            hash ^= name[j] as u64;")
    out.append("            hash = hash.wrapping_mul(0x100_0000_01b3);")
    out.append("            j += 1;")
    out.append("        }")
    out.append("        hash ^= table[i].id as u64;")
    out.append("        hash = hash.wrapping_mul(0x100_0000_01b3);")
    out.append("        let (tag, a, b) = match table[i].shape {")
    out.append("            OpShape::Exact(n) => (1u64, n as u64, 0u64),")
    out.append("            OpShape::AnyOf(x, y) => (2u64, x as u64, y as u64),")
    out.append("            OpShape::Variadic(n) => (3u64, n as u64, 0u64),")
    out.append("        };")
    out.append("        hash ^= tag;")
    out.append("        hash = hash.wrapping_mul(0x100_0000_01b3);")
    out.append("        hash ^= a;")
    out.append("        hash = hash.wrapping_mul(0x100_0000_01b3);")
    out.append("        hash ^= b;")
    out.append("        hash = hash.wrapping_mul(0x100_0000_01b3);")
    out.append("        hash ^= table[i].jump_target as u64;")
    out.append("        hash = hash.wrapping_mul(0x100_0000_01b3);")
    out.append("        i += 1;")
    out.append("    }")
    out.append("    hash")
    out.append("}")
    out.append("")
    out.append("/// FNV-1a hash of the (id, name) pairs compiled into this build.")
    out.append("/// Part of the precompiled artifact fingerprint: two builds whose")
    out.append("/// registries differ in any entry hash differently.")
    out.append("pub(crate) const OP_TABLE_HASH: u64 = {")
    out.append("    let mut hash = 0xcbf2_9ce4_8422_2325;")
    for cfg in sorted(groups, key=lambda c: (c != "", c)):
        table = static_name(cfg)
        if cfg:
            out.append(f"    #[cfg({cfg})]")
            out.append("    {")
            out.append(f"        hash = hash_op_table(hash, {table});")
            out.append("    }")
        else:
            out.append(f"    hash = hash_op_table(hash, {table});")
    out.append("    hash")
    out.append("};")
    out.append("")
    rendered = "\n".join(out)

    # Canonicalize through rustfmt (the workspace's edition) so the
    # committed file is both generator-fresh and `cargo fmt`-clean; the
    # one-line entries this script emits are not what rustfmt produces
    # for the cfg-gated tables. Without rustfmt on PATH, fall back to
    # the raw rendering.
    try:
        proc = subprocess.run(
            ["rustfmt", "--edition", "2024"],
            input=rendered,
            capture_output=True,
            text=True,
            check=True,
        )
        rendered = proc.stdout
    except (OSError, subprocess.CalledProcessError):
        pass

    if "--check" in sys.argv:
        current = OUT.read_text() if OUT.exists() else ""
        if current != rendered:
            print(f"{OUT} is stale; regenerate with tools/gen-op-registry.py", file=sys.stderr)
            return 1
        return 0

    OUT.write_text(rendered)
    print(f"wrote {OUT}: {len(ordered)} handlers in {len(groups)} cfg group(s)")
    for cfg in sorted(groups, key=lambda c: (c != "", c)):
        print(f"  cfg({cfg or 'always'}): {len(groups[cfg])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
