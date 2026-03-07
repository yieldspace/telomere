use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod common;

use common::component_model::{run_component_upstream_case, UpstreamCaseMode};

#[derive(Debug, Clone)]
struct ManifestEntry {
    kind: String,
    relative_path: String,
    mode: String,
    note: String,
}

fn load_manifest() -> Vec<ManifestEntry> {
    let manifest_path = manifest_path();
    let manifest = std::fs::read_to_string(&manifest_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", manifest_path.display()));

    manifest
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| {
            let parts = line.split('|').collect::<Vec<_>>();
            if parts.len() != 4 {
                panic!("invalid manifest line: {line}");
            }
            ManifestEntry {
                kind: parts[0].to_owned(),
                relative_path: parts[1].to_owned(),
                mode: parts[2].to_owned(),
                note: parts[3].to_owned(),
            }
        })
        .collect()
}

fn manifest_path() -> PathBuf {
    std::env::var_os("TELOMERE_COMPONENT_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/component_model_upstream/manifest.txt")
        })
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/component_model_upstream/c7176a512c0bbe4654849f4ba221c1a71c7cf514")
}

#[tokio::test(flavor = "current_thread")]
async fn component_model_upstream() {
    let manifest = load_manifest();
    let snapshot_root = snapshot_root();
    let entries = manifest
        .iter()
        .filter(|entry| entry.kind == "include" && entry.relative_path.ends_with(".wast"))
        .cloned()
        .collect::<Vec<_>>();
    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(entries)));
    let reports = Arc::new(Mutex::new(Vec::new()));
    let worker_count = std::env::var("TELOMERE_COMPONENT_WORKERS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(1);

    let run_worker = |queue: Arc<Mutex<std::collections::VecDeque<ManifestEntry>>>,
                      reports: Arc<
        Mutex<
            Vec<(
                ManifestEntry,
                PathBuf,
                common::component_model::UpstreamCaseReport,
            )>,
        >,
    >,
                      snapshot_root: PathBuf| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build current-thread tokio runtime");
        loop {
            let Some(entry) = queue.lock().unwrap().pop_front() else {
                break;
            };
            let path = snapshot_root.join(&entry.relative_path);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let mode = UpstreamCaseMode::parse(&entry.mode)
                .unwrap_or_else(|error| panic!("{}: {error}", manifest_path().display()));
            let report = runtime.block_on(run_component_upstream_case(path.as_path(), &text, mode));
            reports.lock().unwrap().push((entry, path, report));
        }
    };

    if worker_count == 1 {
        loop {
            let entry = {
                let mut queue = queue.lock().unwrap();
                queue.pop_front()
            };
            let Some(entry) = entry else {
                break;
            };
            let path = snapshot_root.join(&entry.relative_path);
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let mode = UpstreamCaseMode::parse(&entry.mode)
                .unwrap_or_else(|error| panic!("{}: {error}", manifest_path().display()));
            let report = run_component_upstream_case(path.as_path(), &text, mode).await;
            reports.lock().unwrap().push((entry, path, report));
        }
    } else {
        std::thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = Arc::clone(&queue);
                let reports = Arc::clone(&reports);
                let snapshot_root = snapshot_root.clone();
                scope.spawn(move || run_worker(queue, reports, snapshot_root));
            }
        });
    }

    let mut checked = 0usize;
    let mut skipped = 0usize;
    let mut failures = Vec::new();
    for (entry, path, report) in reports.lock().unwrap().drain(..) {
        checked += report.directives_checked;
        skipped += report.directives_skipped;
        if !report.failures.is_empty() {
            failures.push(format!(
                "{} [{} / {}]\n{}",
                path.display(),
                entry.mode,
                entry.note,
                report.failures.join("\n")
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "component_model_upstream failures (checked={}, skipped={})\n\n{}",
            checked,
            skipped,
            failures.join("\n\n")
        );
    }

    println!(
        "component_model_upstream completed: checked={}, skipped={}, files={}",
        checked,
        skipped,
        manifest
            .iter()
            .filter(|entry| entry.kind == "include" && entry.relative_path.ends_with(".wast"))
            .count()
    );
}
