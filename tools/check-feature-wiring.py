#!/usr/bin/env python3
"""Check that workspace feature forwarding follows the telomere policy.

No-op aliases compare direct Cargo metadata entry lists only; transitive feature
expansion is not evaluated.
"""

import json
import subprocess
import sys
from pathlib import Path


# Keep this list complete: adding a workspace crate requires deciding which
# feature-policy role it has, rather than inheriting a role by accident.
ROLE_MAP = {
    "telomere": {
        "role": "definition",
        "reason": "Defines the core simd and threads features that gate code.",
    },
    "telomere-component": {
        "role": "forwarder",
        "reason": "Forwards the core feature policy through the component runtime.",
    },
    "telomere-component-wasi": {
        "role": "forwarder",
        "reason": "Forwards the component runtime feature policy through WASI.",
    },
    "telomere-minimal-embedder": {
        "role": "forwarder",
        "reason": "Offers the opt-in embedder feature forwarding surface.",
    },
    "telomere-cli": {
        "role": "aggregator",
        "reason": "Keeps its CLI full feature as the explicit aggregate entry point.",
    },
    "telomere-macros": {
        "role": "unrelated",
        "reason": "Proc macro with no non-dev dependency on a policy feature crate.",
    },
    "telomere-jit-codegen": {
        "role": "unrelated",
        "reason": "JIT code generation helper with no policy feature dependency.",
    },
    "telomere-component-bindgen": {
        "role": "unrelated",
        "reason": "Bindgen proc macro has family dependencies only in dev-dependencies.",
    },
}

FEATURES = ("simd", "threads")
VALID_ROLES = frozenset(("definition", "forwarder", "aggregator", "unrelated"))
REPO_ROOT = Path(__file__).resolve().parent.parent


def load_metadata():
    """Return Cargo metadata, reporting tool failures without a traceback."""
    if not (REPO_ROOT / "Cargo.toml").is_file():
        raise RuntimeError(f"repository root has no Cargo.toml: {REPO_ROOT}")

    command = [
        "cargo",
        "metadata",
        "--format-version",
        "1",
        "--locked",
        "--no-deps",
    ]
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
    if not isinstance(metadata, dict):
        raise RuntimeError("cargo metadata produced JSON that is not an object")
    return metadata


def workspace_packages(metadata):
    """Return workspace packages keyed by name, rejecting incomplete metadata."""
    packages = metadata.get("packages")
    member_ids = metadata.get("workspace_members")
    if not isinstance(packages, list) or not isinstance(member_ids, list):
        raise RuntimeError("cargo metadata did not contain packages and workspace_members lists")
    if not packages:
        raise RuntimeError("cargo metadata produced an empty packages list")
    if not member_ids:
        raise RuntimeError("cargo metadata produced an empty workspace_members list")

    by_id = {}
    for package in packages:
        if not isinstance(package, dict):
            raise RuntimeError("cargo metadata contains a package that is not an object")
        package_id = package.get("id")
        if not isinstance(package_id, str) or not package_id:
            raise RuntimeError("cargo metadata contains a package without an ID")
        if package_id in by_id:
            raise RuntimeError(f"cargo metadata contains duplicate package ID {package_id!r}")
        by_id[package_id] = package

    missing_ids = []
    for member_id in member_ids:
        if not isinstance(member_id, str):
            raise RuntimeError("cargo metadata contains a workspace member without an ID")
        if member_id not in by_id:
            missing_ids.append(member_id)
    if missing_ids:
        raise RuntimeError(
            "cargo metadata omitted workspace members: " + ", ".join(sorted(missing_ids))
        )

    actual = {}
    duplicates = []
    for member_id in member_ids:
        package = by_id[member_id]
        name = package.get("name")
        if not isinstance(name, str) or not name:
            raise RuntimeError("cargo metadata contains a workspace member without a name")
        if name in actual:
            duplicates.append(name)
        actual[name] = package
    if duplicates:
        raise RuntimeError(
            "cargo metadata contains duplicate workspace package names: "
            + ", ".join(sorted(set(duplicates)))
        )
    if not actual:
        raise RuntimeError("cargo metadata produced no workspace packages")
    return actual


def package_features(package):
    """Return one package's feature map after checking its metadata shape."""
    name = package["name"]
    features = package.get("features")
    if not isinstance(features, dict):
        raise RuntimeError(f"{name}: cargo metadata has no feature map")

    for feature, entries in features.items():
        if not isinstance(feature, str) or not isinstance(entries, list):
            raise RuntimeError(f"{name}: cargo metadata has an invalid feature declaration")
        if not all(isinstance(entry, str) for entry in entries):
            raise RuntimeError(f"{name}: cargo metadata has a non-string feature entry")
    return features


def package_dependencies(package):
    """Return one package's declared dependency edges after checking their shape."""
    name = package["name"]
    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        raise RuntimeError(f"{name}: cargo metadata has no dependency list")

    for dependency in dependencies:
        if not isinstance(dependency, dict):
            raise RuntimeError(f"{name}: cargo metadata has a dependency that is not an object")
        dependency_name = dependency.get("name")
        if not isinstance(dependency_name, str) or not dependency_name:
            raise RuntimeError(f"{name}: cargo metadata has a dependency without a name")
        kind = dependency.get("kind")
        if kind is not None and not isinstance(kind, str):
            raise RuntimeError(f"{name}: cargo metadata has a dependency with an invalid kind")
        rename = dependency.get("rename")
        if rename is not None and (not isinstance(rename, str) or not rename):
            raise RuntimeError(f"{name}: cargo metadata has a dependency with an invalid rename")
    return dependencies


def dependency_feature_name(dependency):
    """Return the dependency key used in a Cargo feature forwarding entry."""
    return dependency.get("rename") or dependency["name"]


def policy_edges(package, family_features):
    """Return all non-dev edges to workspace packages declaring policy features."""
    edges = []
    for dependency in package_dependencies(package):
        target = dependency["name"]
        target_features = family_features.get(target)
        if target_features is None or dependency.get("kind") == "dev":
            continue
        edges.append((dependency, target_features))
    return edges


def declared_roles(actual):
    """Validate role-map coverage and return usable roles plus all map errors."""
    errors = []
    actual_names = set(actual)
    mapped_names = set(ROLE_MAP)

    for name in sorted(actual_names - mapped_names):
        errors.append(
            f"declare feature-policy role and reason for {name} in tools/check-feature-wiring.py"
        )
    for name in sorted(mapped_names - actual_names):
        errors.append(
            f"stale feature-policy role for {name} in tools/check-feature-wiring.py: "
            "crate was removed or renamed"
        )

    roles = {}
    for name in sorted(actual_names & mapped_names):
        specification = ROLE_MAP[name]
        if not isinstance(specification, dict):
            errors.append(f"{name}: feature-policy role entry must be an object")
            continue
        role = specification.get("role")
        reason = specification.get("reason")
        if role not in VALID_ROLES:
            errors.append(f"{name}: unknown feature-policy role {role!r}")
            continue
        if not isinstance(reason, str) or not reason.strip():
            errors.append(f"{name}: feature-policy role needs a non-empty reason")
            continue
        roles[name] = role
    return roles, errors


def activates(entries, dependency_name, feature):
    """Accept direct and weak Cargo dependency-feature forwarding syntax."""
    return (
        f"{dependency_name}/{feature}" in entries
        or f"{dependency_name}?/{feature}" in entries
    )


def validate(actual):
    """Return every feature-policy violation found in the workspace metadata."""
    roles, errors = declared_roles(actual)
    feature_maps = {name: package_features(package) for name, package in actual.items()}
    family_features = {
        name: frozenset(feature for feature in FEATURES if feature in feature_maps[name])
        for name in actual
        if any(feature in feature_maps[name] for feature in FEATURES)
    }
    edges_by_package = {
        name: policy_edges(package, family_features) for name, package in actual.items()
    }

    for name in sorted(actual):
        for dependency, _ in edges_by_package[name]:
            if dependency.get("uses_default_features") is not False:
                errors.append(
                    f"{name}: non-dev dependency {dependency['name']} must set "
                    "default-features = false"
                )

    for name in sorted(roles):
        role = roles[name]
        features = feature_maps[name]
        edges = edges_by_package[name]

        if role == "definition":
            for feature in FEATURES:
                if feature not in features:
                    errors.append(f"{name}: definition must declare {feature}")
        elif role == "forwarder":
            for feature in FEATURES:
                entries = features.get(feature)
                if entries is None:
                    errors.append(f"{name}: forwarder must declare {feature}")
                    continue
                for dependency, target_features in edges:
                    if feature not in target_features:
                        continue
                    dependency_name = dependency_feature_name(dependency)
                    if not activates(entries, dependency_name, feature):
                        errors.append(
                            f"{name}: forwarder feature {feature} must activate "
                            f"{dependency_name}/{feature} for non-dev dependency "
                            f"{dependency['name']}"
                        )
        elif role == "aggregator":
            for dependency, target_features in edges:
                dependency_name = dependency_feature_name(dependency)
                for feature in FEATURES:
                    if feature not in target_features:
                        continue
                    if not any(
                        activates(entries, dependency_name, feature)
                        for entries in features.values()
                    ):
                        errors.append(
                            f"{name}: aggregator has no feature that activates "
                            f"{dependency_name}/{feature} for non-dev dependency "
                            f"{dependency['name']}"
                        )
        else:
            for feature in FEATURES:
                if feature in features:
                    errors.append(f"{name}: unrelated crate must not declare {feature}")
            for dependency, _ in edges:
                errors.append(
                    f"{name}: unrelated crate must not have a non-dev dependency on "
                    f"policy feature crate {dependency['name']}"
                )

        if role != "unrelated":
            default_entries = frozenset(features.get("default", []))
            for feature, entries in features.items():
                if feature != "default" and frozenset(entries) == default_entries:
                    errors.append(
                        f"{name}: feature {feature} declares the same direct entry "
                        "list as default"
                    )

    return errors


def main(argv):
    if argv:
        print("usage: tools/check-feature-wiring.py", file=sys.stderr)
        return 2

    try:
        packages = workspace_packages(load_metadata())
        errors = validate(packages)
    except RuntimeError as error:
        print(f"check-feature-wiring: {error}", file=sys.stderr)
        return 1

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1

    print(f"feature wiring check passed for {len(packages)} workspace crates")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
