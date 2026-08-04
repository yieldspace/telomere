use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use syn::{
    visit::{self, Visit},
    Block, Expr, ExprForLoop, ExprLoop, ExprWhile, ImplItemFn, Item, Stmt, TraitItemFn,
};

// Explicit VM loops plus the Store helpers that execute guest-sized cross-memory work.
const LOOP_COVERAGE_SOURCES: &[&str] = &[
    "src/common/memory.rs",
    "src/common/store.rs",
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
    "src/runtime/vm/superinstructions.rs",
    "src/runtime/vm/tables.rs",
    "src/runtime/vm/traps.rs",
];

// This is the pre-existing unsafe-doc surface. `superinstructions.rs` is included in
// LOOP_COVERAGE_SOURCES for fail-closed loop coverage, but documenting its legacy handlers is
// outside #127.
const VM_UNSAFE_DOC_SOURCES: &[&str] = &[
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

const PRIVATE_UNSAFE_DOC_SURFACE: &[&str] = &[
    "call_code",
    "call_next",
    "store_internal_local",
    "store_internal_shared",
    "store_internal_local_indexed",
    "store_internal_shared_indexed",
];

#[test]
fn vm_unsafe_entrypoints_have_docs_and_safety_sections() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in VM_UNSAFE_DOC_SOURCES {
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
            let ident = item_fn.sig.ident.to_string();
            let is_public_surface = !matches!(item_fn.vis, syn::Visibility::Inherited);
            if !is_public_surface && !PRIVATE_UNSAFE_DOC_SURFACE.contains(&ident.as_str()) {
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
                ident
            );
            assert!(
                docs.contains("# Safety"),
                "{}:{} missing # Safety section",
                path.display(),
                ident
            );
            assert!(
                docs.contains("http")
                    || docs.contains("WebAssembly `")
                    || docs.contains("Telomere runtime helper")
                    || docs.contains("Telomere internal"),
                "{}:{} missing spec or mnemonic context",
                path.display(),
                ident
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

#[derive(Debug)]
struct LoopRecord {
    source: String,
    function: String,
    identifier: String,
    has_direct_checkpoint: bool,
    has_small_literal_bound: bool,
}

struct LoopAllowance {
    source: &'static str,
    function: &'static str,
    identifier: &'static str,
    reason: &'static str,
}

const LOOP_ALLOWANCES: &[LoopAllowance] = &[
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "grouped_profile_stats",
        identifier: "for:stats . iter () . copied ()",
        reason:
            "vm-profile reporting iterates host-owned snapshot statistics after guest execution.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "grouped_profile_pairs",
        identifier: "for:pairs . iter () . copied ()",
        reason:
            "vm-profile reporting iterates host-owned snapshot statistics after guest execution.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "grouped_profile_triples",
        identifier: "for:triples . iter () . copied ()",
        reason:
            "vm-profile reporting iterates host-owned snapshot statistics after guest execution.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "drop",
        identifier: "for:& snapshot . stats",
        reason: "vm-profile reporting iterates a host-owned snapshot during shutdown.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "drop",
        identifier: "for:& snapshot . pairs",
        reason: "vm-profile reporting iterates a host-owned snapshot during shutdown.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "drop",
        identifier: "for:& snapshot . triples",
        reason: "vm-profile reporting iterates a host-owned snapshot during shutdown.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "drop",
        identifier: "for:DispatchProfileFamilyGroup :: ORDER",
        reason: "vm-profile reporting iterates the fixed profiling-family order.",
    },
    LoopAllowance {
        source: "src/runtime/vm.rs",
        function: "push_result_values",
        identifier: "for:types . iter () . zip (values . iter ())",
        reason: "the validated module result type list fixes the paired result count.",
    },
    LoopAllowance {
        source: "src/runtime/vm/control.rs",
        function: "skip_end_ops",
        identifier: "while:std :: ptr :: fn_addr_eq ((* ptr) . op , op_end as Op)",
        reason: "walks compiler-emitted instruction slots and never follows guest data.",
    },
    LoopAllowance {
        source: "src/runtime/vm/memory.rs",
        function: "skip_end_ops",
        identifier: "while:std :: ptr :: fn_addr_eq ((* ptr) . op , op_end as Op)",
        reason: "walks compiler-emitted instruction slots and never follows guest data.",
    },
    LoopAllowance {
        source: "src/runtime/vm/memory.rs",
        function: "op_scalar_copy_local_base_run",
        identifier: "for:0 .. count",
        reason: "the fused instruction encodes count in eight bits, bounding the run to 255 slots.",
    },
    LoopAllowance {
        source: "src/runtime/vm/numeric.rs",
        function: "op_i32_select_bit_step4_run",
        identifier: "for:0 .. count",
        reason:
            "count is a lowered instruction-stream run length, not a guest stack or memory value.",
    },
    LoopAllowance {
        source: "src/runtime/vm/simd.rs",
        function: "i8x16_shuffle",
        identifier: "for:lanes . into_iter () . enumerate ()",
        reason: "lanes is a fixed [u8; 16] SIMD array.",
    },
    LoopAllowance {
        source: "src/runtime/vm/superinstructions.rs",
        function: "op_local_get4_run",
        identifier: "for:0 .. count",
        reason: "the fused local.get run is validated to the encoded 4 through 16 slot width.",
    },
    LoopAllowance {
        source: "src/runtime/vm/superinstructions.rs",
        function: "op_local_get4_run_skip",
        identifier: "for:0 .. count",
        reason: "the fused local.get run is validated to the encoded 4 through 16 slot width.",
    },
    LoopAllowance {
        source: "src/common/memory.rs",
        function: "notify_waiters",
        identifier: "while:remaining != 0",
        reason: "the queue length is bounded by the number of guest threads concurrently blocked in atomic.wait, which is controlled by host thread creation rather than guest data; metered callers charge completed wakes after the wait-queue lock is released.",
    },
    LoopAllowance {
        source: "src/common/memory.rs",
        function: "notify_waiters",
        identifier: "for:wake",
        reason: "the wake set is bounded by the host-controlled number of concurrently waiting guest threads and is charged after the wait-queue lock is released.",
    },
];

#[derive(Debug)]
struct MacroLoopRecord {
    source: String,
    macro_name: String,
    origin: MacroOrigin,
    loops: usize,
    checkpoints: usize,
}

struct MacroLoopAllowance {
    source: &'static str,
    macro_name: &'static str,
    origin: MacroOrigin,
    expected_loops: usize,
    expected_checkpoints: usize,
    expected_uses: usize,
    reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MacroOrigin {
    Definition,
    Invocation,
}

impl MacroOrigin {
    const fn label(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Invocation => "invocation",
        }
    }
}

const MACRO_LOOP_ALLOWANCES: &[MacroLoopAllowance] = &[
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "all_true_instruction",
        origin: MacroOrigin::Definition,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 1,
        reason: "the macro iterates a fixed-width SIMD lane array produced by to_array().",
    },
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "shl_instruction",
        origin: MacroOrigin::Definition,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 1,
        reason: "the macro iterates a fixed-width SIMD lane array whose length is selected by the vector type.",
    },
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "shr_instruction",
        origin: MacroOrigin::Definition,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 1,
        reason: "the macro iterates a fixed-width SIMD lane array whose length is selected by the vector type.",
    },
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "narrow_instruction",
        origin: MacroOrigin::Definition,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 1,
        reason: "the macro iterates the compile-time LaneType lane count of a fixed SIMD vector.",
    },
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "define_unary_simd_operation",
        origin: MacroOrigin::Invocation,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 11,
        reason: "current invocation token bodies use only fixed SIMD lane loops; the trip count is not guest data.",
    },
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "define_binary_simd_operation",
        origin: MacroOrigin::Invocation,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 1,
        reason: "current invocation token bodies use only fixed SIMD lane loops; the trip count is not guest data.",
    },
    MacroLoopAllowance {
        source: "src/runtime/vm/simd.rs",
        macro_name: "define_simd_cmp_operation",
        origin: MacroOrigin::Definition,
        expected_loops: 1,
        expected_checkpoints: 0,
        expected_uses: 1,
        reason: "the macro iterates the compile-time LaneType lane count of a fixed SIMD vector.",
    },
];

/// Covers only explicit AST loops in `LOOP_COVERAGE_SOURCES`; it does not prove that every unit
/// of guest work is bounded or that native bulk primitives are interruptible.
#[test]
fn vm_loop_coverage_is_fail_closed() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert_loop_coverage_source_inventory(&crate_dir);

    let mut uncovered = Vec::new();
    let mut allowance_uses = vec![0usize; LOOP_ALLOWANCES.len()];
    for source in LOOP_COVERAGE_SOURCES {
        let path = crate_dir.join(source);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let file = syn::parse_file(&contents)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        let mut visitor = LoopVisitor::new(source);
        visitor.visit_file(&file);

        for loop_record in visitor.loops {
            if loop_record.has_direct_checkpoint || loop_record.has_small_literal_bound {
                continue;
            }

            let Some((allowance_index, allowance)) =
                LOOP_ALLOWANCES.iter().enumerate().find(|(_, allowance)| {
                    allowance.source == loop_record.source
                        && allowance.function == loop_record.function
                        && allowance.identifier == loop_record.identifier
                })
            else {
                uncovered.push(format!(
                    "{}::{} [{}] has no direct checkpoint and no allow-list reason",
                    loop_record.source, loop_record.function, loop_record.identifier
                ));
                continue;
            };

            if allowance.reason.trim().is_empty() {
                uncovered.push(format!(
                    "{}::{} [{}] has an empty allow-list reason",
                    loop_record.source, loop_record.function, loop_record.identifier
                ));
            }
            allowance_uses[allowance_index] += 1;
        }
    }

    for (allowance, uses) in LOOP_ALLOWANCES.iter().zip(allowance_uses) {
        if uses != 1 {
            uncovered.push(format!(
                "allow-list entry {}::{} [{}] matched {uses} loops; expected exactly one",
                allowance.source, allowance.function, allowance.identifier
            ));
        }
    }

    assert!(
        uncovered.is_empty(),
        "VM loop coverage must be explicit:\n{}",
        uncovered.join("\n")
    );
}

/// Covers only loop tokens explicitly present in macro_rules bodies and macro invocation token
/// streams in `LOOP_COVERAGE_SOURCES`; macro expansion and native library internals are outside
/// this syntactic gate.
#[test]
fn vm_macro_loop_coverage_is_fail_closed() {
    let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    assert_loop_coverage_source_inventory(&crate_dir);

    let mut uncovered = Vec::new();
    let mut allowance_uses = vec![0usize; MACRO_LOOP_ALLOWANCES.len()];
    for source in LOOP_COVERAGE_SOURCES {
        let path = crate_dir.join(source);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let file = syn::parse_file(&contents)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        let mut visitor = MacroLoopVisitor::new(source);
        visitor.visit_file(&file);

        for record in visitor.records {
            if checkpoints_cover_loops(record.loops, record.checkpoints) {
                continue;
            }
            let Some((allowance_index, allowance)) =
                MACRO_LOOP_ALLOWANCES
                    .iter()
                    .enumerate()
                    .find(|(_, allowance)| {
                        allowance.source == record.source
                            && allowance.macro_name == record.macro_name
                            && allowance.origin == record.origin
                    })
            else {
                uncovered.push(format!(
                    "{} {} macro {} contains {} loop tokens but only {} vm_checkpoint tokens",
                    record.source,
                    record.origin.label(),
                    record.macro_name,
                    record.loops,
                    record.checkpoints
                ));
                continue;
            };
            if allowance.reason.trim().is_empty() {
                uncovered.push(format!(
                    "{} macro {} has an empty allow-list reason",
                    record.source, record.macro_name
                ));
            }
            if record.loops != allowance.expected_loops
                || record.checkpoints != allowance.expected_checkpoints
            {
                uncovered.push(format!(
                    "{} {} macro {} changed token shape: expected {}/{} loop/checkpoint tokens, found {}/{}",
                    record.source,
                    record.origin.label(),
                    record.macro_name,
                    allowance.expected_loops,
                    allowance.expected_checkpoints,
                    record.loops,
                    record.checkpoints
                ));
            }
            allowance_uses[allowance_index] += 1;
        }
    }

    for (allowance, uses) in MACRO_LOOP_ALLOWANCES.iter().zip(allowance_uses) {
        if uses != allowance.expected_uses {
            uncovered.push(format!(
                "macro allow-list entry {}::{} ({}) matched {uses} macro bodies or invocations; expected {}",
                allowance.source,
                allowance.macro_name,
                allowance.origin.label(),
                allowance.expected_uses,
            ));
        }
    }

    assert!(
        uncovered.is_empty(),
        "VM macro loop coverage must be explicit:\n{}",
        uncovered.join("\n")
    );
}

fn assert_loop_coverage_source_inventory(crate_dir: &Path) {
    let mut expected = BTreeSet::from(["src/runtime/vm.rs".to_owned()]);
    for entry in fs::read_dir(crate_dir.join("src/runtime/vm"))
        .expect("failed to read runtime/vm source directory")
    {
        let entry = entry.expect("failed to read runtime/vm source entry");
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "rs") {
            expected.insert(format!(
                "src/runtime/vm/{}",
                path.file_name()
                    .expect("runtime/vm source has file name")
                    .to_string_lossy()
            ));
        }
    }
    expected.insert("src/common/memory.rs".to_owned());
    expected.insert("src/common/store.rs".to_owned());
    let actual = LOOP_COVERAGE_SOURCES
        .iter()
        .map(|source| (*source).to_owned())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "LOOP_COVERAGE_SOURCES must cover every runtime/vm source and explicit helper source"
    );
}

struct MacroLoopVisitor<'source> {
    source: &'source str,
    records: Vec<MacroLoopRecord>,
}

impl<'source> MacroLoopVisitor<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            records: Vec::new(),
        }
    }

    fn record(&mut self, macro_name: String, origin: MacroOrigin, tokens: &TokenStream) {
        let counts = token_counts(tokens);
        self.records.push(MacroLoopRecord {
            source: self.source.to_owned(),
            macro_name,
            origin,
            loops: counts.loops,
            checkpoints: counts.checkpoints,
        });
    }
}

impl<'ast> Visit<'ast> for MacroLoopVisitor<'_> {
    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if item.mac.path.is_ident("macro_rules") {
            let name = item
                .ident
                .as_ref()
                .expect("macro_rules! item has a name")
                .to_string();
            self.record(name, MacroOrigin::Definition, &item.mac.tokens);
        } else {
            self.record(
                macro_name(&item.mac),
                MacroOrigin::Invocation,
                &item.mac.tokens,
            );
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.record(macro_name(mac), MacroOrigin::Invocation, &mac.tokens);
    }
}

#[derive(Default)]
struct TokenCounts {
    loops: usize,
    checkpoints: usize,
}

fn token_counts(tokens: &TokenStream) -> TokenCounts {
    let mut counts = TokenCounts::default();
    for token in tokens.clone() {
        match token {
            TokenTree::Group(group) => {
                let nested = token_counts(&group.stream());
                counts.loops += nested.loops;
                counts.checkpoints += nested.checkpoints;
            }
            TokenTree::Ident(identifier)
                if matches!(identifier.to_string().as_str(), "loop" | "while" | "for") =>
            {
                counts.loops += 1;
            }
            TokenTree::Ident(identifier) if identifier == "vm_checkpoint" => {
                counts.checkpoints += 1;
            }
            _ => {}
        }
    }
    counts
}

fn checkpoints_cover_loops(loops: usize, checkpoints: usize) -> bool {
    loops == 0 || checkpoints >= loops
}

#[test]
fn macro_token_scanner_counts_nested_grouped_loops() {
    let tokens = quote::quote! {
        {
            loop { vm_checkpoint!(ctx); }
            if keep_going {
                while ready { vm_checkpoint!(ctx); }
            }
            nested!({ for item in items { vm_checkpoint!(ctx); } });
        }
    };

    let counts = token_counts(&tokens);
    assert_eq!(counts.loops, 3);
    assert_eq!(counts.checkpoints, 3);
    assert!(checkpoints_cover_loops(counts.loops, counts.checkpoints));
}

#[test]
fn macro_token_scanner_rejects_one_checkpoint_for_multiple_loops() {
    let tokens = quote::quote! {
        {
            loop {}
            while ready { vm_checkpoint!(ctx); }
            for item in items {}
        }
    };

    let counts = token_counts(&tokens);
    assert_eq!(counts.loops, 3);
    assert_eq!(counts.checkpoints, 1);
    assert!(!checkpoints_cover_loops(counts.loops, counts.checkpoints));
}

fn macro_name(mac: &syn::Macro) -> String {
    mac.path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

struct LoopVisitor<'source> {
    source: &'source str,
    function_stack: Vec<String>,
    loops: Vec<LoopRecord>,
}

impl<'source> LoopVisitor<'source> {
    fn new(source: &'source str) -> Self {
        Self {
            source,
            function_stack: Vec::new(),
            loops: Vec::new(),
        }
    }

    fn with_function<F>(&mut self, function: String, visit: F)
    where
        F: FnOnce(&mut Self),
    {
        self.function_stack.push(function);
        visit(self);
        self.function_stack.pop();
    }

    fn record_loop(&mut self, identifier: String, body: &Block, has_small_literal_bound: bool) {
        self.loops.push(LoopRecord {
            source: self.source.to_owned(),
            function: self
                .function_stack
                .last()
                .cloned()
                .unwrap_or_else(|| "<module>".to_owned()),
            identifier,
            has_direct_checkpoint: body.stmts.iter().any(is_direct_checkpoint),
            has_small_literal_bound,
        });
    }
}

impl<'ast> Visit<'ast> for LoopVisitor<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.with_function(item.sig.ident.to_string(), |visitor| {
            visit::visit_item_fn(visitor, item);
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.with_function(item.sig.ident.to_string(), |visitor| {
            visit::visit_impl_item_fn(visitor, item);
        });
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        self.with_function(item.sig.ident.to_string(), |visitor| {
            visit::visit_trait_item_fn(visitor, item);
        });
    }

    fn visit_expr_loop(&mut self, expression: &'ast ExprLoop) {
        self.record_loop("loop".to_owned(), &expression.body, false);
        visit::visit_expr_loop(self, expression);
    }

    fn visit_expr_while(&mut self, expression: &'ast ExprWhile) {
        self.record_loop(
            format!("while:{}", expression.cond.to_token_stream()),
            &expression.body,
            false,
        );
        visit::visit_expr_while(self, expression);
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast ExprForLoop) {
        self.record_loop(
            format!("for:{}", expression.expr.to_token_stream()),
            &expression.body,
            is_small_literal_range(&expression.expr),
        );
        visit::visit_expr_for_loop(self, expression);
    }
}

fn is_direct_checkpoint(statement: &Stmt) -> bool {
    match statement {
        Stmt::Macro(statement) => {
            is_checkpoint_macro(&statement.mac) || is_checkpoint_copy_callback(&statement.mac)
        }
        Stmt::Expr(Expr::Macro(expression), _) => {
            is_checkpoint_macro(&expression.mac) || is_checkpoint_copy_callback(&expression.mac)
        }
        _ => false,
    }
}

fn is_checkpoint_macro(mac: &syn::Macro) -> bool {
    mac.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "vm_checkpoint")
}

fn is_checkpoint_copy_callback(mac: &syn::Macro) -> bool {
    if !mac.path.is_ident("vm_try") {
        return false;
    }
    let Ok(Expr::Call(call)) = syn::parse2::<Expr>(mac.tokens.clone()) else {
        return false;
    };
    call.args.is_empty()
        && matches!(
            call.func.as_ref(),
            Expr::Path(path) if path.path.is_ident("checkpoint_copy_chunk")
        )
}

fn is_small_literal_range(expression: &Expr) -> bool {
    let Expr::Range(range) = expression else {
        return false;
    };
    let Some(end) = &range.end else {
        return false;
    };
    let Expr::Lit(literal) = end.as_ref() else {
        return false;
    };
    let syn::Lit::Int(value) = &literal.lit else {
        return false;
    };
    value.base10_parse::<u64>().is_ok_and(|value| value <= 1024)
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
