#!/usr/bin/env python3
"""Check that an `aot-load`-only embedder binary contains no emitter code.

Issue #200 splits the core crate's `aot-load` feature (cache / load path,
runtime stubs, native-call ABI, executable code memory) from the `jit`
feature (the emitter backends and the codegen crate's assemblers). A size
baseline can show the emitters did not *grow* the binary, but only a symbol
check shows they are *absent*: if emitter code ever leaks back into an
`aot-load` build -- a stray non-gated call path, a feature-unification
accident -- this check fails and names the leaked symbols.

The checker builds the minimal embedder's `embed-core` binary with
`simd,aot-load` (and not `jit`) into a dedicated target directory, lists
its symbols with `nm`, and asserts:

* no symbol mentions an emitter entry point (`emit_baseline_function`,
  `emit_baseline_plan`, `build_baseline_plan` -- the names survive Rust
  symbol mangling as substrings), and
* as a positive control, symbols for the load path (`CodeArena`,
  `StoreJitCache`) and for the interpreter (`instantiate`) are present, so
  an empty or wrong binary cannot pass vacuously.

The build uses the `dev` profile on purpose: under the release profiles
the single-caller emitter entry points are inlined by LTO until their
symbols vanish even from a `jit` build (verified against a control build),
which would make an absence check vacuous. The dev profile keeps a symbol
per function, so presence and absence are both meaningful. This checker is
stdlib-only, in the style of tools/check-embedder-size.py.
"""

import argparse
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_TARGET_DIR = REPO_ROOT / "target" / "aot-load-symbol-check"

FORBIDDEN_SYMBOLS = (
    "emit_baseline_function",
    "emit_baseline_plan",
    "build_baseline_plan",
)
REQUIRED_SYMBOLS = (
    "CodeArena",
    "StoreJitCache",
    "instantiate",
)


class SymbolCheckError(RuntimeError):
    """The build or symbol inspection failed before a verdict was valid."""


def find_violations(symbols):
    """Return failure strings for a symbol-name iterable; [] means pass."""
    names = list(symbols)
    failures = []
    for forbidden in FORBIDDEN_SYMBOLS:
        leaked = sorted({name for name in names if forbidden in name})
        for name in leaked:
            failures.append(
                f"aot-load symbol check: emitter symbol {name!r} is present "
                f"in an aot-load-only binary (matched {forbidden!r}); the "
                "emitter backends must stay behind the `jit` feature"
            )
    for required in REQUIRED_SYMBOLS:
        if not any(required in name for name in names):
            failures.append(
                f"aot-load symbol check: no symbol mentions {required!r}; "
                "the binary does not look like an aot-load embedder build, "
                "so the absence check would be vacuous"
            )
    return failures


def parse_nm_output(text):
    """Extract symbol names from `nm` output lines.

    Lines look like `00000000001234abcd T _ZN...` or `                 U
    name`; the symbol name is the last whitespace-separated field. Lines
    without an address/type shape (file headers, blank lines) are ignored.
    """
    names = []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) >= 2:
            names.append(fields[-1])
    return names


def resolve_nm_tool():
    rustup = subprocess.run(
        ["rustup", "which", "llvm-nm"],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    candidate = rustup.stdout.strip()
    if rustup.returncode == 0 and candidate and os.path.isfile(candidate):
        return [candidate]
    resolved = shutil.which("llvm-nm") or shutil.which("nm")
    if resolved:
        return [resolved]
    raise SymbolCheckError("could not find rustup llvm-nm, llvm-nm, or nm")


def build_embedder(target_dir):
    command = [
        "cargo",
        "build",
        "-p",
        "telomere-minimal-embedder",
        "--profile",
        "dev",
        "--no-default-features",
        "--features",
        "simd,aot-load",
        "--bin",
        "embed-core",
    ]
    env = dict(os.environ)
    env["CARGO_TARGET_DIR"] = str(target_dir)
    completed = subprocess.run(
        command,
        cwd=REPO_ROOT,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.rstrip("\n")
        message = f"`{shlex.join(command)}` exited with status {completed.returncode}"
        if detail:
            message = f"{message}\n{detail}"
        raise SymbolCheckError(message)
    binary = target_dir / "debug" / "embed-core"
    if not binary.is_file():
        raise SymbolCheckError(f"embedder build produced no binary at {binary}")
    return binary


def list_symbols(binary):
    command = resolve_nm_tool() + [str(binary)]
    completed = subprocess.run(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if completed.returncode != 0:
        detail = completed.stderr.rstrip("\n")
        message = f"`{shlex.join(command)}` exited with status {completed.returncode}"
        if detail:
            message = f"{message}\n{detail}"
        raise SymbolCheckError(message)
    return parse_nm_output(completed.stdout)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--binary",
        type=Path,
        default=None,
        help="check this binary instead of building the embedder",
    )
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=DEFAULT_TARGET_DIR,
        help="dedicated Cargo target directory for the check build",
    )
    args = parser.parse_args(argv)

    try:
        binary = args.binary if args.binary is not None else build_embedder(args.target_dir)
        symbols = list_symbols(binary)
    except SymbolCheckError as error:
        print(f"aot-load symbol check: {error}", file=sys.stderr)
        return 1

    failures = find_violations(symbols)
    if failures:
        for failure in failures:
            print(failure, file=sys.stderr)
        return 1
    print(
        "aot-load symbol check: no emitter symbols in the aot-load-only "
        f"embedder binary ({binary}); load-path symbols present"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
