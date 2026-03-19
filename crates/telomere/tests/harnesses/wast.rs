use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use libtest_mimic::{Arguments, Failed, Trial};
use tokio::runtime::Builder;

#[path = "../common/mod.rs"]
mod common;
use common::run_wast;

// `labels` / `names` are existing wast parser limitations.
const ROOT_SKIP: &[&str] = &["labels", "names"];
const PROPOSAL_ALLOWLIST: &[&str] = &["threads", "tail-call"];
const PROPOSAL_ONLY: &[&str] = &[
    "proposals/threads/atomic",
    "proposals/tail-call/return_call",
    "proposals/tail-call/return_call_indirect",
];
#[derive(Clone)]
struct Case {
    name: String,
    fixture: PathBuf,
}

fn main() -> ExitCode {
    let args = Arguments::from_args();
    let tests = collect_trials();
    libtest_mimic::run(&args, tests).exit_code()
}

fn collect_trials() -> Vec<Trial> {
    let suite_dir = resolve_suite_dir();
    let (mut cases, root_names) = collect_root_cases(&suite_dir);
    cases.extend(collect_proposal_cases(&suite_dir, &root_names));
    cases.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.fixture.cmp(&right.fixture))
    });

    cases
        .into_iter()
        .map(|case| {
            let fixture = case.fixture.clone();
            Trial::test(case.name, move || run_case(fixture))
        })
        .collect()
}

fn resolve_suite_dir() -> PathBuf {
    let suite_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wasm-testsuite");
    assert!(
        suite_dir.is_dir(),
        "missing wasm testsuite submodule at {}. Run `git submodule update --init --recursive` from the repository root.",
        suite_dir.display()
    );
    suite_dir
}

fn run_case(fixture: PathBuf) -> Result<(), Failed> {
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            format!(
                "failed to build tokio runtime for {}: {err}",
                fixture.display()
            )
        })?;

    runtime.block_on(async move {
        let wast = fs::read_to_string(&fixture)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", fixture.display()));
        run_wast(&wast).await;
    });

    Ok(())
}

fn collect_root_cases(suite_dir: &Path) -> (Vec<Case>, HashSet<String>) {
    let mut root_names = HashSet::new();
    let mut cases = Vec::new();

    for entry in sorted_dir_entries(suite_dir) {
        let path = entry.path();
        if !is_wast_file(&path) {
            continue;
        }

        let stem = file_stem(&path);
        if ROOT_SKIP.contains(&stem.as_str()) {
            continue;
        }

        root_names.insert(stem.clone());
        cases.push(Case {
            name: normalize_name(&stem),
            fixture: path,
        });
    }

    (cases, root_names)
}

fn collect_proposal_cases(suite_dir: &Path, root_names: &HashSet<String>) -> Vec<Case> {
    let mut cases = Vec::new();

    for family in PROPOSAL_ALLOWLIST {
        let family_dir = suite_dir.join("proposals").join(family);
        if family_dir.is_dir() {
            collect_proposal_cases_in_dir(suite_dir, &family_dir, root_names, &mut cases);
        }
    }

    cases
}

fn collect_proposal_cases_in_dir(
    suite_dir: &Path,
    dir: &Path,
    root_names: &HashSet<String>,
    cases: &mut Vec<Case>,
) {
    for entry in sorted_dir_entries(dir) {
        let path = entry.path();
        if path.is_dir() {
            collect_proposal_cases_in_dir(suite_dir, &path, root_names, cases);
            continue;
        }

        if !is_wast_file(&path) {
            continue;
        }

        let rel = relative_fixture_key(suite_dir, &path);
        let stem = file_stem(&path);
        if root_names.contains(&stem) || PROPOSAL_ONLY.contains(&rel.as_str()) {
            cases.push(Case {
                name: normalize_name(&rel),
                fixture: path,
            });
        }
    }
}

fn sorted_dir_entries(dir: &Path) -> Vec<fs::DirEntry> {
    let mut entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read directory {}: {err}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|err| panic!("failed to read directory entry: {err}")))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path());
    entries
}

fn is_wast_file(path: &Path) -> bool {
    path.is_file() && path.extension() == Some(OsStr::new("wast"))
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("non-utf8 or missing fixture name: {}", path.display()))
        .to_owned()
}

fn relative_fixture_key(suite_dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(suite_dir).unwrap_or_else(|err| {
        panic!(
            "failed to strip suite prefix from {}: {err}",
            path.display()
        )
    });
    let rel = rel.to_string_lossy();
    rel.trim_end_matches(".wast").to_owned()
}

fn normalize_name(rel_key: &str) -> String {
    let rel_key = rel_key.strip_prefix("proposals/").unwrap_or(rel_key);
    rel_key
        .chars()
        .map(|c| if c == '/' || c == '-' { '_' } else { c })
        .collect()
}
