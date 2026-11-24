#!/usr/bin/env python3
"""Check that the interpreter dispatch path gains no new thread-locals.

#213 decided where per-execution state lives (docs/core/execution-state.md):
state read per dispatch belongs in `ExecuteContext`, and a thread-local is
permitted only for state that must be reachable from a boundary the context
does not cross. The decision's failure mode is incremental drift -- one new
thread-local at a time, each individually plausible -- and the cost of the
last drift is measured: reading the `JIT_INTERPRETER_STOP_AT` thread-local
on every dispatch slowed the interpreter by up to 2x whenever the `jit`
feature was merely compiled in (#190).

This checker is the mechanical half of the rule. It scans the files that
define or consume the dispatch loop and fails when a `thread_local!` block
declares a static that is not on the recorded allowlist. It is intentionally
narrow and stdlib-only, in the style of tools/check-fn-identity.py: it does
not parse Rust, it does not evaluate cfg variants, and it cannot tell
whether an allowed thread-local is *read* per dispatch -- that judgement
stays with review, backed by the classification table in the decision
document. What it does guarantee is that adding a thread-local to the
dispatch path is a deliberate, reviewed act that updates the allowlist and
the document together.
"""

import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent

# Files that define `ExecuteContext` or run the interpreter dispatch loop.
# A thread-local declared here is one edit away from being read per
# dispatch, so these are the files the #213 rule guards.
GUARDED_FILES = (
    "crates/telomere/src/common.rs",
    "crates/telomere/src/runtime/vm.rs",
    "crates/telomere/src/runtime/jit.rs",
)

# Recorded exceptions: (guarded file, static name) -> why the #213 rule
# permits it. Keep in sync with the classification in
# docs/core/execution-state.md.
ALLOWED = {
    (
        "crates/telomere/src/runtime/vm.rs",
        "DISPATCH_PROFILE_SESSION",
    ): "vm-profile session state; feature-gated profiling, not execution state",
    (
        "crates/telomere/src/runtime/vm.rs",
        "DISPATCH_PROFILE_TEST_ENABLED",
    ): "#[cfg(test)] companion of DISPATCH_PROFILE_SESSION",
    (
        "crates/telomere/src/runtime/vm.rs",
        "LAST_DISPATCH_PROFILE_SNAPSHOT",
    ): "#[cfg(test)] companion of DISPATCH_PROFILE_SESSION",
}

THREAD_LOCAL_RE = re.compile(r"thread_local!\s*\{")
STATIC_RE = re.compile(r"\bstatic\s+([A-Z][A-Z_0-9]*)\s*:")


def thread_local_statics(text):
    """Yield the static names declared by each `thread_local!` block."""
    names = []
    for match in THREAD_LOCAL_RE.finditer(text):
        depth = 1
        index = match.end()
        while index < len(text) and depth:
            char = text[index]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
            index += 1
        block = text[match.end():index]
        names.extend(STATIC_RE.findall(block))
    return names


def check_source(rel_path, text):
    """Return the list of violation messages for one guarded file."""
    violations = []
    for name in thread_local_statics(text):
        if (rel_path, name) not in ALLOWED:
            violations.append(
                f"{rel_path}: thread-local `static {name}` is not on the "
                "#213 allowlist; per-execution state belongs in "
                "ExecuteContext (docs/core/execution-state.md), and a new "
                "exception must be recorded there and in "
                "tools/check-execution-state.py together"
            )
    return violations


def main():
    violations = []
    for rel_path in GUARDED_FILES:
        path = REPO_ROOT / rel_path
        if not path.exists():
            violations.append(f"{rel_path}: guarded file is missing")
            continue
        violations.extend(check_source(rel_path, path.read_text()))
    if violations:
        for violation in violations:
            print(f"error: {violation}", file=sys.stderr)
        return 1
    print("execution-state check passed: no unrecorded dispatch-path thread-locals")
    return 0


if __name__ == "__main__":
    sys.exit(main())
