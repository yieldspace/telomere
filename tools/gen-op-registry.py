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
    out.append("/// One registered interpreter handler and its stable codec id.")
    out.append("pub(crate) struct OpEntry {")
    out.append("    /// Stable id written into precompiled artifacts.")
    out.append("    pub id: u16,")
    out.append("    /// Handler name, for diagnostics and the table hash.")
    out.append("    pub name: &'static str,")
    out.append("    /// The handler itself.")
    out.append("    pub op: Op,")
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
            out.append(
                f'    OpEntry {{ id: {ids[name]}, name: "{name}", op: {path} as Op }},'
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
