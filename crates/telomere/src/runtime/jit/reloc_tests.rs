//! Corpus, differential tests, and golden capture for the #197 JIT
//! relocation support.
//!
//! The corpus exercises every emission shape that produces a relocation:
//! stub calls (the `call_ptr` sites) and the wasm direct-call fast path
//! (the funcaddr immediate), plus the call shapes that decide between the
//! fast path and the generic helper (locally defined callee, imported host
//! callee, imported wasm callee, indirect callee).

use super::backend;
use super::emit_env::{Emission, ModuleFacts, RelocKind};
use super::stubs::{stub_table_fingerprint, StubId};
use crate::common::store::FunctionBody;
use crate::common::{
    ExecuteContext, FuncType, HostFunctionDefinition, Instr, NativeModule, VMResult, ValType,
};
use crate::runtime::instantiate::instantiate_native_module;
use crate::{instantiate, InstanceHandle, IoReadBinaryReader, Registry, Store, WasmParser};

fn host_inc(ctx: &mut ExecuteContext<'_>) -> VMResult<*const Instr> {
    let _ = ctx;
    VMResult::Unimplemented
}

fn parse_wat(wat: &str) -> crate::Module {
    let bytes = wat::parse_str(wat).expect("corpus wat must parse");
    let mut reader = IoReadBinaryReader::from(bytes.as_slice());
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module().expect("corpus module must parse")
}

/// One corpus module. `divergent_funcs` lists module function indices whose
/// AoT emission intentionally differs from JIT emission: functions that call
/// an imported *wasm* function, where #197 mandates the generic helper path
/// in AoT mode while the JIT takes the direct fast path.
struct CorpusEntry {
    name: &'static str,
    wat: &'static str,
    divergent_funcs: &'static [u32],
}

const CORPUS: &[CorpusEntry] = &[
    CorpusEntry {
        name: "calls",
        wat: r#"
(module
  (type $bin (func (param i32 i32) (result i32)))
  (func $leaf (type $bin) local.get 0 local.get 1 i32.add)
  (func $mid (param i32) (result i32) local.get 0 i32.const 7 call $leaf)
  (func $rec (param i32) (result i32)
    local.get 0 i32.eqz
    if (result i32) i32.const 0
    else local.get 0 i32.const 1 i32.sub call $rec end)
  (func $tail (type $bin) local.get 0 local.get 1 return_call $leaf)
  (func (export "run") (param i32) (result i32) local.get 0 call $mid)
  (table $t 4 funcref)
  (elem (i32.const 0) $leaf $mid)
  (func $ind (param i32 i32 i32) (result i32)
    local.get 0 local.get 1 local.get 2 call_indirect (type $bin))
  (func $rf (result funcref) ref.func $leaf)
  (func $many (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
    (local i32 i32 i32 i32 i32 i32 i32 i32)
    local.get 0 local.get 1 i32.add local.get 2 i32.add local.get 3 i32.add
    local.get 4 i32.add local.get 5 i32.add local.get 6 i32.add local.get 7 i32.add)
  (func $callmany (param i32) (result i32)
    local.get 0 local.get 0 local.get 0 local.get 0
    local.get 0 local.get 0 local.get 0 local.get 0
    call $many)
)
"#,
        divergent_funcs: &[],
    },
    CorpusEntry {
        name: "provider",
        wat: r#"
(module
  (func (export "seven") (result i32) i32.const 7)
  (func (export "dbl") (param i32) (result i32) local.get 0 i32.const 2 i32.mul)
)
"#,
        divergent_funcs: &[],
    },
    CorpusEntry {
        name: "consumer",
        wat: r#"
(module
  (import "host" "inc" (func $inc (param i32) (result i32)))
  (import "wasmhost" "dbl" (func $dbl (param i32) (result i32)))
  (func $local (param i32) (result i32) local.get 0 i32.const 3 i32.add)
  (func (export "via_host") (param i32) (result i32) local.get 0 call $inc)
  (func (export "via_wasm_import") (param i32) (result i32) local.get 0 call $dbl)
  (func (export "via_local") (param i32) (result i32) local.get 0 call $local)
)
"#,
        // funcidx 4 = via_wasm_import: calls imported wasm funcidx 1.
        divergent_funcs: &[4],
    },
    CorpusEntry {
        name: "arith",
        wat: r#"
(module
  (func (export "pop") (param i32 i64) (result i32 i64)
    local.get 0 i32.popcnt local.get 1 i64.popcnt)
  (func (export "tz") (param i32 i64) (result i32 i64)
    local.get 0 i32.ctz local.get 1 i64.clz)
  (func (export "tr") (param f32 f64) (result i32 i64 i32 i64)
    local.get 0 i32.trunc_f32_s local.get 1 i64.trunc_f64_s
    local.get 0 i32.trunc_f32_u local.get 1 i64.trunc_f64_u)
  (func (export "trs") (param f32 f64) (result i32 i64 i32 i64)
    local.get 0 i32.trunc_sat_f32_s local.get 1 i64.trunc_sat_f64_s
    local.get 0 i32.trunc_sat_f32_u local.get 1 i64.trunc_sat_f64_u)
  (func (export "mm") (param f32 f64) (result f32 f64 f32 f64)
    local.get 0 local.get 0 f32.min local.get 1 local.get 1 f64.max
    local.get 0 local.get 0 f32.max local.get 1 local.get 1 f64.min)
  (func (export "round") (param f32 f64)
    (result f32 f64 f32 f64 f32 f64 f32 f64)
    local.get 0 f32.ceil local.get 1 f64.ceil
    local.get 0 f32.floor local.get 1 f64.floor
    local.get 0 f32.trunc local.get 1 f64.trunc
    local.get 0 f32.nearest local.get 1 f64.nearest)
)
"#,
        divergent_funcs: &[],
    },
    CorpusEntry {
        name: "memory",
        wat: r#"
(module
  (memory (export "m") 1 4)
  (func (export "sz") (result i32) memory.size)
  (func (export "gr") (param i32) (result i32) local.get 0 memory.grow)
  (func (export "fl") (param i32 i32 i32)
    local.get 0 local.get 1 local.get 2 memory.fill)
  (func (export "cp") (param i32 i32 i32)
    local.get 0 local.get 1 local.get 2 memory.copy)
  (func (export "ld") (param i32) (result i32 i64)
    local.get 0 i32.load local.get 0 i64.load32_u)
  (func (export "st") (param i32 i64)
    local.get 0 local.get 1 i64.store
    local.get 0 local.get 1 i32.wrap_i64 i32.store8)
  (data (i32.const 16) "hello world")
)
"#,
        divergent_funcs: &[],
    },
    CorpusEntry {
        name: "control",
        wat: r#"
(module
  (global $g (mut i32) (i32.const 0))
  (func (export "blk") (param i32) (result i32)
    (block $b (result i32)
      local.get 0 i32.const 1 i32.gt_s
      if local.get 0 br $b end
      i32.const 42))
  (func (export "lp") (param i32) (result i32)
    (local i32)
    (loop $l
      local.get 1 i32.const 1 i32.add local.tee 1
      local.get 0 i32.lt_s br_if $l)
    local.get 1)
  (func (export "sel") (param i32 i32 i32) (result i32)
    local.get 0 local.get 1 local.get 2 select)
  (func (export "gs") (param i32) (result i32)
    local.get 0 global.set $g global.get $g)
  (func (export "bt") (param i32) (result i32)
    (block $exit (result i32)
      (block $case2
        (block $case1
          (block $case0
            local.get 0
            br_table $case0 $case1 $case2)
          i32.const 10
          br $exit)
        i32.const 20
        br $exit)
      i32.const 30))
  (func (export "wide") (param i64 i64 i64 i64 i64 i64 i64 i64 i64 i64)
    (result i64 i64 i64 i64)
    local.get 0 local.get 1 i64.add
    local.get 2 local.get 3 i64.add
    local.get 4 local.get 5 i64.add
    local.get 6 local.get 7 i64.add)
  (func (export "callwide") (param i64) (result i64 i64 i64 i64)
    local.get 0 local.get 0 local.get 0 local.get 0 local.get 0
    local.get 0 local.get 0 local.get 0 local.get 0 local.get 0
    call 5)
)
"#,
        divergent_funcs: &[],
    },
    CorpusEntry {
        name: "tables",
        wat: r#"
(module
  (type $v (func))
  (type $ii (func (param i32) (result i32)))
  (table $t 2 8 funcref)
  (func $a (type $ii) local.get 0 i32.const 1 i32.add)
  (func $b (type $v))
  (elem (i32.const 0) $a $b)
  (func (export "ci") (param i32 i32) (result i32)
    local.get 0 local.get 1 call_indirect (type $ii))
  (func (export "tg") (param i32) (result funcref) local.get 0 table.get $t)
  (func (export "ts") (param i32) local.get 0 ref.func $b table.set $t)
  (func (export "tsz") (result i32) table.size $t)
  (func (export "tgrow") (param i32) (result i32)
    ref.null func local.get 0 table.grow $t)
)
"#,
        divergent_funcs: &[],
    },
    CorpusEntry {
        name: "atomics",
        wat: r#"
(module
  (memory 1 1 shared)
  (func (export "al") (param i32) (result i32) local.get 0 i32.atomic.load)
  (func (export "as") (param i32 i32) local.get 0 local.get 1 i32.atomic.store)
  (func (export "aa") (param i32 i32) (result i32)
    local.get 0 local.get 1 i32.atomic.rmw.add)
  (func (export "fence") atomic.fence)
)
"#,
        divergent_funcs: &[],
    },
];

struct CorpusInstance {
    name: &'static str,
    handle: InstanceHandle,
    module: crate::Module,
    divergent_funcs: &'static [u32],
}

/// Instantiates the host native module, the provider module (registered as
/// `wasmhost`), and every corpus module into one store.
async fn setup_corpus() -> (Store, Vec<CorpusInstance>) {
    let store = Store::new();
    let mut registry = Registry::new();
    let host = match instantiate_native_module(
        NativeModule {
            functions: vec![HostFunctionDefinition {
                name: Some("inc".to_owned()),
                signature: FuncType::new(vec![ValType::I32], vec![ValType::I32]),
                fp: host_inc,
            }],
        },
        &store,
        &registry,
    )
    .await
    {
        VMResult::Success(instance) => instance,
        other => panic!("host module must instantiate, got {other:?}"),
    };
    registry.register("host", host);

    let mut instances = Vec::new();
    for entry in CORPUS {
        let module = parse_wat(entry.wat);
        let kept = module.clone();
        let handle = match instantiate(module, &store, &registry).await {
            VMResult::Success(instance) => instance,
            other => panic!(
                "corpus module {} must instantiate, got {other:?}",
                entry.name
            ),
        };
        if entry.name == "provider" {
            registry.register("wasmhost", handle.clone());
        }
        instances.push(CorpusInstance {
            name: entry.name,
            handle,
            module: kept,
            divergent_funcs: entry.divergent_funcs,
        });
    }
    (store, instances)
}

/// JIT-emits every defined function of one instance with the production
/// entry point. Returns `(funcidx, result)` pairs in function index order.
fn emit_all_jit(store: &Store, instance: &CorpusInstance) -> Vec<(u32, Result<Vec<u8>, String>)> {
    let runtime = store.lock_runtime_or_panic();
    let instance_addr = instance
        .handle
        .object_ref_for_store(store)
        .expect("instance must belong to the store");
    let instance_data = runtime.get_instance(instance_addr);
    let mut out = Vec::new();
    for funcaddr in &instance_data.funcs {
        let func = runtime.get_func(*funcaddr);
        if let FunctionBody::Wasm { code, op_lens, .. } = &func.body {
            let result = backend::emit_baseline_function(*funcaddr, code, op_lens, &runtime)
                .map(|emitted| emitted.code)
                .map_err(|err| format!("{err:?}"));
            out.push((func.funcidx, result));
        }
    }
    out
}

fn all_stub_addresses() -> Vec<u64> {
    StubId::ALL
        .iter()
        .map(|id| id.address() as u64)
        .filter(|addr| *addr != 0)
        .collect()
}

/// AoT-emits every defined function of one instance from the parsed
/// module's facts. Returns `(funcidx, result)` pairs in function index
/// order, aligned with [`emit_all_jit`].
fn emit_all_aot(store: &Store, instance: &CorpusInstance) -> Vec<(u32, Result<Emission, String>)> {
    let runtime = store.lock_runtime_or_panic();
    let instance_addr = instance
        .handle
        .object_ref_for_store(store)
        .expect("instance must belong to the store");
    let instance_data = runtime.get_instance(instance_addr);
    let mut out = Vec::new();
    for funcaddr in &instance_data.funcs {
        let func = runtime.get_func(*funcaddr);
        if let FunctionBody::Wasm { code, op_lens, .. } = &func.body {
            let result = backend::emit_baseline_function_aot(
                code,
                op_lens,
                ModuleFacts::new(&instance.module),
            )
            .map_err(|err| format!("{err:?}"));
            out.push((func.funcidx, result));
        }
    }
    out
}

/// The funcaddr `ObjectRef` values of one instance, in function index
/// order: the resolver input for `FuncAddr` relocations.
fn instance_func_refs(store: &Store, instance: &CorpusInstance) -> Vec<u32> {
    let runtime = store.lock_runtime_or_panic();
    let instance_addr = instance
        .handle
        .object_ref_for_store(store)
        .expect("instance must belong to the store");
    let instance_data = runtime.get_instance(instance_addr);
    instance_data
        .funcs
        .iter()
        .map(|funcaddr| funcaddr.get())
        .collect()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Hash of the emitted bytes with process-local values masked out: stub
/// addresses (ASLR-dependent) and funcaddr `ObjectRef`s (allocation
/// dependent) differ between processes, so a cross-process golden comparison
/// must not see them. Everything else in the byte stream is a pure function
/// of the module.
fn masked_hash(bytes: &[u8], stub_addrs: &[u64], func_refs: &[u32]) -> u64 {
    let mut masked = bytes.to_vec();
    if masked.len() >= 8 {
        let mut i = 0;
        while i + 8 <= masked.len() {
            let value = u64::from_le_bytes(masked[i..i + 8].try_into().unwrap());
            if stub_addrs.contains(&value) {
                masked[i..i + 8].fill(0);
                i += 8;
            } else {
                i += 1;
            }
        }
    }
    if masked.len() >= 4 {
        let mut i = 0;
        while i + 4 <= masked.len() {
            let value = u32::from_le_bytes(masked[i..i + 4].try_into().unwrap());
            if func_refs.contains(&value) {
                masked[i..i + 4].fill(0);
                i += 4;
            } else {
                i += 1;
            }
        }
    }
    fnv1a64(&masked)
}

/// Lane-local golden capture for #197 acceptance (a): run on the
/// unmodified emitter and again after the refactor; the printed lines must
/// be identical on x86_64. Not part of CI.
#[tokio::test]
#[ignore = "lane-local golden capture for #197; run manually with --nocapture"]
async fn reloc_golden_dump() {
    let (store, instances) = setup_corpus().await;
    let stub_addrs = all_stub_addresses();
    let mut func_refs: Vec<u32> = Vec::new();
    {
        let runtime = store.lock_runtime_or_panic();
        for instance in &instances {
            let instance_addr = instance.handle.object_ref_for_store(&store).unwrap();
            let instance_data = runtime.get_instance(instance_addr);
            for funcaddr in &instance_data.funcs {
                func_refs.push(funcaddr.get());
            }
        }
    }
    for instance in &instances {
        for (funcidx, result) in emit_all_jit(&store, instance) {
            match result {
                Ok(bytes) => println!(
                    "GOLDEN {} funcidx={} len={} fnv={:016x}",
                    instance.name,
                    funcidx,
                    bytes.len(),
                    masked_hash(&bytes, &stub_addrs, &func_refs)
                ),
                Err(err) => println!("GOLDEN {} funcidx={} ERR {}", instance.name, funcidx, err),
            }
        }
    }
}

/// #197 acceptance (b): an AoT emission patched with the current
/// process's stub addresses and the instance's funcaddrs is byte-identical
/// to the JIT emission of the same function from the same store. Also
/// asserts error parity (a function one mode rejects, the other must
/// reject too) and that no `FuncAddr` relocation ever targets an imported
/// function index.
#[tokio::test]
async fn reloc_differential_patched_aot_equals_jit() {
    let (store, instances) = setup_corpus().await;
    let mut distinct_stubs = std::collections::BTreeSet::new();
    let mut funcaddr_relocs = 0usize;
    let mut compared = 0usize;
    for instance in &instances {
        let func_refs = instance_func_refs(&store, instance);
        let imported = ModuleFacts::new(&instance.module).imported_func_count();
        let jit = emit_all_jit(&store, instance);
        let aot = emit_all_aot(&store, instance);
        assert_eq!(
            jit.len(),
            aot.len(),
            "{}: function count mismatch",
            instance.name
        );
        for ((jit_funcidx, jit_result), (aot_funcidx, aot_result)) in jit.iter().zip(aot.iter()) {
            assert_eq!(jit_funcidx, aot_funcidx);
            let funcidx = *jit_funcidx;
            match (jit_result, aot_result) {
                (Err(jit_err), Err(aot_err)) => {
                    assert_eq!(
                        jit_err, aot_err,
                        "{} funcidx={}: error mismatch",
                        instance.name, funcidx
                    );
                }
                (Ok(jit_bytes), Ok(emission)) => {
                    for reloc in &emission.relocs {
                        match reloc.kind {
                            RelocKind::Stub(id) => {
                                distinct_stubs.insert(id);
                            }
                            RelocKind::FuncAddr(callee) => {
                                funcaddr_relocs += 1;
                                assert!(
                                    callee >= imported,
                                    "{} funcidx={}: FuncAddr relocation targets imported funcidx {}",
                                    instance.name, funcidx, callee
                                );
                            }
                        }
                    }
                    if instance.divergent_funcs.contains(&funcidx) {
                        continue;
                    }
                    let mut patched = emission.bytes.clone();
                    backend::patch_relocations(
                        &mut patched,
                        &emission.relocs,
                        &|kind| match kind {
                            RelocKind::Stub(id) => id.address() as u64,
                            RelocKind::FuncAddr(callee) => u64::from(func_refs[callee as usize]),
                        },
                    )
                    .expect("patching the AoT emission must succeed");
                    assert_eq!(
                        &patched, jit_bytes,
                        "{} funcidx={}: patched AoT bytes differ from JIT bytes",
                        instance.name, funcidx
                    );
                    compared += 1;
                }
                (jit_result, aot_result) => {
                    panic!(
                        "{} funcidx={}: JIT/AoT success mismatch (jit ok: {}, aot ok: {})",
                        instance.name,
                        funcidx,
                        jit_result.is_ok(),
                        aot_result.is_ok()
                    );
                }
            }
        }
    }
    assert!(
        compared >= 40,
        "corpus must compare at least 40 functions, got {compared}"
    );
    assert!(
        distinct_stubs.len() >= 12,
        "corpus must exercise at least 12 distinct stubs, got {}",
        distinct_stubs.len()
    );
    assert!(
        funcaddr_relocs > 0,
        "corpus must produce FuncAddr relocations"
    );
}

/// #197's AoT call rule, checked directly on the consumer module: a call
/// to an imported wasm function records no `FuncAddr` relocation (generic
/// helper path), a call to an imported host function records none either,
/// and a call to a locally defined function records exactly one.
#[tokio::test]
async fn reloc_aot_imported_callees_take_generic_path() {
    let (store, instances) = setup_corpus().await;
    let consumer = instances
        .iter()
        .find(|instance| instance.name == "consumer")
        .expect("corpus has a consumer module");
    let aot = emit_all_aot(&store, consumer);
    let funcaddr_relocs = |funcidx: u32| -> Vec<u32> {
        let (_, result) = aot
            .iter()
            .find(|(candidate, _)| *candidate == funcidx)
            .expect("function must be emitted");
        let emission = result.as_ref().expect("emission must succeed");
        emission
            .relocs
            .iter()
            .filter_map(|reloc| match reloc.kind {
                RelocKind::FuncAddr(callee) => Some(callee),
                RelocKind::Stub(_) => None,
            })
            .collect()
    };
    // funcidx 3 = via_host (calls imported host funcidx 0)
    assert_eq!(funcaddr_relocs(3), Vec::<u32>::new());
    // funcidx 4 = via_wasm_import (calls imported wasm funcidx 1)
    assert_eq!(funcaddr_relocs(4), Vec::<u32>::new());
    // funcidx 5 = via_local (calls locally defined funcidx 2)
    assert_eq!(funcaddr_relocs(5), vec![2]);
}

/// The stub table is the artifact-facing source of truth: 44 entries in
/// stable discriminant order, unique names, and a deterministic
/// fingerprint.
#[test]
fn reloc_stub_table_is_complete_and_stable() {
    assert_eq!(StubId::ALL.len(), 44);
    for (index, id) in StubId::ALL.iter().enumerate() {
        assert_eq!(
            *id as u32, index as u32,
            "discriminant order must be stable"
        );
        assert_eq!(StubId::from_discriminant(index as u32), Some(*id));
    }
    assert_eq!(StubId::from_discriminant(44), None);
    let mut names: Vec<&str> = StubId::ALL.iter().map(|id| id.name()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 44, "stub names must be unique");
    // Every stub compiled into this target must resolve to a real
    // address. The rounding stubs are riscv64-only and the i32 trunc_sat
    // pair is compiled out on aarch64; both are null here by design.
    for id in StubId::ALL {
        let compiled_out = (matches!(
            id,
            StubId::F32CeilBits
                | StubId::F32FloorBits
                | StubId::F32TruncBits
                | StubId::F32NearestBits
                | StubId::F64CeilBits
                | StubId::F64FloorBits
                | StubId::F64TruncBits
                | StubId::F64NearestBits
        ) && !cfg!(all(
            target_os = "linux",
            target_arch = "riscv64",
            target_env = "gnu"
        ))) || (matches!(id, StubId::I32TruncSatF32 | StubId::I32TruncSatF64)
            && cfg!(target_arch = "aarch64"));
        if compiled_out {
            assert!(
                id.address().is_null(),
                "{} must be null on this target",
                id.name()
            );
        } else {
            assert!(!id.address().is_null(), "{} must resolve", id.name());
        }
    }
    let fingerprint = stub_table_fingerprint();
    assert_ne!(fingerprint, 0);
    assert_eq!(
        fingerprint,
        stub_table_fingerprint(),
        "fingerprint must be deterministic"
    );
}

/// Patch-site verification (#197): patching must refuse bytes whose
/// placeholder does not match the recorded relocation kind, instead of
/// writing an address over unrelated code.
#[tokio::test]
async fn reloc_patch_verifies_placeholder() {
    let (store, instances) = setup_corpus().await;
    let calls = instances
        .iter()
        .find(|instance| instance.name == "calls")
        .expect("corpus has a calls module");
    let func_refs = instance_func_refs(&store, calls);
    let aot = emit_all_aot(&store, calls);
    let mut checked = 0usize;
    for (_, result) in &aot {
        let emission = result.as_ref().expect("emission must succeed");
        if emission.relocs.is_empty() {
            continue;
        }
        let resolve = |kind: RelocKind| match kind {
            RelocKind::Stub(id) => id.address() as u64,
            RelocKind::FuncAddr(callee) => u64::from(func_refs[callee as usize]),
        };
        // Corrupt one placeholder byte: patching must fail and leave the
        // bytes untouched.
        let first = emission.relocs[0];
        let mut corrupted = emission.bytes.clone();
        corrupted[first.offset as usize] ^= 0xff;
        let before = corrupted.clone();
        assert!(
            backend::patch_relocations(&mut corrupted, &emission.relocs, &resolve).is_err(),
            "patching a corrupted placeholder must fail"
        );
        assert_eq!(corrupted, before, "a failed patch must not modify bytes");
        // Truncated bytes must fail too.
        let mut truncated = emission.bytes.clone();
        truncated.truncate(4);
        assert!(backend::patch_relocations(&mut truncated, &emission.relocs, &resolve).is_err());
        checked += 1;
    }
    assert!(checked > 0, "corpus must produce relocations to verify");
}
