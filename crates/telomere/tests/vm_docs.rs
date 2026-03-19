use std::{fs, path::PathBuf};

use syn::Item;

const VM_SOURCES: &[&str] = &[
    "src/runtime/vm.rs",
    "src/runtime/vm/atomics.rs",
    "src/runtime/vm/bulk_memory.rs",
    "src/runtime/vm/call.rs",
    "src/runtime/vm/control.rs",
    "src/runtime/vm/globals.rs",
    "src/runtime/vm/locals.rs",
    "src/runtime/vm/memory.rs",
    "src/runtime/vm/numeric.rs",
    "src/runtime/vm/refs.rs",
    "src/runtime/vm/simd.rs",
    "src/runtime/vm/tables.rs",
    "src/runtime/vm/traps.rs",
];

#[test]
fn vm_unsafe_entrypoints_have_docs_and_safety_sections() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in VM_SOURCES {
        let path = crate_dir.join(rel);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        assert!(
            !source.contains("missing_safety_doc"),
            "{} still suppresses missing_safety_doc",
            path.display()
        );

        let file = syn::parse_file(&source)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        for item in &file.items {
            let Item::Fn(item_fn) = item else {
                continue;
            };
            if item_fn.sig.unsafety.is_none() {
                continue;
            }

            let docs = item_fn
                .attrs
                .iter()
                .filter_map(doc_attr_value)
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                !docs.is_empty(),
                "{}:{} missing doc comments",
                path.display(),
                item_fn.sig.ident
            );
            assert!(
                docs.contains("# Safety"),
                "{}:{} missing # Safety section",
                path.display(),
                item_fn.sig.ident
            );
            assert!(
                docs.contains("http")
                    || docs.contains("WebAssembly `")
                    || docs.contains("Telomere runtime helper")
                    || docs.contains("Telomere internal"),
                "{}:{} missing spec or mnemonic context",
                path.display(),
                item_fn.sig.ident
            );
        }
    }
}

#[test]
fn macro_generated_vm_handlers_emit_doc_templates() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let simd = fs::read_to_string(crate_dir.join("src/runtime/vm/simd.rs"))
        .expect("failed to read simd.rs");
    assert!(
        simd.contains("WebAssembly SIMD handler"),
        "simd.rs is missing macro-generated handler docs"
    );
    assert!(
        simd.contains("# Safety"),
        "simd.rs macro-generated handler docs are missing # Safety"
    );

    let atomics = fs::read_to_string(crate_dir.join("src/runtime/vm/atomics.rs"))
        .expect("failed to read atomics.rs");
    assert!(
        atomics.contains("WebAssembly threads atomic"),
        "atomics.rs is missing macro-generated handler docs"
    );
    assert!(
        atomics.contains("# Safety"),
        "atomics.rs macro-generated handler docs are missing # Safety"
    );

    let simd_macro = fs::read_to_string(
        crate_dir
            .parent()
            .expect("crate dir has parent")
            .join("telomere-macros/src/lib.rs"),
    )
    .expect("failed to read telomere-macros/src/lib.rs");
    assert!(
        simd_macro.contains("Stack effect:")
            && simd_macro.contains("# Safety")
            && simd_macro
                .contains("https://webassembly.github.io/spec/core/exec/instructions.html"),
        "define_simd_operation! is missing spec-based doc generation"
    );
}

fn doc_attr_value(attr: &syn::Attribute) -> Option<String> {
    if !attr.path().is_ident("doc") {
        return None;
    }
    match &attr.meta {
        syn::Meta::NameValue(value) => match &value.value {
            syn::Expr::Lit(expr) => match &expr.lit {
                syn::Lit::Str(s) => Some(s.value()),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}
