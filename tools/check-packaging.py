#!/usr/bin/env python3
"""Check that every workspace crate has an explicit packaging policy."""

import json
import subprocess
import sys
from pathlib import Path


# Keep this list complete: adding a workspace crate requires deciding whether it
# may be published before the release workflow can proceed.
EXPECTED = {
    "telomere": False,
    "telomere-jit-codegen": False,
    "telomere-macros": False,
    "union-find": False,
    "telomere-component": False,
    "telomere-component-bindgen": False,
    "telomere-component-wasi": False,
    "telomere-cli": False,
}

REPO_ROOT = Path(__file__).resolve().parent.parent


def load_metadata():
    """Return Cargo metadata, reporting tool failures without a traceback."""
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
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"cargo metadata produced invalid JSON: {error}") from error


def expected_publish_value(intent):
    """Translate the policy shorthand to Cargo metadata's publish representation."""
    if intent is False:
        return []
    if intent is True:
        return None
    if isinstance(intent, list):
        return intent
    raise ValueError(f"invalid publish intent in EXPECTED: {intent!r}")


def publish_matches(intent, actual):
    """Use type-aware checks so the policy mapping remains strict."""
    expected = expected_publish_value(intent)
    if intent is False:
        return isinstance(actual, list) and actual == expected
    if intent is True:
        return actual is None
    return isinstance(actual, list) and actual == expected


def workspace_packages(metadata):
    """Select workspace members from the complete package list by package ID."""
    packages = metadata.get("packages")
    member_ids = metadata.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(member_ids, list):
        raise RuntimeError("cargo metadata did not contain packages and workspace_members lists")

    by_id = {}
    for package in packages:
        if not isinstance(package, dict) or not isinstance(package.get("id"), str):
            raise RuntimeError("cargo metadata contains a package without an ID")
        by_id[package["id"]] = package

    missing_ids = [member_id for member_id in member_ids if member_id not in by_id]
    if missing_ids:
        raise RuntimeError(
            "cargo metadata omitted workspace members: " + ", ".join(sorted(missing_ids))
        )

    members = [by_id[member_id] for member_id in member_ids]
    actual = {}
    duplicate_names = []
    for package in members:
        name = package.get("name")
        if not isinstance(name, str):
            raise RuntimeError("cargo metadata contains a workspace member without a name")
        if name in actual:
            duplicate_names.append(name)
        actual[name] = package

    if duplicate_names:
        raise RuntimeError(
            "cargo metadata contains duplicate workspace package names: "
            + ", ".join(sorted(set(duplicate_names)))
        )
    return actual


def validate(actual):
    """Return all policy violations so each affected crate is reported together."""
    errors = []
    actual_names = set(actual)
    expected_names = set(EXPECTED)

    for name in sorted(actual_names - expected_names):
        errors.append(f"declare publish intent for {name} in tools/check-packaging.py")
    for name in sorted(expected_names - actual_names):
        errors.append(
            f"stale entry {name} in tools/check-packaging.py: crate was removed or renamed"
        )

    versions = {package.get("version") for package in actual.values()}
    if len(versions) != 1:
        rendered_versions = ", ".join(sorted(repr(version) for version in versions))
        for name in sorted(actual):
            errors.append(
                f"{name}: workspace members must share one version; "
                f"got {actual[name].get('version')!r} (all: {rendered_versions})"
            )

    for name in sorted(actual_names & expected_names):
        package = actual[name]
        license_value = package.get("license")
        license_file = package.get("license_file")
        if license_value != "Apache-2.0" or license_file is not None:
            errors.append(
                f"{name}: expected license='Apache-2.0' and license_file=None; "
                f"got license={license_value!r}, license_file={license_file!r}"
            )

        actual_publish = package.get("publish")
        intent = EXPECTED[name]
        if not publish_matches(intent, actual_publish):
            errors.append(
                f"{name}: expected publish={expected_publish_value(intent)!r}; "
                f"got publish={actual_publish!r}"
            )

    return errors, versions


def main(argv):
    if argv not in ([], ["--print-version"]):
        print("usage: tools/check-packaging.py [--print-version]", file=sys.stderr)
        return 2

    try:
        actual = workspace_packages(load_metadata())
        errors, versions = validate(actual)
    except (RuntimeError, ValueError) as error:
        print(f"check-packaging: {error}", file=sys.stderr)
        return 1

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    version = next(iter(versions))
    if argv == ["--print-version"]:
        print(version)
    else:
        print(f"packaging check passed for {len(actual)} workspace crates (version {version})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
