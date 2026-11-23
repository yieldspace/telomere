//! Trap-diagnostics conformance fixtures (#211).
//!
//! A corpus of small trapping modules lives in `tests/trap_fixtures/`: each
//! `<name>.wat` pairs with a `<name>.expected` data file describing the exact
//! [`TrapInfo`] the interpreter must publish (trap kind, retained frames as
//! funcidx + name, and the pc where one is defined). The runner executes
//! every fixture on the interpreter and asserts the trace matches the file,
//! then — when the `jit` feature is compiled in and the target supports it —
//! executes it again on a JIT-enabled store and asserts the engines agree:
//! same kind, same frame sequence, and the same pc wherever the JIT claims
//! one. Since #209, a trap raised at a native JIT trap site must claim the
//! trapping frame's pc exactly (see `JIT_PC_REQUIRED_FIXTURES`); traps
//! raised by Rust stubs or entry checks carry no site and claim none.
//!
//! Expectations live in data files, not code, so the trap-message work in
//! #131 can extend the same corpus. Regenerate a file after an intentional
//! trace change with `TELOMERE_TRAP_FIXTURES_BLESS=1` and review the diff
//! like any other golden change; CI never sets it.
//!
//! The JIT pass is fail-closed in the #160 style: with
//! `TELOMERE_TRAP_FIXTURES_REQUIRE_JIT=1` (set by the CI job that runs this
//! target with `--features jit`), a missing JIT is a failure, not a skip.
//! Fixtures the JIT cannot compile at all are listed, one per line with a
//! reason, in `tests/trap_fixtures/jit-skip.list`; a fixture that fails
//! under the JIT without an entry fails this test.

mod common;

use common::instantiate_wat;
use std::{collections::BTreeSet, fs, path::PathBuf, sync::Mutex};
use telomere::{
    component_support::runtime::run_core_export_sync_reentrant,
    link_host_function_with_function_idx, run_module_function, HostContext, InstanceHandle,
    Registry, ResultValue, RuntimeConfig, Store, StoreState, TrapFrameKind, TrapInfo, VMResult,
    WasmValue,
};

#[cfg(feature = "jit")]
use telomere::JitConfig;

/// Fixtures the corpus must always contain: every guest-reachable trap kind
/// from #211 plus the four structural cases. Deleting or renaming one of
/// these fails `corpus_coverage_and_skip_list_are_current`.
const REQUIRED_FIXTURES: &[&str] = &[
    // Trap kinds.
    "unreachable",
    "oob_load",
    "oob_store",
    "div_zero",
    "div_overflow",
    "rem_zero",
    "indirect_type_mismatch",
    "table_uninitialized",
    "unaligned_atomic",
    "stack_overflow",
    // Structural cases.
    "deep_chain",
    "tail_call_chain",
    "host_frame_chain",
    "recursion_truncated",
];

/// Fixtures whose trap is raised at a native JIT trap site (#209): on
/// the JIT pass the trapping frame must report the expected pc, not
/// merely agree when it claims one. Fixtures absent here trap through
/// Rust stubs or entry checks — call_indirect type checks, table
/// access, host reentry, stack-depth guards — whose exits carry no
/// trap site, so their trapping frame keeps reporting no pc.
const JIT_PC_REQUIRED_FIXTURES: &[&str] = &[
    "deep_chain",
    "div_overflow",
    "div_zero",
    "oob_load",
    "oob_store",
    "rem_zero",
    "unreachable",
];

fn fixtures_dir() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/trap_fixtures")
}

fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixtures_dir())
        .expect("trap_fixtures directory must exist")
        .filter_map(|entry| {
            let path = entry.expect("fixture dir entry").path();
            (path.extension().is_some_and(|ext| ext == "wat"))
                .then(|| path.file_stem().unwrap().to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DepthSpec {
    /// Absolute depth, used for the innermost retained frames.
    Absolute(u32),
    /// Index into the outermost retained segment of a truncated trace;
    /// the absolute depth is `total_frames - tail_len + index` for the
    /// run being checked, because engines overflow at different depths.
    Tail(u32),
}

#[derive(Clone, Debug)]
struct ExpectedFrame {
    depth: DepthSpec,
    funcidx: Option<u32>,
    func_name: Option<String>,
    module_name: Option<String>,
    pc: Option<u32>,
    kind: TrapFrameKind,
}

#[derive(Clone, Debug)]
struct Expected {
    kind_label: String,
    truncated: bool,
    total_frames: u32,
    frames: Vec<ExpectedFrame>,
}

fn none_or<T: std::str::FromStr>(text: &str) -> Result<Option<T>, String> {
    if text == "-" {
        Ok(None)
    } else {
        text.parse::<T>()
            .map(Some)
            .map_err(|_| format!("unparsable value `{text}`"))
    }
}

fn parse_frame_line(line: &str, tail: bool) -> Result<ExpectedFrame, String> {
    // `frame 3: func=2 name=f2 module=deep_chain kind=wasm pc=2`
    let mut parts = line.splitn(2, ':');
    let head = parts.next().unwrap_or_default();
    let mut head_words = head.split_whitespace();
    head_words.next(); // "frame" / "tail"
    let index: u32 = head_words
        .next()
        .and_then(|word| word.parse().ok())
        .ok_or_else(|| format!("missing frame index in `{line}`"))?;
    let mut frame = ExpectedFrame {
        depth: if tail {
            DepthSpec::Tail(index)
        } else {
            DepthSpec::Absolute(index)
        },
        funcidx: None,
        func_name: None,
        module_name: None,
        pc: None,
        kind: TrapFrameKind::Wasm,
    };
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for token in parts.next().unwrap_or_default().split_whitespace() {
        let (key, value) = token
            .split_once('=')
            .ok_or_else(|| format!("malformed token `{token}` in `{line}`"))?;
        if !seen.insert(key) {
            return Err(format!("duplicate key `{key}` in `{line}`"));
        }
        match key {
            "func" => frame.funcidx = none_or::<u32>(value)?,
            "name" => frame.func_name = none_or::<String>(value)?,
            "module" => frame.module_name = none_or::<String>(value)?,
            "pc" => frame.pc = none_or::<u32>(value)?,
            "kind" => {
                frame.kind = match value {
                    "wasm" => TrapFrameKind::Wasm,
                    "host" => TrapFrameKind::Host,
                    "async-host" => TrapFrameKind::AsyncHost,
                    "unresolved" => TrapFrameKind::Unresolved,
                    other => return Err(format!("unknown frame kind `{other}` in `{line}`")),
                }
            }
            other => return Err(format!("unknown key `{other}` in `{line}`")),
        }
    }
    if seen != BTreeSet::from(["func", "name", "module", "kind", "pc"]) {
        return Err(format!(
            "frame line must carry func/name/module/kind/pc keys: `{line}`"
        ));
    }
    Ok(frame)
}

fn parse_expected(text: &str) -> Result<Expected, String> {
    let mut kind_label = None;
    let mut truncated = None;
    let mut total_frames = None;
    let mut frames = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("kind:") {
            kind_label = Some(rest.trim().to_owned());
        } else if let Some(rest) = line.strip_prefix("truncated:") {
            truncated = rest.trim().parse::<bool>().ok();
            if truncated.is_none() {
                return Err(format!("bad truncated value in `{line}`"));
            }
        } else if let Some(rest) = line.strip_prefix("total-frames:") {
            total_frames = rest.trim().parse::<u32>().ok();
            if total_frames.is_none() {
                return Err(format!("bad total-frames value in `{line}`"));
            }
        } else if line.starts_with("frame ") {
            frames.push(parse_frame_line(line, false)?);
        } else if line.starts_with("tail ") {
            frames.push(parse_frame_line(line, true)?);
        } else {
            return Err(format!("unrecognised expectation line `{line}`"));
        }
    }
    Ok(Expected {
        kind_label: kind_label.ok_or("missing `kind:` line")?,
        truncated: truncated.ok_or("missing `truncated:` line")?,
        total_frames: total_frames.ok_or("missing `total-frames:` line")?,
        frames,
    })
}

fn load_expected(name: &str) -> Result<Expected, String> {
    let path = fixtures_dir().join(format!("{name}.expected"));
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse_expected(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn frame_kind_label(kind: TrapFrameKind) -> &'static str {
    match kind {
        TrapFrameKind::Wasm => "wasm",
        TrapFrameKind::Host => "host",
        TrapFrameKind::AsyncHost => "async-host",
        TrapFrameKind::Unresolved => "unresolved",
        _ => "unknown",
    }
}

fn opt<T: std::fmt::Display>(value: &Option<T>) -> String {
    value
        .as_ref()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_owned())
}

/// Serialises an interpreter trace into the `.expected` data format. Used
/// only by bless mode; CI comparisons go through [`compare`].
fn serialise_expected(info: &TrapInfo) -> String {
    let mut out = String::new();
    out.push_str(&format!("kind: {}\n", info.kind.as_str()));
    out.push_str(&format!("truncated: {}\n", info.truncated));
    out.push_str(&format!("total-frames: {}\n", info.total_frames));
    // A truncated trace retains the innermost 48 frames plus the outermost
    // 16; the outermost segment is written with `tail` indices because its
    // absolute depths depend on the engine-specific overflow depth.
    let tail_len = if info.truncated { 16 } else { 0 };
    let head_len = info.frames.len() - tail_len;
    for (position, frame) in info.frames.iter().enumerate() {
        let (tag, index) = if position < head_len {
            ("frame", frame.depth)
        } else {
            ("tail", (position - head_len) as u32)
        };
        out.push_str(&format!(
            "{tag} {index}: func={} name={} module={} kind={} pc={}\n",
            opt(&frame.funcidx),
            opt(&frame.func_name),
            opt(&frame.module_name),
            frame_kind_label(frame.kind),
            opt(&frame.pc_index),
        ));
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Engine {
    Interpreter,
    Jit,
}

/// Compares one published trace against the fixture expectation. The
/// interpreter must match exactly, including every pc. The JIT must agree
/// on kind, truncation, and the full frame identity sequence, and on the pc
/// of every frame where it claims one; it claims none for a native trapping
/// frame today (see the module docs), and its overflow depth — hence the
/// total frame count of a truncated trace — is its own.
/// Builds the head-segment expectation for one candidate rotation of a
/// truncated trace's innermost segment (see [`compare`]). Position `i`
/// takes its identity from expected frame `(i + rotation) % head_len`;
/// positions that wrap past the recorded window take the pc of the first
/// caller frame with the same identity, because in the untruncated
/// sequence that position is a caller, not the trapping frame.
fn rotated_head(expected: &Expected, head_len: usize, rotation: usize) -> Vec<ExpectedFrame> {
    (0..head_len)
        .map(|i| {
            let source = &expected.frames[(i + rotation) % head_len];
            let mut frame = source.clone();
            frame.depth = DepthSpec::Absolute(i as u32);
            if i + rotation >= head_len {
                frame.pc = expected.frames[1..head_len]
                    .iter()
                    .find(|caller| {
                        caller.funcidx == source.funcidx && caller.func_name == source.func_name
                    })
                    .map(|caller| caller.pc)
                    .unwrap_or(source.pc);
            }
            frame
        })
        .collect()
}

fn compare_frame(
    frame: &telomere::TrapFrame,
    want: &ExpectedFrame,
    want_depth: u32,
    engine: Engine,
    jit_pc_required: bool,
    problems: &mut Vec<String>,
) {
    if frame.funcidx != want.funcidx {
        problems.push(format!(
            "frame {want_depth}: funcidx got {:?}, expected {:?}",
            frame.funcidx, want.funcidx
        ));
    }
    if frame.func_name != want.func_name {
        problems.push(format!(
            "frame {want_depth}: func name got {:?}, expected {:?}",
            frame.func_name, want.func_name
        ));
    }
    if frame.module_name != want.module_name {
        problems.push(format!(
            "frame {want_depth}: module name got {:?}, expected {:?}",
            frame.module_name, want.module_name
        ));
    }
    if frame.kind != want.kind {
        problems.push(format!(
            "frame {want_depth}: kind got {:?}, expected {:?}",
            frame.kind, want.kind
        ));
    }
    match engine {
        Engine::Interpreter => {
            if frame.pc_index != want.pc {
                problems.push(format!(
                    "frame {want_depth}: pc got {:?}, expected {:?}",
                    frame.pc_index, want.pc
                ));
            }
        }
        Engine::Jit => {
            if let Some(pc) = frame.pc_index {
                if Some(pc) != want.pc {
                    problems.push(format!(
                        "frame {want_depth}: JIT pc {pc} disagrees with expected {:?}",
                        want.pc
                    ));
                }
            } else if jit_pc_required && frame.depth == 0 && frame.kind == TrapFrameKind::Wasm {
                problems.push(format!(
                    "frame {want_depth}: JIT claims no pc for a trap raised at a \
                     native trap site; #209 requires the trapping frame to report \
                     the expected pc {:?}",
                    want.pc
                ));
            }
        }
    }
}

/// Compares one published trace against the fixture expectation. The
/// interpreter must match exactly, including every pc. The JIT must agree
/// on kind, truncation, and the full frame identity sequence, and on the pc
/// of every frame where it claims one; for fixtures whose trap is raised
/// at a native trap site (`jit_pc_required`, #209) it must claim the
/// trapping frame's pc. Its overflow depth — hence the total frame count
/// of a truncated trace — is its own.
fn compare(
    info: &TrapInfo,
    expected: &Expected,
    engine: Engine,
    jit_pc_required: bool,
) -> Result<(), String> {
    let mut problems = Vec::new();
    if info.kind.as_str() != expected.kind_label {
        problems.push(format!(
            "kind: got {}, expected {}",
            info.kind.as_str(),
            expected.kind_label
        ));
    }
    if info.truncated != expected.truncated {
        problems.push(format!(
            "truncated: got {}, expected {}",
            info.truncated, expected.truncated
        ));
    }
    match engine {
        Engine::Interpreter => {
            if info.total_frames != expected.total_frames {
                problems.push(format!(
                    "total-frames: got {}, expected {}",
                    info.total_frames, expected.total_frames
                ));
            }
        }
        Engine::Jit => {
            if expected.truncated {
                if info.total_frames as usize <= info.frames.len() {
                    problems.push(format!(
                        "total-frames: got {} but the trace claims truncation with {} retained frames",
                        info.total_frames,
                        info.frames.len()
                    ));
                }
            } else if info.total_frames != expected.total_frames {
                problems.push(format!(
                    "total-frames: got {}, expected {}",
                    info.total_frames, expected.total_frames
                ));
            }
        }
    }
    if info.frames.len() != expected.frames.len() {
        problems.push(format!(
            "retained frames: got {}, expected {}",
            info.frames.len(),
            expected.frames.len()
        ));
        return Err(problems.join("; "));
    }
    let tail_len = expected
        .frames
        .iter()
        .filter(|frame| matches!(frame.depth, DepthSpec::Tail(_)))
        .count();
    let head_len = expected.frames.len() - tail_len;

    // For a truncated trace under the JIT, the innermost segment may be
    // phase-shifted against the interpreter expectation: which function of
    // a recursion cycle sits at depth 0 depends on the parity of the
    // engine-specific overflow depth (the interpreter walked 3196 frames
    // for the corpus fixtures, the JIT 257). Identities and pcs are still
    // compared exactly, at the aligned rotation; only the rotation of a
    // periodic innermost sequence is free. The outermost segment needs no
    // such allowance: anchored at the entry frame, it is identical.
    let head: Vec<ExpectedFrame> = if engine == Engine::Jit && expected.truncated && head_len > 0 {
        let mut chosen = None;
        for rotation in 0..head_len {
            let candidate = rotated_head(expected, head_len, rotation);
            let mut trial = Vec::new();
            for (frame, want) in info.frames[..head_len].iter().zip(&candidate) {
                compare_frame(
                    frame,
                    want,
                    frame.depth,
                    engine,
                    jit_pc_required,
                    &mut trial,
                );
            }
            if trial.is_empty() {
                chosen = Some(candidate);
                break;
            }
        }
        match chosen {
            Some(candidate) => candidate,
            None => {
                problems
                    .push("innermost frames match the expected sequence at no rotation".to_owned());
                rotated_head(expected, head_len, 0)
            }
        }
    } else {
        expected.frames[..head_len].to_vec()
    };

    for (position, frame) in info.frames.iter().enumerate() {
        if position < head_len {
            if frame.depth != position as u32 {
                problems.push(format!(
                    "frame at position {position}: got depth {}",
                    frame.depth
                ));
            }
            compare_frame(
                frame,
                &head[position],
                frame.depth,
                engine,
                jit_pc_required,
                &mut problems,
            );
        } else {
            let want = &expected.frames[position];
            let DepthSpec::Tail(index) = want.depth else {
                unreachable!("frames past the head segment are tail frames");
            };
            let want_depth = info
                .total_frames
                .checked_sub(tail_len as u32)
                .and_then(|base| base.checked_add(index))
                .ok_or_else(|| "tail depth underflow".to_owned())?;
            if frame.depth != want_depth {
                problems.push(format!(
                    "tail frame {index}: got depth {}, expected {want_depth}",
                    frame.depth
                ));
            }
            compare_frame(
                frame,
                want,
                want_depth,
                engine,
                jit_pc_required,
                &mut problems,
            );
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

#[derive(Default)]
struct Directives {
    requires_threads: bool,
    host_reentry: bool,
}

fn parse_directives(wat: &str) -> Directives {
    let mut directives = Directives::default();
    for line in wat.lines() {
        let Some(comment) = line.trim().strip_prefix(";;") else {
            continue;
        };
        let comment = comment.trim();
        if let Some(rest) = comment.strip_prefix("fixture-requires:") {
            if rest.trim() == "threads" {
                directives.requires_threads = true;
            }
        }
        if let Some(rest) = comment.strip_prefix("fixture-mode:") {
            if rest.trim() == "host-reentry" {
                directives.host_reentry = true;
            }
        }
    }
    directives
}

fn interpreter_store() -> Store {
    Store::new()
}

fn jit_store() -> Store {
    #[cfg(feature = "jit")]
    {
        let mut config = RuntimeConfig::default();
        config.jit = JitConfig {
            enabled: true,
            ..JitConfig::default()
        };
        Store::new_with_runtime_config(config)
    }
    #[cfg(not(feature = "jit"))]
    {
        Store::new()
    }
}

async fn run_plain(wat: &str, store: &Store) -> Option<TrapInfo> {
    let registry = Registry::new();
    let instance = instantiate_wat(wat, store, &registry).await;
    let _ = run_module_function(&instance, store, "run", &ResultValue::new(vec![])).await;
    store.take_last_trap()
}

struct HostFixtureState {
    instance: Mutex<Option<InstanceHandle>>,
}

fn propagating_host(cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
    let state = unsafe { cx.store().state.get::<HostFixtureState>() }
        .expect("host fixture requires HostFixtureState in StoreState");
    let instance = state
        .instance
        .lock()
        .expect("host fixture instance mutex must not be poisoned")
        .clone()
        .expect("host fixture instance must be installed before guest execution");
    let inner =
        run_core_export_sync_reentrant(&instance, cx.store(), "inner", &ResultValue::new(vec![]))
            .expect("synchronous reentry itself must be supported");
    assert!(
        matches!(inner, VMResult::Unreachable),
        "the inner export of a host fixture must trap"
    );
    VMResult::Unreachable
}

async fn run_host_reentry(wat: &str, jit: bool) -> Option<TrapInfo> {
    let state = Box::leak(Box::new(HostFixtureState {
        instance: Mutex::new(None),
    }));
    let store = if jit {
        #[cfg(feature = "jit")]
        {
            let mut config = RuntimeConfig::default();
            config.jit = JitConfig {
                enabled: true,
                ..JitConfig::default()
            };
            Store::new_with_state_and_runtime_config(StoreState::from_static(state), config)
        }
        #[cfg(not(feature = "jit"))]
        {
            Store::new_with_state(StoreState::from_static(state))
        }
    } else {
        Store::new_with_state(StoreState::from_static(state))
    };
    let registry = Registry::new();
    let instance = instantiate_wat(wat, &store, &registry).await;
    *state
        .instance
        .lock()
        .expect("host fixture instance mutex must not be poisoned") = Some(instance.clone());
    link_host_function_with_function_idx(&instance, 0, propagating_host, &store);
    let _ = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    store.take_last_trap()
}

async fn run_fixture(name: &str, wat: &str, directives: &Directives, engine: Engine) -> TrapInfo {
    let info = if directives.host_reentry {
        run_host_reentry(wat, engine == Engine::Jit).await
    } else {
        match engine {
            Engine::Interpreter => run_plain(wat, &interpreter_store()).await,
            Engine::Jit => run_plain(wat, &jit_store()).await,
        }
    };
    info.unwrap_or_else(|| panic!("fixture {name} produced no trap information on {engine:?}"))
}

fn jit_skip_list() -> Result<Vec<(String, String)>, String> {
    let path = fixtures_dir().join("jit-skip.list");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let mut entries = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, reason) = line
            .split_once(':')
            .ok_or_else(|| format!("skip-list line lacks `name: reason` shape: `{line}`"))?;
        let (name, reason) = (name.trim().to_owned(), reason.trim().to_owned());
        if reason.is_empty() {
            return Err(format!("skip-list entry for {name} has no reason"));
        }
        entries.push((name, reason));
    }
    Ok(entries)
}

fn bless_enabled() -> bool {
    std::env::var_os("TELOMERE_TRAP_FIXTURES_BLESS").is_some()
}

fn require_jit() -> bool {
    std::env::var_os("TELOMERE_TRAP_FIXTURES_REQUIRE_JIT").is_some()
}

/// Whether the JIT pass should run in this build. With
/// `TELOMERE_TRAP_FIXTURES_REQUIRE_JIT=1` anything that prevents it is a
/// hard failure (fail-closed, #160 style); otherwise the pass is skipped.
fn jit_pass_active() -> bool {
    #[cfg(feature = "jit")]
    {
        if telomere::jit_supported() {
            return true;
        }
        assert!(
            !require_jit(),
            "TELOMERE_TRAP_FIXTURES_REQUIRE_JIT requires JIT support on this target"
        );
        false
    }
    #[cfg(not(feature = "jit"))]
    {
        assert!(
            !require_jit(),
            "TELOMERE_TRAP_FIXTURES_REQUIRE_JIT requires the telomere `jit` feature"
        );
        false
    }
}

#[tokio::test]
async fn corpus_matches_expectations_on_interpreter_and_jit() {
    let skip_list = jit_skip_list().expect("jit-skip.list must parse");
    let skipped: BTreeSet<&str> = skip_list.iter().map(|(name, _)| name.as_str()).collect();
    let jit_active = jit_pass_active();
    let mut failures: Vec<String> = Vec::new();

    for name in fixture_names() {
        let wat_path = fixtures_dir().join(format!("{name}.wat"));
        let wat = fs::read_to_string(&wat_path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", wat_path.display()));
        let directives = parse_directives(&wat);
        if directives.requires_threads && !cfg!(feature = "threads") {
            // The corpus still runs in full wherever threads is compiled
            // in (the workspace and JIT CI jobs); feature-reduced jobs
            // skip only the fixtures that need the missing feature.
            eprintln!("fixture {name}: skipped, requires the `threads` feature");
            continue;
        }

        let interp_info = run_fixture(&name, &wat, &directives, Engine::Interpreter).await;
        if bless_enabled() {
            let path = fixtures_dir().join(format!("{name}.expected"));
            let header = format!(
                "# Trap-diagnostics expectation for `{name}`. Format: see README.md in this directory.\n"
            );
            fs::write(&path, header + &serialise_expected(&interp_info))
                .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
            continue;
        }

        let expected = match load_expected(&name) {
            Ok(expected) => expected,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        if let Err(error) = compare(&interp_info, &expected, Engine::Interpreter, false) {
            failures.push(format!("{name} [interpreter]: {error}"));
        }

        if jit_active && !skipped.contains(name.as_str()) {
            let jit_info = run_fixture(&name, &wat, &directives, Engine::Jit).await;
            if let Err(error) = compare(
                &jit_info,
                &expected,
                Engine::Jit,
                JIT_PC_REQUIRED_FIXTURES.contains(&name.as_str()),
            ) {
                failures.push(format!(
                    "{name} [jit]: {error} (if the JIT cannot compile this fixture at all, \
                     add it to jit-skip.list with a reason instead of weakening the check)"
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "trap fixture mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn corpus_coverage_and_skip_list_are_current() {
    let names = fixture_names();
    let name_set: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    for required in REQUIRED_FIXTURES {
        assert!(
            name_set.contains(required),
            "required fixture `{required}` is missing from the corpus"
        );
    }
    let readme = fs::read_to_string(fixtures_dir().join("README.md"))
        .expect("trap_fixtures/README.md must exist");
    for name in &names {
        assert!(
            fixtures_dir().join(format!("{name}.expected")).is_file(),
            "fixture `{name}` has no .expected file"
        );
        assert!(
            readme.contains(name.as_str()),
            "fixture `{name}` is missing from the README coverage table"
        );
    }
    let skip_list = jit_skip_list().expect("jit-skip.list must parse");
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (name, _) in &skip_list {
        assert!(
            name_set.contains(name.as_str()),
            "jit-skip.list names unknown fixture `{name}`"
        );
        assert!(
            seen.insert(name.as_str()),
            "jit-skip.list lists `{name}` twice"
        );
    }
}

/// The mutation half of the acceptance criteria, kept as a permanent test:
/// an expectation whose pc is off by one must be rejected, so a corrupted
/// or stale golden cannot pass silently.
#[tokio::test]
async fn mutated_expectation_is_rejected() {
    let name = "div_zero";
    let wat = fs::read_to_string(fixtures_dir().join(format!("{name}.wat")))
        .expect("div_zero fixture must exist");
    let directives = parse_directives(&wat);
    let info = run_fixture(name, &wat, &directives, Engine::Interpreter).await;
    let mut expected = load_expected(name).expect("div_zero expectation must parse");
    compare(&info, &expected, Engine::Interpreter, false)
        .expect("unmutated expectation must match before mutation");
    let frame = expected
        .frames
        .iter_mut()
        .find(|frame| frame.pc.is_some())
        .expect("div_zero expectation must carry a pc to mutate");
    frame.pc = frame.pc.map(|pc| pc + 1);
    assert!(
        compare(&info, &expected, Engine::Interpreter, false).is_err(),
        "an off-by-one pc in a fixture expectation must fail the comparison"
    );
}
