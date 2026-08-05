//! Source-level guard for Telomere's curated public paths.
//!
//! The guard intentionally parses source instead of inspecting the active build:
//! every feature combination therefore produces the same list, including paths
//! behind `#[cfg(...)]`. The four parsed files contain the reviewed root,
//! `host_abi`, component-support, and unstable-internal paths; the dedicated
//! `Module::codes` compatibility field and `Store::take_last_trap` method are
//! read from their defining private sources. `lib.rs` is also checked so that a
//! new public module cannot make that source set grow without being reviewed
//! here; the measurement-only `measure_switches` root is the explicit,
//! non-recursive exception.
//!
//! This is a path-level check, not a signature-level API diff. In particular, it
//! does not notice a new `pub` method on an already exported type. The
//! `unnameable_types` and `private_interfaces` lints, promoted by the workspace's
//! `-D warnings` Clippy command, cover internal types leaking through signatures.
//! If a signature-level compatibility check becomes necessary, `cargo public-api`
//! is the intended upgrade path.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use quote::ToTokens;
use syn::{
    punctuated::Punctuated, Attribute, Expr, ExprLit, ForeignItem, ImplItem, Item, Lit, Meta,
    Token, Type, UseTree, Visibility,
};

const ALLOWED_PUBLIC_MODULES: &[&str] = &[
    "component_support",
    "host_abi",
    "unstable_internals",
    "measure_switches",
];

const FORBIDDEN_UNGATED_FINAL_SEGMENTS: &[&str] = &[
    "Operand",
    "Op",
    "LocalsData",
    "AtomicRmwOp",
    "AtomicWaitResult",
    "MemoryMappingOperation",
    "MemoryInitError",
    "CallFrameCache",
    "IntoCallFrameCache",
    "InstanceData",
    "FunctionInstanceData",
    "EffectSupplier",
];

const ROOT_HOST_LINKING_CARVE_OUT: &[&str] = &[
    "instantiate_native_async_module",
    "link_host_function_with_export_name",
    "link_host_function_with_function_idx",
    "link_async_host_function_with_export_name",
    "link_async_host_function_with_function_idx",
];

const ROOT_DRIVER_CARVE_OUT: &[&str] = &[
    "run_module_function_with_driver",
    "Completion",
    "CompletionPayload",
    "ExecutionDriver",
    "HostCallPending",
    "MemoryWaitPending",
    "PendingOp",
    "TokioDriver",
];

const HOST_ABI_ALWAYS: &[&str] = &[
    "AsyncHostFunction",
    "AsyncHostFunctionDefinition",
    "AsyncHostFuture",
    "AsyncNativeModule",
    "CodeSection",
    "ExecuteContext",
    "Func",
    "FunctionBody",
    "HostFunction",
    "HostFunctionDefinition",
    "Instr",
    "Memory",
    "NativeModule",
    "ObjectRef",
    "ReturnSlot",
    "Stack",
    "LocalReference",
    "StoreInner",
    "instantiate_native_module",
];

const HOST_ABI_THREADS: &[&str] = &["SharedMemoryObject", "SharedWaitRegistration"];

const UPDATE_SNAPSHOT_ENV: &str = "TELOMERE_UPDATE_PUBLIC_SURFACE_SNAPSHOT";

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct CfgContext {
    predicates: Vec<String>,
    implies_unstable_internals: bool,
}

impl CfgContext {
    fn with_attributes(&self, attributes: &[Attribute]) -> Self {
        let mut predicates = self.predicates.clone();
        let mut implies_unstable_internals = self.implies_unstable_internals;

        for attribute in attributes
            .iter()
            .filter(|attribute| attribute.path().is_ident("cfg"))
        {
            predicates.push(cfg_predicate_text(attribute));
            implies_unstable_internals |= cfg_attribute_implies_unstable_internals(attribute);
        }

        predicates.sort();
        predicates.dedup();

        Self {
            predicates,
            implies_unstable_internals,
        }
    }

    fn rendered_predicates(&self) -> String {
        if self.predicates.is_empty() {
            "always".to_owned()
        } else {
            self.predicates.join(" && ")
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PublicPath {
    path: String,
    cfg: CfgContext,
}

impl PublicPath {
    fn final_segment(&self) -> &str {
        self.path
            .rsplit("::")
            .next()
            .expect("public paths always have a final segment")
    }

    fn is_gated_by_unstable_internals(&self) -> bool {
        self.cfg.implies_unstable_internals
    }

    fn render(&self) -> String {
        format!("{} | {}\n", self.path, self.cfg.rendered_predicates())
    }
}

#[test]
fn public_surface_matches_committed_snapshot() {
    let actual = render_snapshot(&collect_public_surface());
    let snapshot_path = snapshot_path();

    if matches!(env::var(UPDATE_SNAPSHOT_ENV).as_deref(), Ok("1")) {
        fs::write(&snapshot_path, actual).unwrap_or_else(|error| {
            panic!("failed to update {}: {error}", snapshot_path.display())
        });
        return;
    }

    let expected = fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", snapshot_path.display()));
    assert_eq!(
        actual, expected,
        "public path surface changed; review the source change, then regenerate with \
         {UPDATE_SNAPSHOT_ENV}=1 cargo test -p telomere --release --test public_surface"
    );
}

#[test]
fn snapshot_partitions_match_committed_surface() {
    let records = collect_public_surface();
    let expected = committed_snapshot_records();

    assert_eq!(
        partition_by_unstable_internals(&records, false),
        partition_by_unstable_internals(&expected, false),
        "ungated public paths must exactly match the committed single snapshot"
    );
    assert_eq!(
        partition_by_unstable_internals(&records, true),
        partition_by_unstable_internals(&expected, true),
        "unstable-internal public paths must exactly match the committed single snapshot"
    );
}

#[test]
fn required_default_host_carve_outs_are_explicit() {
    let records = collect_public_surface();

    let expected_host_abi = expected_paths("telomere::host_abi", HOST_ABI_ALWAYS, HOST_ABI_THREADS);
    let actual_host_abi = records
        .iter()
        .filter(|record| record.path.starts_with("telomere::host_abi::"))
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_host_abi, expected_host_abi,
        "host_abi must contain exactly the reviewed nineteen always-present and two threads-only paths"
    );

    let expected_root_carve_out = ROOT_HOST_LINKING_CARVE_OUT
        .iter()
        .chain(ROOT_DRIVER_CARVE_OUT)
        .map(|name| {
            let predicates = if *name == "MemoryWaitPending" {
                &["cfg(feature = \"threads\")"][..]
            } else {
                &[]
            };
            expected_path(&format!("telomere::{name}"), predicates)
        })
        .collect::<BTreeSet<_>>();
    let actual_root_carve_out = records
        .iter()
        .filter(|record| {
            expected_root_carve_out
                .iter()
                .any(|expected| expected.path == record.path)
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_root_carve_out, expected_root_carve_out,
        "the five host-linking and eight driver root carve-out paths must retain their reviewed cfg predicates"
    );

    let expected_module_codes = expected_path("telomere::Module::codes", &[]);
    let actual_module_codes = records
        .iter()
        .filter(|record| record.path == expected_module_codes.path)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_module_codes,
        BTreeSet::from([expected_module_codes]),
        "Module::codes is a reviewed default host-linking compatibility field"
    );

    let expected_store_take_last_trap = expected_path("telomere::Store::take_last_trap", &[]);
    let actual_store_take_last_trap = records
        .iter()
        .filter(|record| record.path == expected_store_take_last_trap.path)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual_store_take_last_trap,
        BTreeSet::from([expected_store_take_last_trap]),
        "Store::take_last_trap is the reviewed one-point method-level diagnostic carve-out"
    );
}

#[test]
fn special_function_return_is_opt_in_only() {
    let records = collect_public_surface();
    let expected = BTreeSet::from([expected_path(
        "telomere::special_function_return",
        &["cfg(feature = \"unstable-internals\")"],
    )]);
    let actual = records
        .iter()
        .filter(|record| record.path == "telomere::special_function_return")
        .cloned()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        actual, expected,
        "special_function_return is an opt-in raw instruction-construction hook, not a default host-linking path"
    );
}

#[test]
fn wasm_async_pending_is_opt_in_only() {
    let records = collect_public_surface();
    let expected = BTreeSet::from([expected_path(
        "telomere::WasmAsyncPending",
        &["cfg(feature = \"unstable-internals\")"],
    )]);
    let actual = records
        .iter()
        .filter(|record| record.path == "telomere::WasmAsyncPending")
        .cloned()
        .collect::<BTreeSet<_>>();

    assert_eq!(
        actual, expected,
        "WasmAsyncPending is a reserved opt-in pending-operation type, not a default driver path"
    );
}

#[test]
fn ungated_surface_excludes_non_abi_interpreter_representation_paths() {
    let records = collect_public_surface();

    for record in &records {
        if record.is_gated_by_unstable_internals() {
            continue;
        }

        let final_segment = record.final_segment();
        assert!(
            !(FORBIDDEN_UNGATED_FINAL_SEGMENTS.contains(&final_segment)
                || final_segment.ends_with("Section") && final_segment != "CodeSection"),
            "ungated public path {} exposes a non-ABI interpreter representation; \
             retain it only through `unstable_internals`",
            record.path
        );
    }
}

#[test]
fn advertised_jit_observability_remains_public_with_jit() {
    let records = collect_public_surface();
    let expected = expected_path("telomere::JitCacheStats", &["cfg(feature = \"jit\")"]);
    let actual = records
        .iter()
        .filter(|record| record.path == expected.path)
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual,
        BTreeSet::from([expected]),
        "the minimal-embedder JIT cache observation type must remain public with `jit`"
    );
}

fn collect_public_surface() -> BTreeSet<PublicPath> {
    let crate_dir = crate_dir();
    let lib = parse_source(&crate_dir, "src/lib.rs");
    let common = parse_source(&crate_dir, "src/common.rs");
    let store = parse_source(&crate_dir, "src/common/store.rs");
    let component_support = parse_source(&crate_dir, "src/component_support.rs");
    let host_abi = parse_source(&crate_dir, "src/host_abi.rs");
    let unstable_internals = parse_source(&crate_dir, "src/unstable_internals.rs");

    assert_public_module_closure(&lib);
    assert_public_module_sources_closed(&lib, "src/lib.rs", ALLOWED_PUBLIC_MODULES);
    assert_public_module_sources_closed(&component_support, "src/component_support.rs", &[]);
    assert_public_module_sources_closed(&host_abi, "src/host_abi.rs", &[]);
    assert_public_module_sources_closed(&unstable_internals, "src/unstable_internals.rs", &[]);

    let lib_cfg = CfgContext::default().with_attributes(&lib.attrs);
    let component_support_cfg = public_module_cfg(&lib, &lib_cfg, "component_support")
        .with_attributes(&component_support.attrs);
    let host_abi_cfg =
        public_module_cfg(&lib, &lib_cfg, "host_abi").with_attributes(&host_abi.attrs);
    let unstable_internals_cfg = public_module_cfg(&lib, &lib_cfg, "unstable_internals")
        .with_attributes(&unstable_internals.attrs);

    let mut records = BTreeSet::new();
    collect_items(&lib.items, &["telomere".to_owned()], &lib_cfg, &mut records);
    let module_cfg = records
        .iter()
        .find(|record| record.path == "telomere::Module")
        .map(|record| record.cfg.clone())
        .expect("src/lib.rs must re-export telomere::Module");
    collect_module_codes_field(&common, &module_cfg, &mut records);
    let store_cfg = records
        .iter()
        .find(|record| record.path == "telomere::Store")
        .map(|record| record.cfg.clone())
        .expect("src/lib.rs must re-export telomere::Store");
    collect_store_take_last_trap_method(&store, &store_cfg, &mut records);
    collect_items(
        &component_support.items,
        &["telomere".to_owned(), "component_support".to_owned()],
        &component_support_cfg,
        &mut records,
    );
    collect_items(
        &host_abi.items,
        &["telomere".to_owned(), "host_abi".to_owned()],
        &host_abi_cfg,
        &mut records,
    );
    collect_items(
        &unstable_internals.items,
        &["telomere".to_owned(), "unstable_internals".to_owned()],
        &unstable_internals_cfg,
        &mut records,
    );

    records
}

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn snapshot_path() -> PathBuf {
    crate_dir().join("tests/public_surface.snapshot")
}

fn committed_snapshot_records() -> BTreeSet<PublicPath> {
    let path = snapshot_path();
    let snapshot = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    let records = parse_snapshot(&snapshot, &path);
    assert_eq!(
        render_snapshot(&records),
        snapshot,
        "{} must use the canonical sorted public-surface format",
        path.display()
    );
    records
}

fn parse_snapshot(snapshot: &str, path: &Path) -> BTreeSet<PublicPath> {
    snapshot
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (public_path, rendered_cfg) = line.split_once(" | ").unwrap_or_else(|| {
                panic!(
                    "{} contains an invalid public-surface line `{line}`; expected `path | cfg`",
                    path.display()
                )
            });
            let predicates = match rendered_cfg {
                "always" => Vec::new(),
                _ => rendered_cfg.split(" && ").map(str::to_owned).collect(),
            };
            let implies_unstable_internals = predicates.iter().any(|predicate| {
                let meta = predicate
                    .strip_prefix("cfg(")
                    .and_then(|predicate| predicate.strip_suffix(')'))
                    .unwrap_or_else(|| {
                        panic!(
                            "{} contains unsupported cfg text `{predicate}`",
                            path.display()
                        )
                    });
                let meta = syn::parse_str::<Meta>(meta).unwrap_or_else(|error| {
                    panic!(
                        "{} contains invalid cfg predicate `{predicate}`: {error}",
                        path.display()
                    )
                });
                cfg_predicate_implies_unstable_internals(&meta)
            });
            PublicPath {
                path: public_path.to_owned(),
                cfg: CfgContext {
                    predicates,
                    implies_unstable_internals,
                },
            }
        })
        .collect()
}

fn partition_by_unstable_internals(
    records: &BTreeSet<PublicPath>,
    unstable_internals: bool,
) -> BTreeSet<PublicPath> {
    records
        .iter()
        .filter(|record| record.is_gated_by_unstable_internals() == unstable_internals)
        .cloned()
        .collect()
}

fn collect_module_codes_field(
    common: &syn::File,
    module_cfg: &CfgContext,
    records: &mut BTreeSet<PublicPath>,
) {
    let module = common
        .items
        .iter()
        .find_map(|item| match item {
            Item::Struct(item) if item.ident == "Module" => Some(item),
            _ => None,
        })
        .expect("src/common.rs must define Module");
    assert!(
        is_public(&module.vis),
        "the root-re-exported Module definition must remain public"
    );
    let codes = module
        .fields
        .iter()
        .find(|field| {
            field
                .ident
                .as_ref()
                .map(|ident| ident == "codes")
                .unwrap_or(false)
        })
        .expect("Module must retain its reviewed codes field");
    assert!(
        is_public(&codes.vis),
        "Module::codes must remain public for the host-linking compatibility carve-out"
    );
    let module_cfg = module_cfg.with_attributes(&module.attrs);
    record_named(
        records,
        &["telomere".to_owned(), "Module".to_owned()],
        "codes",
        &module_cfg,
        &codes.attrs,
    );
}

fn collect_store_take_last_trap_method(
    store: &syn::File,
    store_cfg: &CfgContext,
    records: &mut BTreeSet<PublicPath>,
) {
    let (implementation, method) = store
        .items
        .iter()
        .find_map(|item| {
            let Item::Impl(implementation) = item else {
                return None;
            };
            let Type::Path(self_type) = implementation.self_ty.as_ref() else {
                return None;
            };
            if !self_type.path.is_ident("Store") {
                return None;
            }
            implementation.items.iter().find_map(|item| match item {
                ImplItem::Fn(method) if method.sig.ident == "take_last_trap" => {
                    Some((implementation, method))
                }
                _ => None,
            })
        })
        .expect("Store must retain its reviewed take_last_trap method");
    assert!(
        is_public(&method.vis),
        "Store::take_last_trap must remain public for the diagnostic retrieval carve-out"
    );
    let store_cfg = store_cfg.with_attributes(&implementation.attrs);
    record_named(
        records,
        &["telomere".to_owned(), "Store".to_owned()],
        "take_last_trap",
        &store_cfg,
        &method.attrs,
    );
}

fn expected_paths(base: &str, always: &[&str], threads_only: &[&str]) -> BTreeSet<PublicPath> {
    always
        .iter()
        .map(|name| expected_path(&format!("{base}::{name}"), &[]))
        .chain(
            threads_only.iter().map(|name| {
                expected_path(&format!("{base}::{name}"), &["cfg(feature = \"threads\")"])
            }),
        )
        .collect()
}

fn expected_path(path: &str, predicates: &[&str]) -> PublicPath {
    let predicates = predicates
        .iter()
        .map(|predicate| (*predicate).to_owned())
        .collect::<Vec<_>>();
    let implies_unstable_internals = predicates.iter().any(|predicate| {
        let meta = predicate
            .strip_prefix("cfg(")
            .and_then(|predicate| predicate.strip_suffix(')'))
            .expect("expected test cfg predicates must use cfg(...)");
        let meta = syn::parse_str::<Meta>(meta)
            .expect("expected test cfg predicates must parse as syn metadata");
        cfg_predicate_implies_unstable_internals(&meta)
    });
    PublicPath {
        path: path.to_owned(),
        cfg: CfgContext {
            predicates,
            implies_unstable_internals,
        },
    }
}

fn parse_source(crate_dir: &Path, relative_path: &str) -> syn::File {
    let path = crate_dir.join(relative_path);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    syn::parse_file(&source)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()))
}

fn assert_public_module_closure(lib: &syn::File) {
    for item in &lib.items {
        let Item::Mod(item_mod) = item else {
            continue;
        };
        if !is_public(&item_mod.vis) {
            continue;
        }

        let module_name = item_mod.ident.to_string();
        assert!(
            ALLOWED_PUBLIC_MODULES.contains(&module_name.as_str()),
            "src/lib.rs exposes public module `{module_name}` outside the tracked public-module \
             closure; add it to this test and the reviewed snapshot intentionally"
        );
    }
}

fn assert_public_module_sources_closed(
    file: &syn::File,
    source_name: &str,
    allowed_out_of_line_modules: &[&str],
) {
    for item in &file.items {
        let Item::Mod(item_mod) = item else {
            continue;
        };
        if !is_public(&item_mod.vis) || item_mod.content.is_some() {
            continue;
        }

        let module_name = item_mod.ident.to_string();
        assert!(
            allowed_out_of_line_modules.contains(&module_name.as_str()),
            "{source_name} exposes out-of-line public module `{module_name}` outside the parsed \
             source closure; make it inline or add a reviewed source parser"
        );
    }
}

fn public_module_cfg(lib: &syn::File, root_cfg: &CfgContext, module_name: &str) -> CfgContext {
    let item_mod = lib
        .items
        .iter()
        .find_map(|item| match item {
            Item::Mod(item_mod) if item_mod.ident == module_name => Some(item_mod),
            _ => None,
        })
        .unwrap_or_else(|| panic!("src/lib.rs must declare public module `{module_name}`"));
    assert!(
        is_public(&item_mod.vis),
        "src/lib.rs module `{module_name}` must remain public so its parsed surface has a path"
    );
    root_cfg.with_attributes(&item_mod.attrs)
}

fn collect_items(
    items: &[Item],
    base: &[String],
    inherited_cfg: &CfgContext,
    records: &mut BTreeSet<PublicPath>,
) {
    for item in items {
        match item {
            Item::Const(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::Enum(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::ExternCrate(item) if is_public(&item.vis) => {
                let name = item
                    .rename
                    .as_ref()
                    .map(|(_, rename)| rename)
                    .unwrap_or(&item.ident)
                    .to_string();
                record_named(records, base, &name, inherited_cfg, &item.attrs);
            }
            Item::Fn(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.sig.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::ForeignMod(item) => {
                let cfg = inherited_cfg.with_attributes(&item.attrs);
                collect_foreign_items(&item.items, base, &cfg, records);
            }
            Item::Macro(item) if is_macro_exported(&item.attrs) => {
                if let Some(ident) = &item.ident {
                    let crate_root = ["telomere".to_owned()];
                    record_named(
                        records,
                        &crate_root,
                        &ident.to_string(),
                        inherited_cfg,
                        &item.attrs,
                    );
                }
            }
            Item::Mod(item) if is_public(&item.vis) => {
                let cfg = inherited_cfg.with_attributes(&item.attrs);
                let path = child_path(base, &item.ident.to_string());
                record_path(records, &path, &cfg);
                if let Some((_, contents)) = &item.content {
                    collect_items(contents, &path, &cfg, records);
                }
            }
            Item::Static(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::Struct(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::Trait(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::TraitAlias(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::Type(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::Union(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            Item::Use(item) if is_public(&item.vis) => {
                let cfg = inherited_cfg.with_attributes(&item.attrs);
                collect_use_tree(&item.tree, &mut Vec::new(), base, &cfg, records);
            }
            _ => {}
        }
    }
}

fn collect_foreign_items(
    items: &[ForeignItem],
    base: &[String],
    inherited_cfg: &CfgContext,
    records: &mut BTreeSet<PublicPath>,
) {
    for item in items {
        match item {
            ForeignItem::Fn(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.sig.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            ForeignItem::Static(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            ForeignItem::Type(item) if is_public(&item.vis) => {
                record_named(
                    records,
                    base,
                    &item.ident.to_string(),
                    inherited_cfg,
                    &item.attrs,
                );
            }
            _ => {}
        }
    }
}

fn collect_use_tree(
    tree: &UseTree,
    source_segments: &mut Vec<String>,
    base: &[String],
    cfg: &CfgContext,
    records: &mut BTreeSet<PublicPath>,
) {
    match tree {
        UseTree::Name(name) => {
            let exported_name = if name.ident == "self" {
                source_segments
                    .last()
                    .unwrap_or_else(|| panic!("public `use self` has no exported path segment"))
                    .clone()
            } else {
                name.ident.to_string()
            };
            record_named(records, base, &exported_name, cfg, &[]);
        }
        UseTree::Rename(rename) => {
            if rename.rename != "_" {
                record_named(records, base, &rename.rename.to_string(), cfg, &[]);
            }
        }
        UseTree::Path(path) => {
            source_segments.push(path.ident.to_string());
            collect_use_tree(&path.tree, source_segments, base, cfg, records);
            source_segments.pop();
        }
        UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_tree(tree, source_segments, base, cfg, records);
            }
        }
        UseTree::Glob(_) => {
            panic!(
                "public glob re-export `{}::*` cannot be expanded into a closed reviewed surface; \
                 replace it with explicit names",
                source_segments.join("::")
            );
        }
    }
}

fn record_named(
    records: &mut BTreeSet<PublicPath>,
    base: &[String],
    name: &str,
    inherited_cfg: &CfgContext,
    attributes: &[Attribute],
) {
    let cfg = inherited_cfg.with_attributes(attributes);
    let path = child_path(base, name);
    record_path(records, &path, &cfg);
}

fn record_path(records: &mut BTreeSet<PublicPath>, path: &[String], cfg: &CfgContext) {
    records.insert(PublicPath {
        path: path.join("::"),
        cfg: cfg.clone(),
    });
}

fn child_path(base: &[String], name: &str) -> Vec<String> {
    let mut path = base.to_vec();
    path.push(name.to_owned());
    path
}

fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn is_macro_exported(attributes: &[Attribute]) -> bool {
    attributes
        .iter()
        .any(|attribute| attribute.path().is_ident("macro_export"))
}

fn cfg_predicate_text(attribute: &Attribute) -> String {
    match &attribute.meta {
        Meta::List(list) => format!("cfg({})", list.tokens.to_token_stream()),
        meta => meta.to_token_stream().to_string(),
    }
}

fn cfg_attribute_implies_unstable_internals(attribute: &Attribute) -> bool {
    let predicate = attribute
        .parse_args::<Meta>()
        .unwrap_or_else(|error| panic!("failed to parse cfg predicate: {error}"));
    cfg_predicate_implies_unstable_internals(&predicate)
}

fn cfg_predicate_implies_unstable_internals(predicate: &Meta) -> bool {
    match predicate {
        Meta::NameValue(name_value) => {
            name_value.path.is_ident("feature")
                && matches!(
                    &name_value.value,
                    Expr::Lit(ExprLit {
                        lit: Lit::Str(feature),
                        ..
                    }) if feature.value() == "unstable-internals"
                )
        }
        Meta::List(list) if list.path.is_ident("all") => {
            let predicates = parse_cfg_predicates(list);
            predicates
                .iter()
                .any(cfg_predicate_implies_unstable_internals)
        }
        Meta::List(list) if list.path.is_ident("any") => {
            let predicates = parse_cfg_predicates(list);
            !predicates.is_empty()
                && predicates
                    .iter()
                    .all(cfg_predicate_implies_unstable_internals)
        }
        _ => false,
    }
}

fn parse_cfg_predicates(list: &syn::MetaList) -> Punctuated<Meta, Token![,]> {
    list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
        .unwrap_or_else(|error| panic!("failed to parse cfg predicate list: {error}"))
}

fn render_snapshot(records: &BTreeSet<PublicPath>) -> String {
    records.iter().map(PublicPath::render).collect()
}
