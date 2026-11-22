#!/usr/bin/env python3
"""Check that every jit-gated test target is executed by a CI job.

Compiling a test target is not executing it. A target whose source gates
tests behind the `jit` feature only runs those tests when some CI job
executes the target with the feature enabled; the default-feature workspace
job compiles them out entirely. Before this check existed, the jit-gated
tests in tests/trap_info.rs (#210) and tests/metering.rs were compiled by
clippy but executed by no job (#235).

This checker therefore fails when a test target that contains jit-gated
tests is not named by any `cargo test --features ... jit ...` step in
.github/workflows/ci.yaml (a step without a `--test` restriction covers
every target). The reader is intentionally narrow and stdlib-only, in the
style of tools/check-feature-wiring.py: it recognises `--features jit`,
comma-separated feature lists containing `jit`, and `--all-features`, and
it does not evaluate `-F` spellings or features set through the environment.
"""

import json
import re
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yaml"

JIT_FEATURE_RE = re.compile(r'feature\s*=\s*"jit"')
TEST_ATTR_RE = re.compile(r"#\[\s*(?:[A-Za-z_][\w]*::)?test\s*(?:\]|\()")
CARGO_TEST_RE = re.compile(r"\bcargo\s+test\b")
FEATURES_RE = re.compile(r"--features[= ]+\"?([A-Za-z0-9_,\-]+)\"?")
TEST_TARGET_RE = re.compile(r"--test[= ]+\"?([A-Za-z0-9_\-]+)\"?")


def load_test_targets():
    """Return (package, name, src_path) for every Cargo test target."""
    if not (REPO_ROOT / "Cargo.toml").is_file():
        raise RuntimeError(f"repository root has no Cargo.toml: {REPO_ROOT}")

    command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    try:
        result = subprocess.run(
            command,
            cwd=REPO_ROOT,
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
    except OSError as error:
        raise RuntimeError(f"could not run cargo metadata: {error}") from error

    if result.returncode != 0:
        details = result.stderr.strip() or result.stdout.strip()
        if details:
            raise RuntimeError(
                f"cargo metadata failed with exit code {result.returncode}: {details}"
            )
        raise RuntimeError(f"cargo metadata failed with exit code {result.returncode}")

    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"cargo metadata produced invalid JSON: {error}") from error

    targets = []
    for package in metadata.get("packages", []):
        for target in package.get("targets", []):
            if "test" in target.get("kind", []):
                targets.append(
                    (package["name"], target["name"], Path(target["src_path"]))
                )
    return targets


def source_has_jit_gated_tests(source):
    """True when a test target source gates part of itself on the jit feature.

    The gate may cover a single test (``#[cfg(feature = "jit")]`` above one
    ``#[test]``) or the whole file (a cfg on the imports of tests/jit.rs);
    either way the tests exist only when the feature is enabled. A file that
    mentions the feature but has no tests (a shared helper module) is not a
    test target and never reaches this function.
    """
    return bool(JIT_FEATURE_RE.search(source)) and bool(TEST_ATTR_RE.search(source))


def jit_gated_targets(targets):
    """Filter (package, name, src_path) targets to those with jit-gated tests."""
    gated = []
    for package, name, src_path in targets:
        source = src_path.read_text(encoding="utf-8")
        if source_has_jit_gated_tests(source):
            gated.append((package, name, src_path))
    return gated


def step_enables_jit(line):
    """True when a cargo test command line enables the jit feature."""
    if "--all-features" in line:
        return True
    match = FEATURES_RE.search(line)
    if match is None:
        return False
    return "jit" in match.group(1).split(",")


def executed_jit_targets(ci_text):
    """Return (covered_names, covers_all) from the workflow's cargo test steps.

    Only steps that enable the jit feature count: running a target without
    the feature compiles the gated tests out, which is exactly the hole this
    checker guards. A jit-enabled step with no `--test` restriction executes
    every target of its package selection, so it covers all names.
    """
    covered = set()
    covers_all = False
    for line in ci_text.splitlines():
        if not CARGO_TEST_RE.search(line):
            continue
        if not step_enables_jit(line):
            continue
        names = TEST_TARGET_RE.findall(line)
        if names:
            covered.update(names)
        else:
            covers_all = True
    return covered, covers_all


def coverage_errors(gated, covered, covers_all):
    """One error per jit-gated target that no jit-enabled CI step executes."""
    if covers_all:
        return []
    errors = []
    for package, name, src_path in gated:
        if name not in covered:
            errors.append(
                f"{package}: test target {name} ({src_path}) contains tests gated "
                'on feature "jit" but no cargo test step in '
                ".github/workflows/ci.yaml executes it with the jit feature "
                "enabled; add a --test "
                f"{name} step with --features jit (see the jit-native-test job) "
                "or run the package's full test suite with --features jit"
            )
    return errors


def main():
    try:
        targets = load_test_targets()
        gated = jit_gated_targets(targets)
        ci_text = CI_WORKFLOW.read_text(encoding="utf-8")
    except (OSError, RuntimeError) as error:
        print(f"::error::{error}", file=sys.stderr)
        return 1

    covered, covers_all = executed_jit_targets(ci_text)
    errors = coverage_errors(gated, covered, covers_all)
    for error in errors:
        print(f"::error::{error}", file=sys.stderr)
    if errors:
        return 1

    names = ", ".join(sorted(name for _, name, _ in gated)) or "none"
    print(f"all jit-gated test targets are executed by CI: {names}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
