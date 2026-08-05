#!/usr/bin/env python3
"""Unit tests for the manifest feature-wiring checker."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path


TOOLS_DIR = Path(__file__).resolve().parent
MODULE_PATH = TOOLS_DIR / "check-feature-wiring.py"
SPEC = importlib.util.spec_from_file_location("check_feature_wiring", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
feature_wiring = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = feature_wiring
SPEC.loader.exec_module(feature_wiring)


def package_with_dependency(dependency):
    return {"name": "fixture", "dependencies": [dependency]}


class OptionalDependencyErrorsTests(unittest.TestCase):
    def test_a_leak_is_reported(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency({"name": "wide", "optional": True}),
            {"simd": ["wide"], "wide": ["dep:wide"]},
            {"simd"},
        )

        self.assertEqual(
            errors,
            [
                "fixture: feature simd references optional dependency wide by its bare "
                "name, which leaks an implicit wide feature; write dep:wide"
            ],
        )

    def test_b_fixed_dep_reference_is_allowed(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency({"name": "wide", "optional": True}),
            {"simd": ["dep:wide"]},
            {"simd"},
        )

        self.assertEqual(errors, [])

    def test_c_explicit_same_name_feature_is_allowed(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency({"name": "wide", "optional": True}),
            {"simd": ["wide"], "wide": ["dep:wide"]},
            {"simd", "wide"},
        )

        self.assertEqual(errors, [])

    def test_explicit_dep_reference_is_allowed(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency({"name": "tokio", "optional": True}),
            {"threads": ["dep:tokio"]},
            {"threads"},
        )

        self.assertEqual(errors, [])

    def test_renamed_optional_dependency_uses_its_feature_key(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency(
                {"name": "wide", "rename": "w", "optional": True}
            ),
            {"simd": ["w"], "w": ["dep:w"]},
            {"simd"},
        )

        self.assertEqual(
            errors,
            [
                "fixture: feature simd references optional dependency w by its bare "
                "name, which leaks an implicit w feature; write dep:w"
            ],
        )

    def test_nonoptional_dependency_is_ignored(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency({"name": "wide", "optional": False}),
            {"simd": ["wide"], "wide": ["dep:wide"]},
            {"simd"},
        )

        self.assertEqual(errors, [])

    def test_dev_optional_dependency_is_ignored(self) -> None:
        errors = feature_wiring.optional_dependency_errors(
            "fixture",
            package_with_dependency(
                {"name": "wide", "optional": True, "kind": "dev"}
            ),
            {"simd": ["wide"], "wide": ["dep:wide"]},
            {"simd"},
        )

        self.assertEqual(errors, [])

    def test_invalid_optional_shape_is_rejected(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "invalid optional"):
            feature_wiring.optional_dependency_errors(
                "fixture",
                package_with_dependency({"name": "wide", "optional": "true"}),
                {"simd": ["wide"], "wide": ["dep:wide"]},
                {"simd"},
            )


class DeclaredFeatureNamesTests(unittest.TestCase):
    def test_reader_accepts_quoted_keys_and_ignores_multiline_values_and_other_sections(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            manifest_path = Path(temporary) / "Cargo.toml"
            manifest_path.write_text(
                """[package]
name = "fixture"

[features]
"quoted-feature" = [
    "simd",
    "not-a-feature-key",
]
plain = []

[dependencies]
dependency = "1"
""",
                encoding="utf-8",
            )

            self.assertEqual(
                feature_wiring.declared_feature_names(manifest_path),
                {"quoted-feature", "plain"},
            )

    def test_validate_rejects_manifest_features_missing_from_metadata(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            manifest_path = Path(temporary) / "Cargo.toml"
            manifest_path.write_text(
                """[features]
missing = []
""",
                encoding="utf-8",
            )
            package = {
                "name": "telomere",
                "manifest_path": str(manifest_path),
                "features": {"simd": []},
                "dependencies": [],
            }

            with self.assertRaisesRegex(RuntimeError, "absent from cargo metadata"):
                feature_wiring.validate({"telomere": package})


if __name__ == "__main__":
    unittest.main()
