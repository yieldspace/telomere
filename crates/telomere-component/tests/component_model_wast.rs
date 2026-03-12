use std::path::{Path, PathBuf};

mod common;

fn testsuite_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/component_model_testsuite")
}

fn suite_files(root: &Path) -> Vec<PathBuf> {
    let mut files = std::fs::read_dir(root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", root.display()))
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "wast"))
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn component_model_testsuite() {
    let root = testsuite_dir();
    let files = suite_files(&root);
    assert!(
        !files.is_empty(),
        "expected at least one testsuite file in {}",
        root.display()
    );

    let mut checked = 0usize;
    let mut failures = Vec::new();

    for path in files {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let report = common::component_model::run_component_testsuite_case(&path, &text);
        checked += report.directives_checked;
        failures.extend(report.failures);
    }

    if !failures.is_empty() {
        panic!(
            "component_model_testsuite failures (checked={})\n\n{}",
            checked,
            failures.join("\n\n")
        );
    }

    println!("component_model_testsuite completed: checked={checked}");
}
