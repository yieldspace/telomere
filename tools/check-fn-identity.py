#!/usr/bin/env python3
"""Check that every fn-pointer-identity-compared handler is `#[inline(never)]`.

Interpreter handlers are recognised by function-pointer identity: the
optimizer pipeline, the instantiator, and the JIT baseline decoder compare
an `Instr`'s stored `Op` against handler items with
`std::ptr::fn_addr_eq` (directly, through an `op_eq` helper, through the
`OP_SPECS` table in the JIT baseline decoder, or through the `label!`
matcher in vm diagnostics). Rust does not guarantee that a function item
has a single address in a linked binary: under the release profile
(ThinLTO), an internal-linkage handler can be imported into several LTO
modules and internalised in each, after which `fn_addr_eq` returns `false`
for two references to the same handler. This silently broke trap pc
attribution for `op_unreachable` in #209 and was measured again for
`special_function_vm_end` in #240 (two local symbols with one mangled
hash in 20 of 28 release test binaries).

The defence is that every identity-compared handler carries
`#[inline(never)]`, which forces a single global emission (#209 verified
the symbol change with `nm`). This checker therefore fails when a handler
that is compared by identity resolves to a definition — a plain function,
a `macro_rules!` template product, an alias re-export target, or a
`telomere_macros::define_simd_operation` product — whose definition does
not carry the attribute.

The reader is intentionally narrow and stdlib-only, in the style of
tools/check-jit-test-coverage.py: it scans `crates/*/src/**/*.rs` only,
recognises the comparison spellings listed above, resolves aliases of the
form `use path::name as other;`, treats a `macro_rules!` template as
protected only when every Op-signature `fn $param` site in it carries the
attribute, and does not evaluate conditional-compilation variants beyond
requiring every definition site of a compared handler to be protected.
"""

import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent

NAME_AS_OP_RE = re.compile(
    r"(?<![$\w])([a-z_][a-z_0-9]*)\s+as\s+(?:[a-z_0-9]+::)*Op\b"
)
FN_ADDR_EQ_RE = re.compile(r"fn_addr_eq\s*\(")
OP_EQ_RE = re.compile(r"\.op_eq\s*\(")
LABEL_RE = re.compile(
    r"(?<!macro_rules!\s)label!\(\s*(?:[a-z_0-9]+::)*([a-z_][a-z_0-9]*)\s*,"
)
OP_SPEC_RE = re.compile(r"op:\s*(?:[a-z_0-9]+::)*([a-z_][a-z_0-9]*)\s+as\s+Op\b")
FN_DEF_RE = re.compile(r"\bfn ([a-z_][a-z_0-9]*)\s*(?:<[^>]*>)?\s*\(")
TEMPLATE_FN_RE = re.compile(r"\bfn \$([a-z_0-9]+)")
MACRO_RULES_RE = re.compile(r"macro_rules! ([a-z_0-9]+)")
ALIAS_RE = re.compile(
    r"\buse\s+(?:[a-z_0-9:]+::)?([a-z_][a-z_0-9]*)\s+as\s+([a-z_][a-z_0-9]*)\s*;"
)
SIMD_OP_MACROS = (
    "define_unary_simd_operation",
    "define_binary_simd_operation",
    "define_simd_cmp_operation",
)


def load_sources(root):
    """Return {relative path: text} for every Rust source under crates/*/src."""
    sources = {}
    crates_dir = root / "crates"
    if not crates_dir.is_dir():
        raise RuntimeError(f"repository has no crates directory: {root}")
    for path in sorted(crates_dir.glob("*/src/**/*.rs")):
        sources[path.relative_to(root).as_posix()] = path.read_text()
    return sources


def balanced_span(text, open_index):
    """Return the text inside the balanced bracket opened at open_index."""
    pairs = {"(": ")", "[": "]", "{": "}"}
    opener = text[open_index]
    closer = pairs[opener]
    depth = 0
    for i in range(open_index, len(text)):
        ch = text[i]
        if ch == opener:
            depth += 1
        elif ch == closer:
            depth -= 1
            if depth == 0:
                return text[open_index + 1 : i]
    return text[open_index + 1 :]


def extract_compared_handlers(sources):
    """Map each identity-compared handler name to the files comparing it."""
    compared = {}
    for rel, text in sources.items():
        names = set()
        for pattern in (FN_ADDR_EQ_RE, OP_EQ_RE):
            for match in pattern.finditer(text):
                span = balanced_span(text, match.end() - 1)
                names.update(NAME_AS_OP_RE.findall(span))
        names.update(LABEL_RE.findall(text))
        if rel.endswith("runtime/jit/backend/ops.rs"):
            start = text.find("const OP_SPECS")
            if start != -1:
                end = text.find("\n];", start)
                names.update(OP_SPEC_RE.findall(text[start:end]))
        for name in names:
            compared.setdefault(name, set()).add(rel)
    return compared


def attributes_above(lines, index):
    """Collect the attribute lines in the block directly above lines[index]."""
    attrs = []
    j = index - 1
    while j >= 0:
        stripped = lines[j].strip()
        if stripped.startswith("#["):
            attrs.append(stripped)
            j -= 1
        elif stripped.startswith("///") or stripped.startswith("//") or stripped == "":
            j -= 1
        else:
            break
    return attrs


def has_inline_never(lines, index):
    return any("inline(never)" in attr for attr in attributes_above(lines, index))


def index_plain_definitions(sources):
    """Map a function name to [(file, line, protected)] for plain definitions."""
    definitions = {}
    for rel, text in sources.items():
        lines = text.splitlines()
        for i, line in enumerate(lines):
            match = FN_DEF_RE.search(line)
            if match:
                definitions.setdefault(match.group(1), []).append(
                    (rel, i + 1, has_inline_never(lines, i))
                )
    return definitions


def index_macro_templates(sources):
    """Map a macro_rules name to [(file, line, protected)] Op-signature sites.

    A site is a `fn $param` whose signature (this line plus the next few)
    mentions `*const Instr` and returns `VMResult<()>` — the handler shape.
    """
    templates = {}
    for rel, text in sources.items():
        lines = text.splitlines()
        current_macro = None
        for i, line in enumerate(lines):
            macro_match = MACRO_RULES_RE.search(line)
            if macro_match:
                current_macro = macro_match.group(1)
            fn_match = TEMPLATE_FN_RE.search(line)
            if fn_match and current_macro:
                signature = "\n".join(lines[i : i + 4])
                if "*const Instr" in signature and "VMResult<()>" in signature:
                    templates.setdefault(current_macro, []).append(
                        (rel, i + 1, has_inline_never(lines, i))
                    )
    return templates


def index_macro_invocations(sources, macro_names):
    """Map a handler name to the template macros whose invocations mention it."""
    products = {}
    for rel, text in sources.items():
        for macro in macro_names:
            for match in re.finditer(rf"(?<!macro_rules!\s){re.escape(macro)}!\s*([\(\{{\[])", text):
                span = balanced_span(text, match.end() - 1)
                for name in re.findall(r"\b((?:op_|special_)[a-z_0-9]*)\b", span):
                    products.setdefault(name, set()).add(macro)
                for name in re.findall(r"\b([a-z][a-z_0-9]*x[0-9]+[a-z_0-9]*)\b", span):
                    products.setdefault(name, set()).add(macro)
    return products


def index_aliases(sources):
    aliases = {}
    for text in sources.values():
        for source, target in ALIAS_RE.findall(text):
            aliases[target] = source
    return aliases


def simd_proc_macro_products(sources):
    """Names generated by telomere_macros::define_simd_operation invocations.

    The proc macro emits one handler per (target type, operation) pair,
    named `{target}_{operation}`; the invocations live in runtime/vm/simd.rs
    behind the define_unary/binary/cmp_simd_operation macro_rules wrappers.
    """
    products = set()
    for rel, text in sources.items():
        if not rel.endswith("runtime/vm/simd.rs"):
            continue
        for macro in SIMD_OP_MACROS:
            for match in re.finditer(rf"{macro}!\s*\(", text):
                span = balanced_span(text, match.end() - 1)
                head = re.match(r"\s*([a-z_][a-z_0-9]*)\s*,\s*\[([^\]]*)\]", span)
                if head:
                    operation = head.group(1)
                    targets = re.findall(r"[a-z_][a-z_0-9]*", head.group(2))
                    for target in targets:
                        products.add(f"{target}_{operation}")
    return products


def proc_macro_emits_inline_never(sources):
    """Whether define_simd_operation's expansion annotates its handlers."""
    for rel, text in sources.items():
        if rel.endswith("telomere-macros/src/lib.rs"):
            marker = text.find("pub unsafe fn #mangled")
            if marker == -1:
                return False
            return "#[inline(never)]" in text[max(0, marker - 200) : marker]
    return False


def find_violations(sources):
    """Return a list of human-readable violations (empty when the tree is clean)."""
    compared = extract_compared_handlers(sources)
    definitions = index_plain_definitions(sources)
    templates = index_macro_templates(sources)
    invocations = index_macro_invocations(sources, templates.keys())
    aliases = index_aliases(sources)
    simd_products = simd_proc_macro_products(sources)
    simd_protected = proc_macro_emits_inline_never(sources)

    violations = []

    def resolve(name, seen):
        """Return a list of (description, protected) leaves for a handler name."""
        if name in seen:
            return []
        seen = seen | {name}
        if name in definitions:
            return [
                (f"{rel}:{line}", protected)
                for rel, line, protected in definitions[name]
            ]
        if name in aliases:
            return resolve(aliases[name], seen)
        leaves = []
        if name in invocations:
            for macro in sorted(invocations[name]):
                sites = templates.get(macro, [])
                for rel, line, protected in sites:
                    leaves.append((f"{rel}:{line} (macro {macro})", protected))
        if name in simd_products:
            leaves.append(("telomere_macros::define_simd_operation", simd_protected))
        return leaves

    for name in sorted(compared):
        leaves = resolve(name, set())
        if not leaves:
            violations.append(
                f"{name}: compared by identity in "
                f"{', '.join(sorted(compared[name]))} but its definition "
                "could not be resolved; if this is a new definition shape, "
                "teach tools/check-fn-identity.py about it"
            )
            continue
        for description, protected in leaves:
            if not protected:
                violations.append(
                    f"{name}: definition at {description} lacks "
                    "#[inline(never)] (compared by identity in "
                    f"{', '.join(sorted(compared[name]))})"
                )
    return violations


def main():
    sources = load_sources(REPO_ROOT)
    violations = find_violations(sources)
    if violations:
        print("fn-pointer identity check failed:")
        for violation in violations:
            print(f"  - {violation}")
        return 1
    compared = extract_compared_handlers(sources)
    print(
        f"fn-pointer identity check passed: {len(compared)} identity-compared "
        "handlers all resolve to #[inline(never)] definitions"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
