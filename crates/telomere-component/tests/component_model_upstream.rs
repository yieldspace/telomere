//! Vendored upstream Component Model conformance suite (issue #136).
//!
//! The suite is the `test/` tree of <https://github.com/WebAssembly/component-model>,
//! vendored as the git submodule `tests/component-model-testsuite` — the
//! same vendoring mechanism the core layer uses for WebAssembly/testsuite.
//! Every file is executed by the shared harness in
//! `tests/common/component_model.rs`, which runs runtime directives
//! (`invoke`, `assert_return`, `assert_trap`, `assert_unlinkable`,
//! `register`, `(component instance ...)`) against real instances, so the
//! canonical ABI is checked against an external source instead of only
//! compile/validate assertions and hand-written expectations.
//!
//! Exclusions are explicit per file with reasons, mirroring the core
//! harness: nothing is skipped silently, and in vendored mode the test
//! fails if an exclusion names a file that no longer exists (stale
//! exclusion) or a discovered file is neither run nor excluded.
//!
//! The suite root can be overridden with the
//! `TELOMERE_COMPONENT_SUITE_DIR` environment variable (pointing at a
//! `test/` tree) for local investigation; CI uses the submodule.

use std::path::{Path, PathBuf};

mod common;

/// Reason shared by every file under `test/async/`.
const ASYNC_NON_GOAL: &str = "async component-model proposals are a documented \
    non-goal (docs/component-model/new-component-runtime.md, Non-goals)";

/// Per-file exclusions outside `test/async/`, each with its reason.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "values/post-return.wast",
        "listed in upstream test/nyi.txt (not yet implemented by the \
         reference implementation)",
    ),
    (
        "validation/kebab.wast",
        "listed in upstream test/nyi.txt (not yet implemented by the \
         reference implementation)",
    ),
    (
        "linking/tags.wast",
        "tags are a documented non-goal \
         (docs/component-model/new-component-runtime.md, Non-goals)",
    ),
    (
        "validation/map.wast",
        "CM_MAP is a documented non-goal \
         (docs/component-model/new-component-runtime.md, Non-goals)",
    ),
    (
        "values/concat.wast",
        "the map cases (from line 462) use CM_MAP types, a documented \
         non-goal (docs/component-model/new-component-runtime.md, \
         Non-goals); the string/list cases before them pass",
    ),
    (
        "values/numerics.wast",
        "blocked by a canonical-ABI validation defect: `char` arguments \
         are not validated when lowering, so invalid code points \
         (surrogates, values above U+10FFFF) reach the guest, which \
         traps `unreachable`, instead of the canonical `invalid char \
         bit pattern` trap (lines 193, 195, 197)",
    ),
    (
        "resources/borrows.wast",
        "blocked by resource-handle defects: the guest self-check \
         traps on handle/rep state at line 158 (issue #317 family), \
         and dropping an owned resource that is currently borrowed is \
         not rejected (line 161: the expected trap `cannot remove \
         owned resource while borrowed` does not happen)",
    ),
    (
        "resources/multiple-resources.wast",
        "blocked by issue #317: resource handle state is shared \
         across instances instead of per-instance, so the \
         self-checking guest traps at line 169",
    ),
    (
        "values/alignment.wast",
        "blocked by issue #312: canonical ABI loads do not trap on \
         unaligned pointers, so the assert_trap cases (lines 26, 51, \
         81, 110, 138, 170, 172, 204, 206) succeed instead of trapping",
    ),
    (
        "values/realloc.wast",
        "blocked by issue #322 (the result of realloc is not \
         bounds-checked: line 110 does not trap, and lines 66, 108, 123 \
         trap with a different error than the spec text) and issue #312 \
         (line 93: an unaligned pointer does not trap in the canonical \
         layer)",
    ),
    (
        "values/variants.wast",
        "blocked by a canonical-ABI validation defect: variant/enum \
         discriminants are not validated when lowering, so invalid \
         discriminants reach the guest, which traps `unreachable`, \
         instead of the canonical `invalid variant discriminant` trap \
         (lines 72-78); additionally the component at line 82 uses \
         async canonical operations (canon task.return, async \
         lift/lower), a documented non-goal, which also removes the \
         exports used at lines 182-185",
    ),
    (
        "linking/unit.wast",
        "blocked by several known defects: resource handle indices \
         and destructor counts are shared \
         across instances instead of per-instance (issue #317; lines \
         724-729, 787-792, 863-866, 930-933, 994-997, 1020-1022); a \
         valid func import whose surface type is an unnamed generic \
         type is rejected (issue #321; line 799); inline core \
         instances exporting globals are not supported by the runtime \
         (issue #324; lines 2010, 2043)",
    ),
    (
        "resources/handle-table.wast",
        "blocked by resource-handle defects (issue #317 family): \
         handle index allocation/reuse order diverges from the spec, \
         so the self-checking guest traps (lines 48, 105); \
         additionally canonical handle operations do not validate \
         indices, so unknown, already-dropped, zero, and wrong-type \
         handles reach the guest or silently succeed instead of \
         trapping `unknown handle index` (lines 200-212, 260, 292, \
         321, 323)",
    ),
    (
        "validation/attributes.wast",
        "the split extern-name encoding used for `implements` \
         annotations (name prefix 0x02) is not supported by the decoder",
    ),
    (
        "validation/version-suffix.wast",
        "the split extern-name encoding used for `version-suffix` \
         annotations (name prefix 0x02) is not supported by the decoder",
    ),
    (
        "validation/extern-names.wast",
        "malformed extern names are not rejected: the package-name \
         grammar (expected `/` after package name, trailing characters) \
         is not validated",
    ),
    (
        "binary/binary.wast",
        "malformed binaries are accepted: truncated sections and \
         invalid UTF-8 names decode successfully, and core-layer decode \
         errors are wrapped without the specific reason the suite \
         matches on",
    ),
    (
        "validation/resources.wast",
        "resource type validation gaps: valid resource types are \
         rejected as imports (lines 270, 291, 312), a resource-identity \
         mismatch is accepted (line 576), and own/borrow mismatches are \
         reported only as a generic defvaltype mismatch (lines 101, 179)",
    ),
    (
        "validation/outer-alias.wast",
        "outer-alias validation is not enforced: aliases that \
         transitively refer to resources (line 68) and aliases to items \
         other than types or instances (line 172) are accepted",
    ),
    (
        "validation/external-visibility.wast",
        "func exportability is not validated: funcs that are not valid \
         to be used as exports are accepted (lines 27, 69, 129), and one \
         valid case is rejected instead (line 79)",
    ),
    (
        "validation/max-value-size.wast",
        "the validator's effective type size limit (1_000_000) is \
         stricter than the spec's elem_size < 2^28 rule and rejects the \
         valid boundary types at line 5",
    ),
    (
        "validation/indicies.wast",
        "uses async/threads canonical operations (waitable-set, \
         thread.new-indirect, future), which are documented non-goals, \
         and a valid core-module import/alias shape is rejected with \
         `type idx not found` (line 340)",
    ),
];

/// Every file under `test/async/` is excluded with [`ASYNC_NON_GOAL`].
const ASYNC_EXCLUDED: &[&str] = &[
    "async/async-calls-sync.wast",
    "async/big-interleaving-test.wast",
    "async/builtin-trap-poisons-instance.wast",
    "async/cancel-and-exclusive-lock.wast",
    "async/cancel-delivery.wast",
    "async/cancel-not-delivered.wast",
    "async/cancel-resumed-callback-switch.wast",
    "async/cancel-stream.wast",
    "async/cancel-subtask.wast",
    "async/cancel-targeted-resume.wast",
    "async/closed-stream.wast",
    "async/cross-abi-calls.wast",
    "async/cross-task-future.wast",
    "async/deadlock.wast",
    "async/dont-block-start.wast",
    "async/drop-cross-task-borrow.wast",
    "async/drop-stream.wast",
    "async/drop-subtask.wast",
    "async/drop-waitable-set.wast",
    "async/dtor-context-is-zero.wast",
    "async/during-sync-call-blocking-ops.wast",
    "async/during-sync-call-exclusive-resume.wast",
    "async/during-sync-call-may-block-if-other-ready-threads.wast",
    "async/during-sync-call-no-sibling-resume.wast",
    "async/during-sync-scheduling-candidates.wast",
    "async/empty-wait.wast",
    "async/exit-without-resolving.wast",
    "async/forward.wast",
    "async/future-completion-order.wast",
    "async/futures-must-write.wast",
    "async/idle-drop.wast",
    "async/partial-stream-copies.wast",
    "async/passing-resources.wast",
    "async/reentrance.wast",
    "async/same-component-stream-future.wast",
    "async/self-switch-traps.wast",
    "async/switch-to-ready-callback.wast",
    "async/sync-barges-in.wast",
    "async/sync-streams.wast",
    "async/trap-if-block-and-sync.wast",
    "async/trap-if-done.wast",
    "async/trap-if-sync-and-waitable-set.wast",
    "async/trap-if-transfer-in-waitable-set.wast",
    "async/validate-no-async-abi-for-sync-type.wast",
    "async/validate-no-stream-char.wast",
    "async/validate-task-return.wast",
    "async/wait-during-callback.wast",
    "async/zero-length.wast",
];

fn exclusion_reason(relative: &str) -> Option<&'static str> {
    if let Some((_, reason)) = EXCLUDED.iter().find(|(path, _)| *path == relative) {
        return Some(reason);
    }
    if ASYNC_EXCLUDED.contains(&relative) {
        return Some(ASYNC_NON_GOAL);
    }
    None
}

fn suite_root() -> (PathBuf, bool) {
    if let Some(dir) = std::env::var_os("TELOMERE_COMPONENT_SUITE_DIR") {
        return (PathBuf::from(dir), false);
    }
    (
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/component-model-testsuite/test"),
        true,
    )
}

fn collect_wast_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()));
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_wast_files(root, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "wast") {
            let relative = path
                .strip_prefix(root)
                .expect("collected file is under the suite root")
                .to_string_lossy()
                .replace('\\', "/");
            out.push(relative);
        }
    }
}

#[tokio::test]
async fn component_model_upstream_testsuite() {
    let (root, vendored) = suite_root();
    if !root.is_dir() {
        panic!(
            "vendored component-model testsuite not found at {}\n\
             run `git submodule update --init` (the suite is the \
             WebAssembly/component-model submodule at \
             tests/component-model-testsuite)",
            root.display()
        );
    }

    let mut files = Vec::new();
    collect_wast_files(&root, &root, &mut files);
    assert!(
        !files.is_empty(),
        "expected at least one testsuite file in {}",
        root.display()
    );

    if vendored {
        // Every exclusion must name a file that exists at the pinned
        // revision; a stale exclusion would silently shrink coverage when
        // the pin moves.
        for (path, _) in EXCLUDED {
            assert!(
                files.iter().any(|file| file == path),
                "excluded file {path} does not exist in the vendored suite; \
                 update the exclusion table in component_model_upstream.rs"
            );
        }
        for path in ASYNC_EXCLUDED {
            assert!(
                files.iter().any(|file| file == path),
                "excluded file {path} does not exist in the vendored suite; \
                 update the exclusion table in component_model_upstream.rs"
            );
        }
    }

    let mut checked = 0usize;
    let mut ran = 0usize;
    let mut excluded = 0usize;
    let mut failures = Vec::new();

    for relative in &files {
        if let Some(reason) = exclusion_reason(relative) {
            excluded += 1;
            println!("excluded {relative}: {reason}");
            continue;
        }
        ran += 1;
        let path = root.join(relative);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let report = common::component_model::run_component_testsuite_case(&path, &text).await;
        checked += report.directives_checked;
        failures.extend(report.failures);
    }

    println!(
        "component_model_upstream_testsuite completed: files={} ran={} excluded={} checked={}",
        files.len(),
        ran,
        excluded,
        checked
    );

    if !failures.is_empty() {
        panic!(
            "component_model_upstream_testsuite failures (checked={checked})\n\n{}",
            failures.join("\n\n")
        );
    }
}

/// Self-contained smoke coverage for the harness's runtime-directive
/// machinery: canonical ABI value round-trips for every type family the
/// runtime claims to support (per
/// `docs/component-model/new-component-runtime.md`). These cases are
/// written locally in the upstream tests' style (named parameters,
/// exported named types) and run even where the vendored submodule is not
/// checked out, so the execution path itself is always guarded.
#[tokio::test]
async fn upstream_harness_runtime_directives_smoke() {
    let cases: &[(&str, &str)] = &[
        (
            "strings",
            r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 8))
      (i32.store (i32.const 4) (i32.const 2))
      (i32.store8 (i32.const 8) (i32.const 104))
      (i32.store8 (i32.const 9) (i32.const 105))
      (i32.const 0)))
  (core instance $m (instantiate $M))
  (func (export "f") (result string) (canon lift (core func $m "f") (memory (core memory $m "mem")))))
(assert_return (invoke "f") (str.const "hi"))
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 0xdeadbeef))
      (i32.store (i32.const 4) (i32.const 0))
      (i32.const 0)))
  (core instance $m (instantiate $M))
  (func (export "f") (result string) (canon lift (core func $m "f") (memory (core memory $m "mem")))))
(assert_trap (invoke "f") "string pointer/length out of bounds of memory")
"#,
        ),
        (
            "numerics",
            r#"
(component
  (core module $M
    (func (export "id32") (param i32) (result i32) (local.get 0)))
  (core instance $m (instantiate $M))
  (func (export "i-to-u8") (param "x" u32) (result u8) (canon lift (core func $m "id32")))
  (func (export "i-to-b") (param "x" u32) (result bool) (canon lift (core func $m "id32")))
  (func (export "roundtrip") (param "c" char) (result char) (canon lift (core func $m "id32"))))
(assert_return (invoke "i-to-u8" (u32.const 0xf01)) (u8.const 1))
(assert_return (invoke "i-to-b" (u32.const 2)) (bool.const true))
(assert_return (invoke "roundtrip" (char.const "A")) (char.const "A"))
"#,
        ),
        (
            "floats",
            r#"
(component
  (core module $M
    (func (export "f32id") (param f32) (result f32) (local.get 0))
    (func (export "f64id") (param f64) (result f64) (local.get 0)))
  (core instance $m (instantiate $M))
  (func (export "f32id") (param "x" f32) (result f32) (canon lift (core func $m "f32id")))
  (func (export "f64id") (param "x" f64) (result f64) (canon lift (core func $m "f64id"))))
(assert_return (invoke "f32id" (f32.const 1.5)) (f32.const 1.5))
(assert_return (invoke "f64id" (f64.const -0.25)) (f64.const -0.25))
"#,
        ),
        (
            "tuple-and-list",
            r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "t") (result i32)
      (i32.store (i32.const 0) (i32.const 7))
      (i32.store (i32.const 4) (i32.const 9))
      (i32.const 0))
    (func (export "l") (result i32)
      (i32.store (i32.const 0) (i32.const 8))
      (i32.store (i32.const 4) (i32.const 3))
      (i32.store8 (i32.const 8) (i32.const 1))
      (i32.store8 (i32.const 9) (i32.const 2))
      (i32.store8 (i32.const 10) (i32.const 3))
      (i32.const 0)))
  (core instance $m (instantiate $M))
  (func (export "t") (result (tuple u32 u32)) (canon lift (core func $m "t") (memory (core memory $m "mem"))))
  (func (export "l") (result (list u8)) (canon lift (core func $m "l") (memory (core memory $m "mem")))))
(assert_return (invoke "t") (tuple.const (u32.const 7) (u32.const 9)))
(assert_return (invoke "l") (list.const (u8.const 1) (u8.const 2) (u8.const 3)))
"#,
        ),
        (
            "record-variant-enum-flags",
            r#"
(component
  (type $r (record (field "a" u32) (field "b" bool)))
  (export "r" (type $r))
  (type $v (variant (case "a") (case "b" u32)))
  (export "v" (type $v))
  (type $fl (flags "a" "b" "c"))
  (export "fl" (type $fl))
  (type $en (enum "x" "y" "z"))
  (export "en" (type $en))
  (core module $M
    (memory (export "mem") 1)
    (func (export "fr") (result i32)
      (i32.store (i32.const 0) (i32.const 42))
      (i32.store8 (i32.const 4) (i32.const 1))
      (i32.const 0))
    (func (export "fv") (result i32)
      (i32.store8 (i32.const 0) (i32.const 1))
      (i32.store (i32.const 4) (i32.const 77))
      (i32.const 0))
    (func (export "ff") (result i32) (i32.const 5))
    (func (export "fe") (result i32) (i32.const 2)))
  (core instance $m (instantiate $M))
  (func (export "fr") (result $r) (canon lift (core func $m "fr") (memory (core memory $m "mem"))))
  (func (export "fv") (result $v) (canon lift (core func $m "fv") (memory (core memory $m "mem"))))
  (func (export "ff") (result $fl) (canon lift (core func $m "ff")))
  (func (export "fe") (result $en) (canon lift (core func $m "fe"))))
(assert_return (invoke "fr") (record.const (field "a" u32.const 42) (field "b" bool.const true)))
(assert_return (invoke "fv") (variant.const "b" (u32.const 77)))
(assert_return (invoke "ff") (flags.const "a" "c"))
(assert_return (invoke "fe") (enum.const "z"))
"#,
        ),
        (
            "option-and-result",
            r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "fo") (result i32)
      (i32.store8 (i32.const 0) (i32.const 1))
      (i32.store (i32.const 4) (i32.const 55))
      (i32.const 0))
    (func (export "fr") (result i32)
      (i32.store8 (i32.const 0) (i32.const 0))
      (i32.store (i32.const 4) (i32.const 33))
      (i32.const 0)))
  (core instance $m (instantiate $M))
  (func (export "fo") (result (option u32)) (canon lift (core func $m "fo") (memory (core memory $m "mem"))))
  (func (export "fr") (result (result u32 (error string))) (canon lift (core func $m "fr") (memory (core memory $m "mem")))))
(assert_return (invoke "fo") (option.some (u32.const 55)))
(assert_return (invoke "fr") (result.ok (u32.const 33)))
"#,
        ),
        (
            "string-argument-lowering",
            r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (global $bump (mut i32) (i32.const 1024))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (global.get $bump))
    (func (export "len") (param i32 i32) (result i32) (local.get 1)))
  (core instance $m (instantiate $M))
  (func (export "len") (param "s" string) (result u32)
    (canon lift (core func $m "len") (memory (core memory $m "mem")) (realloc (core func $m "realloc")))))
(assert_return (invoke "len" (str.const "hello")) (u32.const 5))
"#,
        ),
        (
            "definition-and-instance",
            r#"
(component definition $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component instance $i $C)
(assert_return (invoke "answer") (u32.const 42))
(assert_return (invoke $i "answer") (u32.const 42))
"#,
        ),
        (
            "unlinkable-used-import",
            r#"
(assert_unlinkable
  (component
    (import "missing" (func (param "x" u32) (result u32)))
    (core func $lf (canon lower (func 0)))
    (core module $M (import "" "f" (func (param i32) (result i32))))
    (core instance $m (instantiate $M (with "" (instance (export "f" (func $lf)))))))
  "unresolved")
"#,
        ),
    ];

    let mut failures = Vec::new();
    let mut checked = 0usize;
    for (name, text) in cases {
        let path = PathBuf::from(format!("smoke/{name}.wast"));
        let report = common::component_model::run_component_testsuite_case(&path, text).await;
        checked += report.directives_checked;
        failures.extend(report.failures);
    }

    if !failures.is_empty() {
        panic!(
            "upstream_harness_runtime_directives_smoke failures (checked={checked})\n\n{}",
            failures.join("\n\n")
        );
    }
    println!("upstream_harness_runtime_directives_smoke completed: checked={checked}");
}

/// Regression coverage for the four Codex P2 findings on PR #326, all in
/// the shared harness (`tests/common/component_model.rs`):
///
/// 1. Trap matching must not accept a trap raised for a different reason:
///    an unconditional `unreachable` must not satisfy an out-of-bounds
///    `assert_trap` through a shared error category.
/// 2. Payloadless `result` values must keep their discriminant: the
///    runtime represents a payloadless case with an empty-tuple payload,
///    so `(result.ok)` and `(result.err)` are distinct expected values
///    and are accepted as invoke arguments.
/// 3. `register` immediately after a `(component ...)` definition must
///    resolve the instance through the lazy instantiation path instead
///    of failing with "no instance to register".
/// 4. `assert_unlinkable` must not be satisfiable by an encode or
///    compilation failure: only an instantiation (link) failure counts.
///
/// Each case runs through `run_component_testsuite_case` and asserts
/// whether the harness records a failure for it.
#[tokio::test]
async fn upstream_harness_codex_p2_regressions() {
    // (name, wast, expect_failure_recorded)
    let cases: &[(&str, &str, bool)] = &[
        (
            "trap-wrong-cause-must-not-match",
            r#"
(component
  (core module $M
    (func (export "boom") unreachable))
  (core instance $m (instantiate $M))
  (func (export "boom") (canon lift (core func $m "boom"))))
(assert_trap (invoke "boom") "string pointer/length out of bounds of memory")
"#,
            true,
        ),
        (
            "trap-genuine-out-of-bounds-matches",
            r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (result i32)
      (i32.store (i32.const 0) (i32.const 0xdeadbeef))
      (i32.store (i32.const 4) (i32.const 0))
      (i32.const 0)))
  (core instance $m (instantiate $M))
  (func (export "f") (result string) (canon lift (core func $m "f") (memory (core memory $m "mem")))))
(assert_trap (invoke "f") "string pointer/length out of bounds of memory")
"#,
            false,
        ),
        (
            "payloadless-result-return",
            r#"
(component
  (core module $M
    (func (export "ok") (result i32) (i32.const 0))
    (func (export "err") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "ok") (result (result)) (canon lift (core func $m "ok")))
  (func (export "err") (result (result)) (canon lift (core func $m "err"))))
(assert_return (invoke "ok") (result.ok))
(assert_return (invoke "err") (result.err))
"#,
            false,
        ),
        (
            "payloadless-result-wrong-branch-must-not-match",
            r#"
(component
  (core module $M
    (func (export "ok") (result i32) (i32.const 0)))
  (core instance $m (instantiate $M))
  (func (export "ok") (result (result)) (canon lift (core func $m "ok"))))
(assert_return (invoke "ok") (result.err))
"#,
            true,
        ),
        (
            "payloadless-result-argument",
            r#"
(component
  (core module $M
    (func (export "which") (param i32) (result i32) (local.get 0)))
  (core instance $m (instantiate $M))
  (func (export "which") (param "r" (result)) (result u32) (canon lift (core func $m "which"))))
(assert_return (invoke "which" (result.ok)) (u32.const 0))
(assert_return (invoke "which" (result.err)) (u32.const 1))
"#,
            false,
        ),
        (
            "register-right-after-component",
            r#"
(component
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(register "reg")
(assert_return (invoke $reg "answer") (u32.const 42))
"#,
            false,
        ),
        (
            "unlinkable-compile-failure-must-not-satisfy",
            r#"
(assert_unlinkable
  (component
    (core module $M
      (func (result i32) i64.const 0))
    (core instance $m (instantiate $M)))
  "type mismatch")
"#,
            true,
        ),
        (
            "unlinkable-genuine-link-failure",
            r#"
(assert_unlinkable
  (component
    (import "missing" (func (param "x" u32) (result u32)))
    (core func $lf (canon lower (func 0)))
    (core module $M (import "" "f" (func (param i32) (result i32))))
    (core instance $m (instantiate $M (with "" (instance (export "f" (func $lf)))))))
  "unresolved")
"#,
            false,
        ),
    ];

    let mut problems = Vec::new();
    for (name, text, expect_failure) in cases {
        let path = PathBuf::from(format!("regressions/{name}.wast"));
        let report = common::component_model::run_component_testsuite_case(&path, text).await;
        let recorded_failure = !report.failures.is_empty();
        if recorded_failure != *expect_failure {
            problems.push(format!(
                "{name}: expected failure_recorded={expect_failure}, got {recorded_failure} \
                 (checked={}, failures={:?})",
                report.directives_checked, report.failures
            ));
        }
    }

    if !problems.is_empty() {
        panic!(
            "upstream_harness_codex_p2_regressions failures\n\n{}",
            problems.join("\n\n")
        );
    }
    println!("upstream_harness_codex_p2_regressions completed: all cases behaved as expected");
}

/// Regression coverage for the two Codex P2 findings raised on the
/// re-review of PR #326 (head `daec3c5`), again in the shared harness:
///
/// 1. A plain `(component $C ...)` directive names an instance in the
///    reference runner, so `(invoke $C ...)` and `(register "reg" $C)`
///    must resolve it. The harness only recorded `$C` as a definition
///    and searched `instances`/`registered` for named operands, so both
///    directives failed with "no instance named `$C`". The instance is
///    now created lazily on first use and recorded under `$C`. A
///    definition-only name must still not resolve.
/// 2. `(component instance $i)` with no module operand must instantiate
///    the most recent component directive, including a
///    `(component definition ...)` (which never becomes the pending
///    current component), and must be repeatable instead of consuming
///    the pending program with `take()`.
///
/// Each case runs through `run_component_testsuite_case` and asserts
/// whether the harness records a failure for it.
#[tokio::test]
async fn upstream_harness_codex_p2_round2_regressions() {
    // (name, wast, expect_failure_recorded)
    let cases: &[(&str, &str, bool)] = &[
        (
            "named-invoke-after-plain-component",
            r#"
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 42))
"#,
            false,
        ),
        (
            "named-register-after-plain-component",
            r#"
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(register "reg" $C)
(assert_return (invoke $reg "answer") (u32.const 42))
"#,
            false,
        ),
        (
            "named-invoke-of-earlier-plain-component",
            r#"
(component $A
  (core module $M
    (func (export "answer") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component $B
  (core module $M
    (func (export "answer") (result i32) (i32.const 2)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $A "answer") (u32.const 1))
(assert_return (invoke $B "answer") (u32.const 2))
(assert_return (invoke "answer") (u32.const 2))
"#,
            false,
        ),
        (
            "named-invoke-of-definition-only-must-fail",
            r#"
(component definition $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 42))
"#,
            true,
        ),
        (
            "implicit-instance-after-definition",
            r#"
(component definition $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component instance $i)
(assert_return (invoke $i "answer") (u32.const 42))
"#,
            false,
        ),
        (
            "implicit-instance-repeated-after-plain-component",
            r#"
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component instance $i)
(component instance $j)
(assert_return (invoke $i "answer") (u32.const 42))
(assert_return (invoke $j "answer") (u32.const 42))
"#,
            false,
        ),
        (
            "implicit-instance-repeated-after-definition",
            r#"
(component definition $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 42)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component instance $i)
(component instance $j)
(assert_return (invoke $i "answer") (u32.const 42))
(assert_return (invoke $j "answer") (u32.const 42))
"#,
            false,
        ),
        (
            "implicit-instance-uses-latest-definition",
            r#"
(component $A
  (core module $M
    (func (export "answer") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component definition $B
  (core module $M
    (func (export "answer") (result i32) (i32.const 2)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component instance $i)
(assert_return (invoke $i "answer") (u32.const 2))
"#,
            false,
        ),
    ];

    let mut problems = Vec::new();
    for (name, text, expect_failure) in cases {
        let path = PathBuf::from(format!("regressions-round2/{name}.wast"));
        let report = common::component_model::run_component_testsuite_case(&path, text).await;
        let recorded_failure = !report.failures.is_empty();
        if recorded_failure != *expect_failure {
            problems.push(format!(
                "{name}: expected failure_recorded={expect_failure}, got {recorded_failure} \
                 (checked={}, failures={:?})",
                report.directives_checked, report.failures
            ));
        }
    }

    if !problems.is_empty() {
        panic!(
            "upstream_harness_codex_p2_round2_regressions failures\n\n{}",
            problems.join("\n\n")
        );
    }
    println!(
        "upstream_harness_codex_p2_round2_regressions completed: all cases behaved as expected"
    );
}

/// Regression coverage for the Codex P2 finding on the re-review of
/// PR #326 (head 713563b), in the shared harness
/// (`tests/common/component_model.rs`): redeclaring a plain component
/// under the same `$id` must invalidate the instance cached under that
/// name. Before the fix, `handle_module` updated `definitions` and
/// `pending` for the new declaration but left the first instance in
/// `instances`, so named `invoke`/`register` resolved the previous
/// component while an unnamed `invoke` instantiated the new one —
/// assertions could exercise the wrong component.
///
/// Each case runs through `run_component_testsuite_case` and asserts
/// whether the harness records a failure for it.
#[tokio::test]
async fn upstream_harness_codex_p2_round3_regressions() {
    // (name, wast, expect_failure_recorded)
    let cases: &[(&str, &str, bool)] = &[
        (
            "redeclared-plain-component-named-invoke-uses-new-instance",
            r#"
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 1))
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 2)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 2))
(assert_return (invoke "answer") (u32.const 2))
"#,
            false,
        ),
        (
            "redeclared-plain-component-named-register-uses-new-instance",
            r#"
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 1))
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 2)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(register "reg" $C)
(assert_return (invoke $reg "answer") (u32.const 2))
"#,
            false,
        ),
        (
            "redeclared-plain-component-after-fallback-invoke-uses-new-instance",
            r#"
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(component $D
  (core module $M
    (func (export "answer") (result i32) (i32.const 9)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 1))
(component $C
  (core module $M
    (func (export "answer") (result i32) (i32.const 2)))
  (core instance $m (instantiate $M))
  (func (export "answer") (result u32) (canon lift (core func $m "answer"))))
(assert_return (invoke $C "answer") (u32.const 2))
"#,
            false,
        ),
    ];

    let mut problems = Vec::new();
    for (name, text, expect_failure) in cases {
        let path = PathBuf::from(format!("regressions-round3/{name}.wast"));
        let report = common::component_model::run_component_testsuite_case(&path, text).await;
        let recorded_failure = !report.failures.is_empty();
        if recorded_failure != *expect_failure {
            problems.push(format!(
                "{name}: expected failure_recorded={expect_failure}, got {recorded_failure} \
                 (checked={}, failures={:?})",
                report.directives_checked, report.failures
            ));
        }
    }

    if !problems.is_empty() {
        panic!(
            "upstream_harness_codex_p2_round3_regressions failures\n\n{}",
            problems.join("\n\n")
        );
    }
    println!(
        "upstream_harness_codex_p2_round3_regressions completed: all cases behaved as expected"
    );
}
