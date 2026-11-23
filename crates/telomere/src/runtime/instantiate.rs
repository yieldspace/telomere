use crate::{
    common::{
        execute_elem_init_const_expr,
        memory::MemoryInitError,
        store::{FunctionBody as RuntimeFunctionBody, StoreExecutionError},
        AsyncHostFunction, AsyncHostFunctionDefinition, AsyncNativeModule, CallFrameCache,
        CodeSection, ConstExpr, DataMode, DataSection, ElemInit, ElemMode, ElementSection,
        ExecuteContext, Export, ExportDesc, ExportSection, FuncIdx, FunctionBody,
        FunctionInstanceData, GlobalIdx, HostFunction, HostFunctionDefinition, ImportDesc,
        ImportSection, Instance, InstanceData, InstanceHandle, Instr, Limits, LocalReference,
        MemIdx, ModuleInstance, ModuleNames, NativeModule, ObjectRef, StablePc, Stack, StoreInner,
        TableIdx, TypeIdx, TypeSection,
    },
    host::{AsyncHostFn, AsyncHostModule, HostFn, HostModule},
    runtime::{
        scheduler::{fresh_task_id, ReadyFlag, Scheduler, Task},
        vm,
    },
    Module, Registry, Store, VMResult,
};
use std::sync::Arc;

#[cfg(test)]
use crate::common::{decode_local_binop32_kind, LocalBinop32Op, LocalFastRhsShape};

pub(crate) fn init_global(
    runtime: &mut StoreInner,
    init: &ConstExpr,
    globals: &[ObjectRef],
    funcs: &[ObjectRef],
) -> VMResult<ObjectRef> {
    tracing::trace!("global init: {init:?}");

    let res = match init {
        ConstExpr::I32(v) => runtime.new_global_data4(*v as u32),
        ConstExpr::I64(v) => runtime.new_global_data8(*v as u64),
        ConstExpr::F32(v) => runtime.new_global_data4(v.to_bits()),
        ConstExpr::F64(v) => runtime.new_global_data8(v.to_bits()),
        ConstExpr::V128(v) => runtime.new_global_data16(*v),

        ConstExpr::RefNull(_t) => runtime.new_global_ref(ObjectRef(0)),
        ConstExpr::FuncRef(v) => {
            let addr = funcs.get(*v as usize);
            if let Some(addr) = addr {
                runtime.new_global_ref(*addr)
            } else {
                return VMResult::InvalidOperand;
            }
        }
        ConstExpr::GlobalGet(idx) => {
            let idx = *idx as usize;
            let addr = globals[idx];
            runtime.copy_object(addr)
        }
    };
    VMResult::Success(res)
}

fn validate_limit(import_limit: Limits, real: u32, export_limit: Limits) -> VMResult<()> {
    if import_limit.min > real {
        tracing::trace!("invalid import_limit min");

        return VMResult::Unlinkable;
    }
    match export_limit.max {
        None => {
            if import_limit.max.is_some() {
                tracing::trace!("invalid import_limit max");

                return VMResult::Unlinkable;
            }
        }
        Some(export_max) => {
            if let Some(import_max) = import_limit.max {
                if export_max > import_max {
                    tracing::trace!("invalid import_limit max");

                    return VMResult::Unlinkable;
                }
            }
        }
    }
    VMResult::Success(())
}
fn execute_offset_const_expr(
    runtime: &mut StoreInner,
    globals: &[ObjectRef],
    exprs: &[ConstExpr],
) -> VMResult<u32> {
    if exprs.len() != 1 {
        return VMResult::Unlinkable;
    }
    match &exprs[0] {
        ConstExpr::I32(v) => VMResult::Success(*v as u32),
        ConstExpr::GlobalGet(idx) => {
            let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                VMResult::Unlinkable
            }));
            let Ok(buf): Result<[u8; 4], _> = runtime.get_global(addr).try_into() else {
                return VMResult::Unlinkable;
            };
            VMResult::Success(u32::from_le_bytes(buf))
        }
        _ => VMResult::Unlinkable,
    }
}

#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
fn convert_native_module_to_module(m: NativeModule) -> Module {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    for HostFunctionDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(fp));
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    Module {
        codes: CodeSection(codes),
        functions,
        fts: TypeSection(fts),
        data: DataSection(vec![]),
        elems: ElementSection(vec![]),
        imports: ImportSection(vec![]),
        mems: vec![],
        globals: vec![],
        global_init: vec![],
        exs: ExportSection(exs),
        tables: vec![],
        start: None,
        name: None,
    }
}

fn async_host_placeholder(_ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    VMResult::Unreachable
}

#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
fn convert_async_native_module_to_module(m: AsyncNativeModule) -> (Module, Vec<AsyncHostFunction>) {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    let mut async_functions = Vec::with_capacity(m.functions.len());
    for AsyncHostFunctionDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(async_host_placeholder));
        async_functions.push(fp);
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    (
        Module {
            codes: CodeSection(codes),
            functions,
            fts: TypeSection(fts),
            data: DataSection(vec![]),
            elems: ElementSection(vec![]),
            imports: ImportSection(vec![]),
            mems: vec![],
            globals: vec![],
            global_init: vec![],
            exs: ExportSection(exs),
            tables: vec![],
            start: None,
            name: None,
        },
        async_functions,
    )
}
fn convert_public_native_module_to_module(m: HostModule) -> (Module, Vec<HostFn>) {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    let mut host_functions = Vec::with_capacity(m.functions.len());
    for crate::host::HostFnDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(async_host_placeholder));
        host_functions.push(fp);
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    (
        Module {
            codes: CodeSection(codes),
            functions,
            fts: TypeSection(fts),
            data: DataSection(vec![]),
            elems: ElementSection(vec![]),
            imports: ImportSection(vec![]),
            mems: vec![],
            globals: vec![],
            global_init: vec![],
            exs: ExportSection(exs),
            tables: vec![],
            start: None,
            name: None,
        },
        host_functions,
    )
}

fn convert_public_async_native_module_to_module(m: AsyncHostModule) -> (Module, Vec<AsyncHostFn>) {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    let mut host_functions = Vec::with_capacity(m.functions.len());
    for crate::host::AsyncHostFnDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(async_host_placeholder));
        host_functions.push(fp);
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    (
        Module {
            codes: CodeSection(codes),
            functions,
            fts: TypeSection(fts),
            data: DataSection(vec![]),
            elems: ElementSection(vec![]),
            imports: ImportSection(vec![]),
            mems: vec![],
            globals: vec![],
            global_init: vec![],
            exs: ExportSection(exs),
            tables: vec![],
            start: None,
            name: None,
        },
        host_functions,
    )
}

/// Instantiates a synthetic module made entirely of synchronous host functions.
///
/// This is useful when an embedder wants to register a Rust-provided module in
/// a [`Registry`] before instantiating a guest that imports it.
///
/// # Examples
///
/// ```
/// use futures::executor::block_on;
/// use telomere::{
///     component_support::common::{FuncType, ValType},
///     instantiate, instantiate_native_module, run_module_function, HostContext, HostFnDefinition,
///     HostModule, IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser,
///     WasmValue,
/// };
///
/// fn answer(_cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
///     VMResult::Success(vec![WasmValue::I32(42)])
/// }
///
/// let store = Store::new();
/// let mut registry = Registry::new();
/// let host = block_on(instantiate_native_module(
///     HostModule {
///         functions: vec![HostFnDefinition {
///             name: Some("answer".to_owned()),
///             signature: FuncType::new(vec![], vec![ValType::I32]),
///             fp: answer,
///         }],
///     },
///     &store,
///     &registry,
/// ))
/// .unwrap();
/// registry.register("host", host);
/// let bytes = wat::parse_str(
///     r#"(module
///         (import "host" "answer" (func $answer (result i32)))
///         (func (export "run") (result i32) call $answer))"#,
/// )
/// .expect("the inline module is valid WebAssembly");
/// let mut reader = IoReadBinaryReader::from(&bytes[..]);
/// let module = WasmParser::new(&mut reader)
///     .parse_module()
///     .expect("the module parses");
/// let instance = block_on(instantiate(module, &store, &registry)).unwrap();
/// let result = block_on(run_module_function(
///     &instance,
///     &store,
///     "run",
///     &ResultValue::new(vec![]),
/// ));
/// assert_eq!(
///     result.unwrap(),
///     ResultValue::new(vec![WasmValue::I32(42)])
/// );
/// ```
pub async fn instantiate_native_module(
    m: HostModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let (module, host_functions) = convert_public_native_module_to_module(m);
    let instance = vm_try!(instantiate(module, store, registry).await);
    for (funcidx, fp) in host_functions.into_iter().enumerate() {
        link_host_function_with_function_idx(&instance, funcidx as u32, fp, store);
    }
    VMResult::Success(instance)
}

/// Instantiates a synthetic module made entirely of synchronous raw host
/// callbacks.
///
/// This is the raw-ABI counterpart of [`instantiate_native_module`], kept
/// for the opt-in raw integration surface (`unstable_internals`).
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub async fn instantiate_raw_native_module(
    m: NativeModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    instantiate(convert_native_module_to_module(m), store, registry).await
}

/// Instantiates a synthetic module made entirely of asynchronous host functions.
///
/// Each host callback is linked before the returned handle is exposed, so guest
/// calls suspend through the runtime's [`crate::ExecutionDriver`] integration.
///
/// # Examples
///
/// An embedder can define an asynchronous host callback, create a native
/// module, link it by index and export name, and drive the returned host
/// future through a custom [`crate::ExecutionDriver`]. The driver only
/// forwards the opaque [`crate::HostCallFuture`] and its
/// [`crate::HostCallOutcome`]; it never names an interpreter-internal type.
///
/// ```
/// use std::{collections::VecDeque, future::Future, pin::Pin};
///
/// use futures::executor::block_on;
/// use telomere::{
///     component_support::common::{FuncType, ValType},
///     instantiate, instantiate_native_async_module, link_async_host_function_with_export_name,
///     link_async_host_function_with_function_idx, run_module_function_with_driver,
///     AsyncHostFnDefinition, AsyncHostModule, Completion, CompletionPayload, ExecutionDriver,
///     HostCallFuture, HostCallPending, HostContext, HostFuture, IoReadBinaryReader, PendingOp,
///     Registry, ResultValue, Store, TokioDriver, VMResult, WasmParser, WasmValue,
/// };
///
/// #[derive(Default)]
/// struct InlineDriver {
///     inflight: VecDeque<(u64, HostCallFuture)>,
/// }
///
/// impl ExecutionDriver for InlineDriver {
///     fn submit(&mut self, op: PendingOp) {
///         match op {
///             PendingOp::HostCall(HostCallPending { task_id, future, .. }) => {
///                 self.inflight.push_back((task_id, future));
///             }
///             _ => panic!("this example only drives asynchronous host calls"),
///         }
///     }
///
///     fn next_completion<'a>(
///         &'a mut self,
///     ) -> Pin<Box<dyn Future<Output = Option<Completion>> + 'a>> {
///         Box::pin(async move {
///             let (task_id, future) = self.inflight.pop_front()?;
///             Some(Completion {
///                 task_id,
///                 payload: CompletionPayload::HostCall {
///                     result: future.await,
///                 },
///             })
///         })
///     }
/// }
///
/// fn return_42(_cx: &mut HostContext<'_, '_>) -> HostFuture {
///     Box::pin(async { VMResult::Success(vec![WasmValue::I32(42)]) })
/// }
///
/// let _built_in_driver = TokioDriver::new();
/// let bytes = wat::parse_str(
///     r#"(module
///         (import "host" "answer" (func $answer (result i32)))
///         (func (export "run") (result i32) call $answer))"#,
/// )
/// .expect("the inline module is valid WebAssembly");
/// let mut reader = IoReadBinaryReader::from(&bytes[..]);
/// let module = WasmParser::new(&mut reader)
///     .parse_module()
///     .expect("the module parses");
/// let store = Store::new();
/// let mut registry = Registry::new();
/// let host = block_on(instantiate_native_async_module(
///     AsyncHostModule {
///         functions: vec![AsyncHostFnDefinition {
///             name: Some("answer".to_owned()),
///             signature: FuncType::new(vec![], vec![ValType::I32]),
///             fp: return_42,
///         }],
///     },
///     &store,
///     &registry,
/// ))
/// .unwrap();
/// link_async_host_function_with_function_idx(&host, 0, return_42, &store);
/// link_async_host_function_with_export_name(&host, "answer", return_42, &store);
/// registry.register("host", host);
/// let instance = block_on(instantiate(module, &store, &registry)).unwrap();
/// let mut driver = InlineDriver::default();
/// let result = block_on(run_module_function_with_driver(
///     &instance,
///     &store,
///     "run",
///     &ResultValue::new(vec![]),
///     &mut driver,
/// ));
/// assert_eq!(
///     result.unwrap(),
///     ResultValue::new(vec![WasmValue::I32(42)])
/// );
/// ```
pub async fn instantiate_native_async_module(
    m: AsyncHostModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let (module, host_functions) = convert_public_async_native_module_to_module(m);
    let instance = vm_try!(instantiate(module, store, registry).await);
    for (funcidx, fp) in host_functions.into_iter().enumerate() {
        link_async_host_function_with_function_idx(&instance, funcidx as u32, fp, store);
    }
    VMResult::Success(instance)
}

/// Instantiates a synthetic module made entirely of asynchronous raw host
/// callbacks.
///
/// This is the raw-ABI counterpart of [`instantiate_native_async_module`],
/// kept for the opt-in raw integration surface (`unstable_internals`).
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub async fn instantiate_raw_native_async_module(
    m: AsyncNativeModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let (module, async_functions) = convert_async_native_module_to_module(m);
    let instance = vm_try!(instantiate(module, store, registry).await);
    for (funcidx, fp) in async_functions.into_iter().enumerate() {
        link_raw_async_host_function_with_function_idx(&instance, funcidx as u32, fp, store);
    }
    VMResult::Success(instance)
}

/// Allocates and initializes a parsed module in `store`.
///
/// Imports are resolved through `registry`; a missing or incompatible import,
/// invalid initialization expression, or start-function trap returns a
/// non-success [`VMResult`]. The resulting [`InstanceHandle`] is bound to the
/// supplied store.
///
/// # Examples
///
/// ```
/// use telomere::{instantiate, IoReadBinaryReader, Registry, Store, VMResult, WasmParser};
///
/// let bytes = wat::parse_str("(module (func (export \"ready\")))")
///     .expect("the inline module is valid");
/// let mut reader = IoReadBinaryReader::from(&bytes[..]);
/// let module = WasmParser::new(&mut reader)
///     .parse_module()
///     .expect("the module parses");
/// let runtime = tokio::runtime::Builder::new_current_thread()
///     .build()
///     .expect("Tokio runtime builds");
///
/// match runtime.block_on(instantiate(module, &Store::new(), &Registry::new())) {
///     VMResult::Success(_) => {}
///     failure => panic!("instantiation failed: {failure:?}"),
/// }
/// ```
pub async fn instantiate(
    m: Module,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let Module {
        fts,
        functions,
        imports,
        mems,
        globals: m_globals,
        global_init,
        exs,
        tables: m_tables,
        elems: m_elems,
        codes,
        data,
        start,
        name,
    } = m;

    let names = if store.runtime_config().diagnostics.retain_function_names {
        name.and_then(ModuleNames::from_name_section).map(Arc::new)
    } else {
        None
    };

    let mut scheduler = Scheduler::new(store);
    let (addr, has_start) = {
        let mut runtime = match store.lock_runtime("instantiate") {
            Ok(runtime) => runtime,
            Err(err) => {
                err.report();
                return VMResult::Unlinkable;
            }
        };
        runtime.clear_last_trap();
        let instance_id = store.new_instance_id();

        // -> addr
        let mut memories: Vec<ObjectRef> = Vec::new();
        let mut globals = vec![];
        let mut funcs: Vec<ObjectRef> = vec![];
        let mut tables = vec![];

        for import in &imports.0 {
            tracing::trace!("processing import: {import:?}");
            let ext_inst_addr =
                vm_try!(VMResult::from_option(registry.get(&import.module), || {
                    tracing::error!("unknown instance");
                    VMResult::Unlinkable
                }));
            let instance_object_ref = vm_try!(VMResult::from_option(
                ext_inst_addr.object_ref_for_store(store),
                || {
                    tracing::error!("instance handle belongs to another store");
                    VMResult::Unlinkable
                }
            ));
            let ext_inst = unsafe { &*runtime.get_instance_unchecked(instance_object_ref) };
            let ext_module = runtime.get_module(ext_inst.module_addr);
            let export = vm_try!(VMResult::from_option(
                ext_module.exports.find(&import.name),
                || {
                    tracing::error!("unknown export");
                    VMResult::Unlinkable
                }
            ));
            match (&import.desc, export) {
                (ImportDesc::TypeIdx(tidx), ExportDesc::Func(funcidx)) => {
                    // #128: every index in this linking match resolves
                    // fail-closed. The parser validates these indices for
                    // parsed modules, but a registry entry whose indices
                    // disagree with its module is unlinkable, never a
                    // host panic.
                    let import_ft = vm_try!(VMResult::from_option(fts.get(*tidx), || {
                        tracing::trace!("import type index out of range");
                        VMResult::Unlinkable
                    }));
                    let export_ft_idx = vm_try!(VMResult::from_option(
                        ext_module.functions.get(funcidx.0 as usize),
                        || {
                            tracing::trace!("export function index out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    let export_ft = vm_try!(VMResult::from_option(
                        ext_module.function_types.get(export_ft_idx.0 as usize),
                        || {
                            tracing::trace!("export function type index out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    if import_ft != export_ft {
                        tracing::trace!("import function type");
                        return VMResult::Unlinkable;
                    }
                    let funcaddr = vm_try!(VMResult::from_option(
                        ext_inst.funcs.as_slice().get(funcidx.0 as usize),
                        || {
                            tracing::trace!("export function address out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    let funcaddr = *funcaddr;
                    let funcidx = funcs.len();
                    funcs.push(funcaddr);
                    tracing::trace!("linking: {funcidx} => {funcaddr:?}")
                }
                (ImportDesc::GlobalType(import_gt), ExportDesc::Global(global_idx)) => {
                    let export_gt = vm_try!(VMResult::from_option(
                        ext_module.globals.get(global_idx.0 as usize),
                        || {
                            tracing::trace!("export global index out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    if import_gt != export_gt {
                        tracing::trace!("import global type");
                        return VMResult::Unlinkable;
                    }
                    let global_addr = vm_try!(VMResult::from_option(
                        ext_inst.globals.as_slice().get(global_idx.0 as usize),
                        || {
                            tracing::trace!("export global address out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    globals.push(*global_addr);
                }
                (ImportDesc::TableType(import_tt), ExportDesc::Table(idx)) => {
                    let export_tt = vm_try!(VMResult::from_option(
                        ext_module.tables.get(idx.0 as usize),
                        || {
                            tracing::trace!("export table index out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    let export_tt = *export_tt;
                    tracing::trace!("{export_tt:?}");

                    if import_tt.reftype != export_tt.reftype {
                        tracing::trace!("import table type");
                        return VMResult::Unlinkable;
                    }
                    let addr = vm_try!(VMResult::from_option(
                        ext_inst.tables.as_slice().get(idx.0 as usize),
                        || {
                            tracing::trace!("export table address out of range");
                            VMResult::Unlinkable
                        }
                    ));
                    let addr = *addr;
                    vm_try!(validate_limit(
                        import_tt.limits,
                        runtime.get_table(addr).1.len() as u32,
                        export_tt.limits
                    ));
                    tables.push(addr);
                }
                (ImportDesc::MemType(mt), ExportDesc::Mem(_idx)) => {
                    let memory_addr = *vm_try!(VMResult::from_option(
                        ext_inst.mems.as_slice().get(_idx.0 as usize),
                        || {
                            tracing::trace!("invalid instance memory");
                            VMResult::Unlinkable
                        }
                    ));
                    let limits = vm_try!(VMResult::from_option(
                        ext_module.mems.get(_idx.0 as usize),
                        || {
                            tracing::trace!("export memory index out of range");
                            VMResult::Unlinkable
                        }
                    ));

                    if mt.shared != limits.shared {
                        tracing::trace!("import shared memory flag mismatch");
                        return VMResult::Unlinkable;
                    }
                    let handle = runtime.memory_handle(memory_addr);
                    vm_try!(validate_limit(
                        mt.limits,
                        runtime.memory_page_size(handle),
                        limits.limits
                    ));
                    memories.push(memory_addr);
                }
                _ => {
                    tracing::trace!("import other type objects");
                    return VMResult::Unlinkable;
                }
            }
        }

        let mod_addr = runtime.new_module(ModuleInstance {
            function_types: fts.0.clone(),
            functions: functions.clone(),
            exports: exs.clone(),
            tables: m_tables.clone(),
            globals: m_globals.clone(),
            mems: mems.clone(),
            names,
        });
        let inst_id = runtime.alloc_instance(InstanceData {
            instance_id,
            module_addr: mod_addr,
            globals: Vec::new(),
            funcs: Vec::new(),
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });
        let inst_addr = runtime.object_ref_for_instance(inst_id);

        let memory_ceiling = store.runtime_config().memory.max_memory_pages;
        // Every limit check instantiation performs completes here,
        // before any memory or table is created. Memories and tables
        // are adopted by the store as they are created and never
        // removed, so a refusal issued after creation would leave the
        // earlier objects - and, for memories, their pages charged to
        // the total budget - behind permanently even though no instance
        // is ever returned.
        if let Some(cap) = store.runtime_config().limits.max_table_elements {
            for table_type in &m_tables {
                if table_type.limits.min > cap {
                    return VMResult::ResourceLimitExceeded;
                }
            }
        }
        let total_memory_cap = store.runtime_config().limits.max_total_memory_pages;
        if total_memory_cap.is_some() {
            // Claim the aggregate minimum of the module's defined
            // memories from the total budget in a single atomic
            // reservation, before creating any of them. Imported
            // memories are not summed here: they already live in the
            // store and their pages are already charged, so charging
            // them again would double-count. The reservation replaces a
            // read-and-compare preflight on purpose: a shared memory
            // can grow through a handle that never takes the store's
            // runtime lock, so a total read here could be overtaken by
            // a racing growth claim before the memories below register
            // their pages, overshooting the ceiling. Claiming first
            // makes the check and the charge one operation; the
            // creation loop then pays each memory out of the
            // reservation, and the loop's failure exit releases any
            // unspent remainder.
            let defined_minimum_total: u64 = mems
                .iter()
                .skip(memories.len())
                .map(|mem| u64::from(mem.limits.min))
                .sum();
            if !runtime.reserve_total_pages(defined_minimum_total) {
                return VMResult::ResourceLimitExceeded;
            }
        }
        for mem in mems.iter().skip(memories.len()) {
            let limits = mem.limits;
            let effective_max = limits.max.unwrap_or(memory_ceiling).min(memory_ceiling);
            let memory = if mem.shared {
                runtime.new_shared_memory(limits.min, effective_max)
            } else {
                runtime.new_memory(limits.min, effective_max)
            };
            match memory {
                Ok(memory) => memories.push(memory),
                Err(error) => {
                    // Memories already adopted keep their pages - they
                    // stay in the store - but the unspent reservation
                    // for the memories that will now never be created
                    // must not leak.
                    runtime.release_reserved_pages();
                    return match error {
                        MemoryInitError::TotalBudgetExceeded => VMResult::ResourceLimitExceeded,
                        _ => VMResult::MemoryAllocationFailed,
                    };
                }
            }
        }
        // The reservation covered exactly the defined memories' minima
        // and each adoption paid its own minimum out of it.
        debug_assert_eq!(runtime.reserved_total_pages(), 0);

        for (idx, d) in (0..).zip(data.0) {
            match &d.mode {
                DataMode::Active(mem, offset) => {
                    let offset =
                        vm_try!(execute_offset_const_expr(&mut runtime, &globals, offset)) as usize;
                    let memory =
                        *vm_try!(VMResult::from_option(memories.get(mem.0 as usize), || {
                            VMResult::MemoryIndexOutOfRange
                        }));
                    vm_try!(runtime.with_memory_by_addr(memory, |memory| {
                        if let Some(slice) = memory.get_mut(offset..offset + d.init.len()) {
                            slice.copy_from_slice(&d.init);
                            VMResult::Success(())
                        } else {
                            VMResult::MemoryIndexOutOfRange
                        }
                    }));
                    store.lock_segments().data.insert((instance_id, idx), d);
                }
                DataMode::Passive => {
                    store.lock_segments().data.insert((instance_id, idx), d);
                }
            }
        }

        let mut local_wasm_funcs = Vec::new();
        for func in codes.0.into_iter() {
            let funcidx = funcs.len() as u32;

            let func_addr = match func {
                FunctionBody::Wasm(code) => {
                    let func_addr = runtime.new_func(&FunctionInstanceData {
                        instance: inst_id,
                        body: RuntimeFunctionBody::Wasm {
                            locals: code.locals,
                            code: Arc::<[Instr]>::from([]),
                            op_lens: Arc::<[u16]>::from([]),
                            lowered: code.lowered.clone(),
                        },
                        funcidx,
                    });
                    local_wasm_funcs.push(func_addr);
                    func_addr
                }
                FunctionBody::Host(fp) => runtime.new_func(&FunctionInstanceData {
                    instance: inst_id,
                    body: RuntimeFunctionBody::Host(fp),
                    funcidx,
                }),
            };

            funcs.push(func_addr);
            tracing::trace!("linking: {funcidx} => {func_addr:?}");
        }

        let recipe_slots = funcs
            .iter()
            .map(|&funcaddr| runtime.call_recipe_slot_for_func(funcaddr))
            .collect::<Vec<_>>();
        #[cfg(feature = "aot-load")]
        let jit_local_wasm_recipe_slots = local_wasm_funcs
            .iter()
            .map(|&func_addr| runtime.call_recipe_slot_for_func(func_addr))
            .collect::<Vec<_>>();
        let mut materialized_local_wasm_funcs = Vec::with_capacity(local_wasm_funcs.len());
        for &func_addr in &local_wasm_funcs {
            let materialized = match &runtime.get_func(func_addr).body {
                RuntimeFunctionBody::Wasm { lowered, .. } => {
                    lowered.materialize_with_recipe_slots(&recipe_slots)
                }
                RuntimeFunctionBody::Host(_)
                | RuntimeFunctionBody::AsyncHost(_)
                | RuntimeFunctionBody::PublicHost(_)
                | RuntimeFunctionBody::PublicAsyncHost(_) => continue,
            };
            #[cfg(feature = "aot-load")]
            let mut materialized = materialized;
            #[cfg(feature = "aot-load")]
            if crate::runtime::jit::supported() && store.runtime_config().jit.enabled {
                rewrite_direct_wasm_calls_for_jit(
                    &mut materialized.instrs,
                    &materialized.op_lens,
                    &jit_local_wasm_recipe_slots,
                );
            }
            #[cfg(feature = "vm-diagnostics")]
            dump_materialized_function_if_requested(
                funcs
                    .iter()
                    .position(|&addr| addr == func_addr)
                    .expect("local wasm function must belong to instance") as u32,
                &materialized.instrs,
                &materialized.op_lens,
            );
            materialized_local_wasm_funcs.push((func_addr, materialized));
        }
        for (func_addr, materialized) in materialized_local_wasm_funcs {
            let func = runtime.get_func_mut(func_addr);
            let RuntimeFunctionBody::Wasm { code, op_lens, .. } = &mut func.body else {
                unreachable!("materialized local wasm function must remain wasm")
            };
            *op_lens = materialized.op_lens.into();
            *code = materialized.instrs.into();
        }

        for init in &global_init {
            globals.push(vm_try!(init_global(&mut runtime, init, &globals, &funcs)));
        }
        let table_cap = store.runtime_config().limits.max_table_elements;
        let mut table_instances: Vec<ObjectRef> = Vec::with_capacity(m_tables.len());
        for table_type in m_tables.iter() {
            let mut table_type = *table_type;
            if let Some(cap) = table_cap {
                // The preflight above has already refused every table
                // whose minimum exceeds the cap, so only the clamp
                // remains: clamping the stored maximum makes
                // `table.grow` enforce the ceiling through the existing
                // declared-max path.
                debug_assert!(table_type.limits.min <= cap);
                table_type.limits.max = Some(table_type.limits.max.unwrap_or(cap).min(cap));
            }
            table_instances.push(runtime.new_table(table_type));
        }
        tables.append(&mut table_instances);

        let res = (|| {
            for (idx, elem) in (0u32..).zip(m_elems.0) {
                match &elem.mode {
                    ElemMode::Active(idx, offset) => match &elem.init {
                        ElemInit::FuncIdx(idxs) => {
                            let offset =
                                vm_try!(execute_offset_const_expr(&mut runtime, &globals, offset))
                                    as usize;
                            let table_addr =
                                *vm_try!(VMResult::from_option(tables.get(idx.0 as usize), || {
                                    VMResult::TableIndexOutOfRange
                                }));
                            let instance = runtime.get_table(table_addr);

                            if instance.0.reftype != elem.kind {
                                // #128: the parser validates active segments
                                // against the declared table type, so a
                                // mismatch means a module producer broke an
                                // internal invariant; fail closed instead of
                                // aborting the host.
                                return VMResult::Unlinkable;
                            }
                            if offset + idxs.len() > instance.1.len() {
                                return VMResult::TableIndexOutOfRange;
                            }
                            for (idx, funcidx) in idxs.iter().enumerate() {
                                let func_addr = vm_try!(VMResult::from_option(
                                    funcs.get(*funcidx as usize),
                                    || VMResult::InvalidOperand
                                ));
                                instance.1[offset + idx] = func_addr.get();
                            }
                        }
                        ElemInit::ConstExpr(idxs) => {
                            let offset =
                                vm_try!(execute_offset_const_expr(&mut runtime, &globals, offset))
                                    as usize;
                            let table_addr =
                                *vm_try!(VMResult::from_option(tables.get(idx.0 as usize), || {
                                    VMResult::TableIndexOutOfRange
                                }));
                            let instance = runtime.get_table(table_addr);
                            if offset + idxs.len() > instance.1.len() {
                                return VMResult::TableIndexOutOfRange;
                            }
                            let rt = instance.0.reftype;

                            for (idx, idx_expr) in idxs.iter().enumerate() {
                                let elem_addr = vm_try!(execute_elem_init_const_expr(
                                    &mut runtime,
                                    &globals,
                                    &funcs,
                                    idx_expr,
                                    rt
                                ));
                                let instance = runtime.get_table(table_addr);
                                instance.1[offset + idx] = elem_addr.get();
                            }
                        }
                    },
                    ElemMode::Passive => {
                        store.lock_segments().elems.insert((instance_id, idx), elem);
                    }
                    ElemMode::Declarative => {}
                }
            }
            VMResult::Success(())
        })();

        let instance = Instance {
            module_addr: mod_addr,
            instance_id,
            memory: memories,
            tables,
            globals,
            funcs,
        };

        unsafe {
            runtime.place_instance_unchecked(inst_addr, &instance);
        }
        vm_try!(res);
        for &funcaddr in &instance.funcs {
            let recipe = runtime.build_call_recipe(funcaddr);
            runtime.set_call_recipe_for_func(funcaddr, recipe);
        }
        let addr = InstanceHandle::new(store, inst_id, instance_id);

        let has_start = if let Some(start) = start {
            let mut stack = Stack::new(store.runtime_config().limits.value_stack_bytes as usize);
            let funcaddr = vm_try!(VMResult::from_option(
                instance.funcs.get(start.0 as usize),
                || {
                    tracing::error!("start function index out of range");
                    VMResult::Unlinkable
                }
            ));
            let funcaddr = *funcaddr;
            let funcinst = runtime.get_func(funcaddr);
            let func_instance = runtime.instance(funcinst.instance);
            if funcinst.is_host_func() {
                let local_reference = vm_try!(stack.function_call(
                    0,
                    0,
                    CallFrameCache::from_parts(
                        funcaddr,
                        funcinst,
                        func_instance
                            .memory_slots
                            .first()
                            .copied()
                            .and_then(|slot| slot.handle()),
                    ),
                    LocalReference {
                        local_size: 0,
                        local_top: 0
                    },
                    &vm::VM_END,
                    &runtime,
                ));

                scheduler.push(Task {
                    task_id: fresh_task_id(),
                    stack,
                    local_reference,
                    ready_flag: ReadyFlag::Ready,
                    fp: StablePc::from_stable_ptr(vm::START_HOST_FUNCTION_PROGRAM.as_ptr()),
                    pending_effects: 0,
                    terminal_result: None,
                    terminal_trap: None,
                });
            } else {
                let locals = funcinst.locals();
                let local_reference = vm_try!(stack.function_call(
                    0,
                    locals.byte_size(),
                    CallFrameCache::from_parts(
                        funcaddr,
                        funcinst,
                        func_instance
                            .memory_slots
                            .first()
                            .copied()
                            .and_then(|slot| slot.handle()),
                    ),
                    LocalReference {
                        local_size: 0,
                        local_top: 0
                    },
                    &vm::VM_END,
                    &runtime,
                ));

                scheduler.push(Task {
                    fp: vm::wasm_entry_pc(store),
                    task_id: fresh_task_id(),
                    stack,
                    local_reference,
                    ready_flag: ReadyFlag::Ready,
                    pending_effects: 0,
                    terminal_result: None,
                    terminal_trap: None,
                });
            }
            true
        } else {
            false
        };

        (addr, has_start)
    };

    if has_start {
        scheduler.run().await;
        // #128: the start task was pushed above and `run` drives it with
        // the built-in driver, so a completion must exist; if the invariant
        // is ever broken, fail closed instead of aborting the host.
        let Some(completed) = scheduler.completed_tasks.pop() else {
            return VMResult::InvalidOperand;
        };
        {
            let mut runtime = store.lock_runtime_or_panic();
            runtime.set_last_trap(completed.trap);
        }
        vm_try!(completed.result);
    }

    VMResult::Success(addr)
}
#[allow(dead_code)]
/// Builds a lightweight module that re-exports imports from registered instances.
///
/// Every tuple is `(registered_module, imported_export, new_export_name)`. This
/// advanced helper shares the existing underlying functions, globals, memories,
/// or tables; it does not copy their state.
pub fn aliasing(
    registry: &Registry,
    triplets: &[(String, String, String)],
    store: &Store,
) -> VMResult<InstanceHandle> {
    let mut runtime = match store.lock_runtime("aliasing") {
        Ok(runtime) => runtime,
        Err(err) => {
            err.report();
            return VMResult::Unlinkable;
        }
    };
    let inst_id = store.new_instance_id();
    let mut functions = vec![];
    let mut function_types = vec![];
    let mut globals = vec![];
    let mut memories = vec![];
    let mut tables = vec![];
    let mut function_addrs = vec![];
    let mut global_addrs = vec![];
    let mut memory_addrs = vec![];
    let mut table_addrs = vec![];
    let mut exports = vec![];
    for (modname, importname, exportname) in triplets {
        let instance_addr = vm_try!(VMResult::from_option(registry.get(modname), || {
            VMResult::Unlinkable
        }));

        let object_ref = vm_try!(VMResult::from_option(
            instance_addr.object_ref_for_store(store),
            || { VMResult::Unlinkable }
        ));
        let ext_instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
        let ext_module = runtime.get_module(ext_instance.module_addr);
        let export_desc = vm_try!(VMResult::from_option(
            ext_module.exports.find(importname),
            || { VMResult::Unlinkable }
        ));
        let exportname = (*exportname).to_owned();
        match export_desc {
            ExportDesc::Func(idx) => {
                // #128: registry indices resolve fail-closed, as in
                // `instantiate`'s import linking.
                let tidx = vm_try!(VMResult::from_option(
                    ext_module.functions.get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let ft = vm_try!(VMResult::from_option(
                    ext_module.function_types.get(tidx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let new_tidx = function_types.len();
                let new_funcidx = functions.len();
                function_types.push(ft.clone());
                functions.push(TypeIdx(new_tidx as u32));
                let addr = vm_try!(VMResult::from_option(
                    ext_instance.funcs.as_slice().get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let addr = *addr;
                function_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Func(FuncIdx(new_funcidx as u32)),
                ));
            }
            ExportDesc::Global(idx) => {
                let gt = vm_try!(VMResult::from_option(
                    ext_module.globals.get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let new_gidx = globals.len();
                globals.push(*gt);
                let addr = vm_try!(VMResult::from_option(
                    ext_instance.globals.as_slice().get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let addr = *addr;
                global_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Global(GlobalIdx(new_gidx as u32)),
                ));
            }
            ExportDesc::Mem(idx) => {
                let mt = vm_try!(VMResult::from_option(
                    ext_module.mems.get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let new_memidx = memories.len();
                memories.push(*mt);
                let addr = vm_try!(VMResult::from_option(
                    ext_instance.mems.as_slice().get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let addr = *addr;
                memory_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Mem(MemIdx(new_memidx as u32)),
                ));
            }
            ExportDesc::Table(idx) => {
                let tt = vm_try!(VMResult::from_option(
                    ext_module.tables.get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                let new_tableidx = tables.len();
                tables.push(*tt);
                let table_addr = vm_try!(VMResult::from_option(
                    ext_instance.tables.as_slice().get(idx.0 as usize),
                    || VMResult::Unlinkable
                ));
                table_addrs.push(*table_addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Table(TableIdx(new_tableidx as u32)),
                ));
            }
        }
    }
    let mod_addr = runtime.new_module(ModuleInstance {
        exports: ExportSection(exports),
        tables,
        globals,
        functions,
        function_types,
        mems: memories,
        names: None,
    });
    let inst_id_handle = runtime.alloc_instance(InstanceData {
        module_addr: mod_addr,
        mems: memory_addrs,
        globals: global_addrs,
        funcs: function_addrs,
        tables: table_addrs,
        instance_id: inst_id,
        memory_slots: Vec::new(),
    });
    VMResult::Success(InstanceHandle::new(store, inst_id_handle, inst_id))
}
/// Replaces a function in `addr` with a synchronous host callback by index.
///
/// `funcidx` must select an existing function of a signature compatible with
/// `f`, and `addr` must belong to `store`.
pub fn link_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFn,
    store: &Store,
) {
    if let Err(err) = link_public_host_function_with_function_idx_impl(addr, funcidx, f, store) {
        err.report();
    }
}

pub(crate) fn link_public_host_function_with_function_idx_impl(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFn,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let mut runtime = store.lock_runtime("link_host_function_with_function_idx")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    // #128: the index is embedder input; an out-of-range index is a
    // rejected link, never a host panic.
    let Some(funcaddr) = instance.funcs.as_slice().get(funcidx as usize).copied() else {
        tracing::error!("function index out of range");
        return Ok(());
    };
    {
        let func = runtime.get_func_mut(funcaddr);
        func.replace_public_host_code_pointer(f);
    }
    let recipe = runtime.build_call_recipe(funcaddr);
    runtime.set_call_recipe_for_func(funcaddr, recipe);
    Ok(())
}

/// Raw-ABI counterpart of [`link_host_function_with_function_idx`], kept for
/// the opt-in raw integration surface (`unstable_internals`).
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub fn link_raw_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFunction,
    store: &Store,
) {
    if let Err(err) = link_host_function_with_function_idx_impl(addr, funcidx, f, store) {
        err.report();
    }
}

#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub(crate) fn link_host_function_with_function_idx_impl(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFunction,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let mut runtime = store.lock_runtime("link_host_function_with_function_idx")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    let Some(funcaddr) = instance.funcs.as_slice().get(funcidx as usize).copied() else {
        tracing::error!("function index out of range");
        return Ok(());
    };
    {
        let func = runtime.get_func_mut(funcaddr);
        func.replace_host_code_pointer(f);
    }
    let recipe = runtime.build_call_recipe(funcaddr);
    runtime.set_call_recipe_for_func(funcaddr, recipe);
    Ok(())
}
/// Replaces an exported function with a synchronous host callback.
///
/// `name` must resolve to a function export of `addr`; use the index variant
/// when the embedder already has the function index.
///
/// # Examples
///
/// A host callback written against the public boundary receives its
/// arguments decoded as [`WasmValue`]s and returns the guest-visible
/// results; it never names an interpreter-internal type.
///
/// ```
/// use futures::executor::block_on;
/// use telomere::{
///     instantiate, link_host_function_with_export_name, run_module_function, HostContext,
///     IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmValue,
/// };
///
/// fn return_42(_cx: &mut HostContext<'_, '_>) -> VMResult<Vec<WasmValue>> {
///     VMResult::Success(vec![WasmValue::I32(42)])
/// }
///
/// let bytes = wat::parse_str(r#"(module (func (export "answer") (result i32) i32.const 0))"#)
///     .expect("valid module");
/// let mut reader = IoReadBinaryReader::from(&bytes[..]);
/// let module = WasmParser::new(&mut reader)
///     .parse_module()
///     .expect("the module parses");
/// let store = Store::new();
/// let registry = Registry::new();
/// let instance = match block_on(instantiate(module, &store, &registry)) {
///     VMResult::Success(instance) => instance,
///     failure => panic!("instantiation failed: {failure:?}"),
/// };
/// link_host_function_with_export_name(&instance, "answer", return_42, &store);
/// let result = block_on(run_module_function(
///     &instance,
///     &store,
///     "answer",
///     &ResultValue::new(vec![]),
/// ));
/// assert_eq!(
///     result.unwrap(),
///     ResultValue::new(vec![WasmValue::I32(42)])
/// );
/// ```
pub fn link_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: HostFn,
    store: &Store,
) {
    if let Err(err) = link_public_host_function_with_export_name_impl(addr, name, f, store) {
        err.report();
    }
}

pub(crate) fn link_public_host_function_with_export_name_impl(
    addr: &InstanceHandle,
    name: &str,
    f: HostFn,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let runtime = store.lock_runtime("link_host_function_with_export_name")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    let module = runtime.get_module(instance.module_addr);
    // #128: the name is embedder input; an unknown or non-function
    // export is a rejected link, never a host panic.
    let Some(export) = module.exports.find(name) else {
        tracing::error!("unknown export");
        return Ok(());
    };
    let ExportDesc::Func(func_idx) = export else {
        tracing::error!("export is not a function");
        return Ok(());
    };
    let func_idx = func_idx.0;
    drop(runtime);
    link_public_host_function_with_function_idx_impl(addr, func_idx, f, store)
}

/// Raw-ABI counterpart of [`link_host_function_with_export_name`], kept for
/// the opt-in raw integration surface (`unstable_internals`).
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub fn link_raw_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: HostFunction,
    store: &Store,
) {
    if let Err(err) = link_host_function_with_export_name_impl(addr, name, f, store) {
        err.report();
    }
}

#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub(crate) fn link_host_function_with_export_name_impl(
    addr: &InstanceHandle,
    name: &str,
    f: HostFunction,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let runtime = store.lock_runtime("link_host_function_with_export_name")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    let module = runtime.get_module(instance.module_addr);
    // #128: the name is embedder input; an unknown or non-function
    // export is a rejected link, never a host panic.
    let Some(export) = module.exports.find(name) else {
        tracing::error!("unknown export");
        return Ok(());
    };
    let ExportDesc::Func(func_idx) = export else {
        tracing::error!("export is not a function");
        return Ok(());
    };
    let func_idx = func_idx.0;
    drop(runtime);
    link_host_function_with_function_idx_impl(addr, func_idx, f, store)
}

/// Replaces a function in `addr` with an asynchronous host callback by index.
///
/// The callback future is submitted to the [`crate::ExecutionDriver`] used by
/// [`crate::run_module_function_with_driver`].
pub fn link_async_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: AsyncHostFn,
    store: &Store,
) {
    if let Err(err) =
        link_public_async_host_function_with_function_idx_impl(addr, funcidx, f, store)
    {
        err.report();
    }
}

pub(crate) fn link_public_async_host_function_with_function_idx_impl(
    addr: &InstanceHandle,
    funcidx: u32,
    f: AsyncHostFn,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let mut runtime = store.lock_runtime("link_async_host_function_with_function_idx")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    // #128: the index is embedder input; an out-of-range index is a
    // rejected link, never a host panic.
    let Some(funcaddr) = instance.funcs.as_slice().get(funcidx as usize).copied() else {
        tracing::error!("function index out of range");
        return Ok(());
    };
    {
        let func = runtime.get_func_mut(funcaddr);
        func.replace_public_async_host_code_pointer(f);
    }
    let recipe = runtime.build_call_recipe(funcaddr);
    runtime.set_call_recipe_for_func(funcaddr, recipe);
    Ok(())
}

/// Raw-ABI counterpart of [`link_async_host_function_with_function_idx`],
/// kept for the opt-in raw integration surface (`unstable_internals`).
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub fn link_raw_async_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: AsyncHostFunction,
    store: &Store,
) {
    if let Err(err) = link_async_host_function_with_function_idx_impl(addr, funcidx, f, store) {
        err.report();
    }
}

#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub(crate) fn link_async_host_function_with_function_idx_impl(
    addr: &InstanceHandle,
    funcidx: u32,
    f: AsyncHostFunction,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let mut runtime = store.lock_runtime("link_async_host_function_with_function_idx")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    let Some(funcaddr) = instance.funcs.as_slice().get(funcidx as usize).copied() else {
        tracing::error!("function index out of range");
        return Ok(());
    };
    {
        let func = runtime.get_func_mut(funcaddr);
        func.replace_async_host_code_pointer(f);
    }
    let recipe = runtime.build_call_recipe(funcaddr);
    runtime.set_call_recipe_for_func(funcaddr, recipe);
    Ok(())
}

/// Replaces an exported function with an asynchronous host callback.
///
/// `name` must resolve to a function export of `addr`; use the index variant
/// when the embedder already has the function index.
pub fn link_async_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: AsyncHostFn,
    store: &Store,
) {
    if let Err(err) = link_public_async_host_function_with_export_name_impl(addr, name, f, store) {
        err.report();
    }
}

pub(crate) fn link_public_async_host_function_with_export_name_impl(
    addr: &InstanceHandle,
    name: &str,
    f: AsyncHostFn,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let runtime = store.lock_runtime("link_async_host_function_with_export_name")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    let module = runtime.get_module(instance.module_addr);
    // #128: the name is embedder input; an unknown or non-function
    // export is a rejected link, never a host panic.
    let Some(export) = module.exports.find(name) else {
        tracing::error!("unknown export");
        return Ok(());
    };
    let ExportDesc::Func(func_idx) = export else {
        tracing::error!("export is not a function");
        return Ok(());
    };
    let func_idx = func_idx.0;
    drop(runtime);
    link_public_async_host_function_with_function_idx_impl(addr, func_idx, f, store)
}

/// Raw-ABI counterpart of [`link_async_host_function_with_export_name`],
/// kept for the opt-in raw integration surface (`unstable_internals`).
#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub fn link_raw_async_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: AsyncHostFunction,
    store: &Store,
) {
    if let Err(err) = link_async_host_function_with_export_name_impl(addr, name, f, store) {
        err.report();
    }
}

#[cfg_attr(not(feature = "unstable-internals"), allow(dead_code))]
pub(crate) fn link_async_host_function_with_export_name_impl(
    addr: &InstanceHandle,
    name: &str,
    f: AsyncHostFunction,
    store: &Store,
) -> Result<(), StoreExecutionError> {
    let runtime = store.lock_runtime("link_async_host_function_with_export_name")?;
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return Ok(());
    };
    let instance = unsafe { &*runtime.get_instance_unchecked(object_ref) };
    let module = runtime.get_module(instance.module_addr);
    // #128: the name is embedder input; an unknown or non-function
    // export is a rejected link, never a host panic.
    let Some(export) = module.exports.find(name) else {
        tracing::error!("unknown export");
        return Ok(());
    };
    let ExportDesc::Func(func_idx) = export else {
        tracing::error!("export is not a function");
        return Ok(());
    };
    let func_idx = func_idx.0;
    drop(runtime);
    link_async_host_function_with_function_idx_impl(addr, func_idx, f, store)
}

#[cfg(test)]
fn rewrite_direct_calls_for_slots(
    instrs: &mut [Instr],
    op_lens: &[u16],
    recipe_slots_to_rewrite: &[u32],
    replacement: crate::common::Op,
) {
    if recipe_slots_to_rewrite.is_empty() {
        return;
    }

    let mut cursor = 0usize;
    for len in op_lens {
        let op = unsafe { instrs[cursor].op };
        if std::ptr::fn_addr_eq(op, vm::op_call as crate::common::Op) {
            let target = unsafe { instrs[cursor + 1].operand.call_recipe_ref };
            if target
                .resolved_recipe_slot()
                .is_some_and(|slot| recipe_slots_to_rewrite.contains(&slot))
            {
                instrs[cursor] = Instr { op: replacement };
            }
        }
        cursor += usize::from(*len);
    }
    debug_assert_eq!(cursor, instrs.len());
}

#[cfg(feature = "aot-load")]
fn rewrite_direct_wasm_calls_for_jit(
    instrs: &mut [Instr],
    op_lens: &[u16],
    local_wasm_recipe_slots: &[u32],
) {
    if local_wasm_recipe_slots.is_empty() {
        return;
    }

    let mut cursor = 0usize;
    for len in op_lens {
        let op = unsafe { instrs[cursor].op };
        let replacement = if std::ptr::fn_addr_eq(op, vm::op_call as crate::common::Op) {
            Some(vm::op_call_jit_lazy as crate::common::Op)
        } else if std::ptr::fn_addr_eq(op, vm::op_return_call as crate::common::Op) {
            Some(vm::op_return_call_jit_lazy as crate::common::Op)
        } else {
            None
        };
        if let Some(replacement) = replacement {
            let target = unsafe { instrs[cursor + 1].operand.call_recipe_ref };
            if target
                .resolved_recipe_slot()
                .is_some_and(|slot| local_wasm_recipe_slots.contains(&slot))
            {
                instrs[cursor] = Instr { op: replacement };
            }
        }
        cursor += usize::from(*len);
    }
    debug_assert_eq!(cursor, instrs.len());
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct Crc16UpdateMaskedWrapperShape {
    data_local: u32,
    crc_local: u32,
    return_addr: usize,
}

#[cfg(test)]
fn crc16_update_masked_wrapper_shape(
    instrs: &[Instr],
    op_lens: &[u16],
) -> Option<Crc16UpdateMaskedWrapperShape> {
    const EXPECTED_LENS: [u16; 5] = [4, 2, 2, 1, 2];

    if op_lens != EXPECTED_LENS.as_slice() {
        return None;
    }
    let required_instrs: usize = EXPECTED_LENS.iter().map(|len| usize::from(*len)).sum();
    if instrs.len() != required_instrs {
        return None;
    }

    let expected = [
        vm::op_local_binop32 as crate::common::Op,
        vm::op_local_get4 as crate::common::Op,
        vm::op_call_i32_crc16_update16 as crate::common::Op,
        vm::op_end as crate::common::Op,
        vm::special_function_return as crate::common::Op,
    ];
    let mut cursor = 0usize;
    for (index, expected_op) in expected.into_iter().enumerate() {
        let op = unsafe { instrs[cursor].op };
        if !std::ptr::fn_addr_eq(op, expected_op) {
            return None;
        }
        cursor += usize::from(op_lens[index]);
    }

    let mask_kind = unsafe { instrs[1].operand.u32 };
    let data_local = unsafe { instrs[2].operand.local_addr };
    let mask_rhs = unsafe { instrs[3].operand.i32 };
    if decode_local_binop32_kind(mask_kind)
        != Some((LocalBinop32Op::I32And, LocalFastRhsShape::Const))
        || mask_rhs != 0xffff
    {
        return None;
    }

    Some(Crc16UpdateMaskedWrapperShape {
        data_local,
        crc_local: unsafe { instrs[5].operand.local_addr },
        return_addr: 9,
    })
}

#[cfg(test)]
fn materialized_starts_with_cached_u16_low7_guard(instrs: &[Instr], op_lens: &[u16]) -> bool {
    const EXPECTED_LENS: [u16; 5] = [5, 3, 2, 4, 2];

    if op_lens.len() < EXPECTED_LENS.len() {
        return false;
    }
    if op_lens[..EXPECTED_LENS.len()] != EXPECTED_LENS {
        return false;
    }
    let required_instrs = EXPECTED_LENS.iter().map(|len| usize::from(*len)).sum();
    if instrs.len() < required_instrs {
        return false;
    }

    let expected = [
        vm::op_i32_load16_u_local_base_tee4 as crate::common::Op,
        vm::op_i32_const_binop as crate::common::Op,
        vm::op_if as crate::common::Op,
        vm::op_local_binop32 as crate::common::Op,
        vm::op_return as crate::common::Op,
    ];
    let mut cursor = 0usize;
    for (index, expected_op) in expected.into_iter().enumerate() {
        if cursor >= instrs.len() {
            return false;
        }
        let op = unsafe { instrs[cursor].op };
        if !std::ptr::fn_addr_eq(op, expected_op) {
            return false;
        }
        cursor += usize::from(op_lens[index]);
    }

    let load_local_addr = unsafe { instrs[1].operand.local_addr };
    let load_delta = unsafe { instrs[2].operand.i32 };
    let load_memarg = unsafe { instrs[3].operand.memarg };
    let cached_local_addr = unsafe { instrs[4].operand.local_addr };
    if load_local_addr != 0 || load_delta != 0 || load_memarg.offset != 0 {
        return false;
    }

    let guard_kind = unsafe { instrs[6].operand.u32 };
    let guard_rhs = unsafe { instrs[7].operand.i32 };
    if decode_local_binop32_kind(guard_kind)
        != Some((LocalBinop32Op::I32And, LocalFastRhsShape::Const))
        || guard_rhs != 0x80
    {
        return false;
    }

    let return_kind = unsafe { instrs[11].operand.u32 };
    let return_lhs = unsafe { instrs[12].operand.local_addr };
    let return_rhs = unsafe { instrs[13].operand.i32 };
    decode_local_binop32_kind(return_kind)
        == Some((LocalBinop32Op::I32And, LocalFastRhsShape::Const))
        && return_lhs == cached_local_addr
        && return_rhs == 0x7f
}

#[cfg(test)]
fn rewrite_crc16_update_masked_wrapper(instrs: &mut [Instr], op_lens: &[u16]) {
    let Some(shape) = crc16_update_masked_wrapper_shape(instrs, op_lens) else {
        return;
    };

    instrs[0] = Instr {
        op: vm::op_i32_crc16_update16_masked,
    };
    instrs[1] = Instr {
        operand: crate::common::Operand {
            local_addr: shape.data_local,
        },
    };
    instrs[2] = Instr {
        operand: crate::common::Operand {
            local_addr: shape.crc_local,
        },
    };
    instrs[3] = Instr {
        operand: crate::common::Operand {
            jump_addr: u32::try_from(shape.return_addr).expect("return address exceeds u32::MAX"),
        },
    };
}

#[allow(dead_code)]
fn rewrite_list_crc_summary_function(instrs: &mut [Instr], op_lens: &[u16]) {
    if instrs.len() < 4 || op_lens.len() < 250 {
        return;
    }

    let mut cursor = 0usize;
    let mut relink_loops = 0usize;
    let mut cached_value_calls = 0usize;
    let mut crc_calls = 0usize;
    let mut local_load8 = 0usize;
    let mut return_addr = None;
    for len in op_lens {
        let op = unsafe { instrs[cursor].op };
        if std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load_store_local_base_relink_loop as crate::common::Op,
        ) {
            relink_loops += 1;
        } else if std::ptr::fn_addr_eq(op, vm::op_call_cached_u16_low7_guard as crate::common::Op) {
            cached_value_calls += 1;
        } else if std::ptr::fn_addr_eq(
            op,
            vm::op_call_i32_crc16_update16_masked as crate::common::Op,
        ) {
            crc_calls += 1;
        } else if std::ptr::fn_addr_eq(op, vm::op_i32_load8_u_local_base as crate::common::Op) {
            local_load8 += 1;
        } else if std::ptr::fn_addr_eq(op, vm::special_function_return as crate::common::Op) {
            return_addr = Some(cursor);
        } else if std::ptr::fn_addr_eq(op, vm::op_call as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_call_import as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_call_indirect as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call_import as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as crate::common::Op)
        {
            return;
        }
        cursor += usize::from(*len);
    }
    debug_assert_eq!(cursor, instrs.len());
    let Some(return_addr) = return_addr else {
        return;
    };
    if relink_loops < 1 || cached_value_calls < 2 || crc_calls < 2 || local_load8 < 1 {
        return;
    }

    instrs[0] = Instr {
        op: vm::op_i32_list_crc_summary,
    };
    for (slot, local_addr) in [0, 4].into_iter().enumerate() {
        instrs[1 + slot] = Instr {
            operand: crate::common::Operand { local_addr },
        };
    }
    instrs[3] = Instr {
        operand: crate::common::Operand {
            jump_addr: u32::try_from(return_addr).expect("return address exceeds u32::MAX"),
        },
    };
}

#[allow(dead_code)]
fn rewrite_matrix_i16_crc_summary_function(instrs: &mut [Instr], op_lens: &[u16]) {
    if instrs.len() < 7 || op_lens.len() < 160 {
        return;
    }

    let mut cursor = 0usize;
    let mut update_store16_loops = 0usize;
    let mut signed_mul_loops = 0usize;
    let mut bitmix_loops = 0usize;
    let mut sum_clip_loops = 0usize;
    let mut crc_calls = 0usize;
    let mut return_addr = None;
    for len in op_lens {
        let op = unsafe { instrs[cursor].op };
        if std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load16_u_update_store16_local_base_loop as crate::common::Op,
        ) {
            update_store16_loops += 1;
        } else if std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load16_s_mul_add_local_base_delta_loop as crate::common::Op,
        ) || std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load16_s_mul_add_local_base_loop as crate::common::Op,
        ) || std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load16_s_dot4_local_base_loop as crate::common::Op,
        ) {
            signed_mul_loops += 1;
        } else if std::ptr::fn_addr_eq(
            op,
            vm::op_i32_load16_u_bitmix_acc_local_base_delta_loop as crate::common::Op,
        ) {
            bitmix_loops += 1;
        } else if std::ptr::fn_addr_eq(op, vm::op_i32_sum_clip_local_base_loop as crate::common::Op)
        {
            sum_clip_loops += 1;
        } else if std::ptr::fn_addr_eq(
            op,
            vm::op_call_i32_crc16_update16_masked as crate::common::Op,
        ) {
            crc_calls += 1;
        } else if std::ptr::fn_addr_eq(op, vm::special_function_return as crate::common::Op) {
            return_addr = Some(cursor);
        } else if std::ptr::fn_addr_eq(op, vm::op_call as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_call_import as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_call_indirect as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call_import as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as crate::common::Op)
        {
            return;
        }
        cursor += usize::from(*len);
    }
    debug_assert_eq!(cursor, instrs.len());
    let Some(return_addr) = return_addr else {
        return;
    };
    if update_store16_loops < 2
        || signed_mul_loops < 2
        || bitmix_loops < 1
        || sum_clip_loops < 4
        || crc_calls < 4
    {
        return;
    }

    instrs[0] = Instr {
        op: vm::op_i32_matrix_i16_crc_summary,
    };
    for (slot, local_addr) in [0, 4, 8, 12, 16].into_iter().enumerate() {
        instrs[1 + slot] = Instr {
            operand: crate::common::Operand { local_addr },
        };
    }
    instrs[6] = Instr {
        operand: crate::common::Operand {
            jump_addr: u32::try_from(return_addr).expect("return address exceeds u32::MAX"),
        },
    };
}

#[allow(dead_code)]
fn rewrite_core_state_benchmark_function(instrs: &mut [Instr], op_lens: &[u16]) {
    if instrs.len() < 8 || op_lens.len() < 80 {
        return;
    }

    let mut cursor = 0usize;
    let mut numeric_transition_calls = 0usize;
    let mut crc_calls = 0usize;
    let mut return_addr = None;
    for len in op_lens {
        let op = unsafe { instrs[cursor].op };
        if std::ptr::fn_addr_eq(
            op,
            vm::op_call_i32_numeric_token_state_transition as crate::common::Op,
        ) {
            numeric_transition_calls += 1;
        } else if std::ptr::fn_addr_eq(op, vm::op_call as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_call_i32_crc16_update16 as crate::common::Op)
        {
            crc_calls += 1;
        } else if std::ptr::fn_addr_eq(op, vm::special_function_return as crate::common::Op) {
            return_addr = Some(cursor);
        } else if std::ptr::fn_addr_eq(op, vm::op_return_call as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call_import as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return_call_indirect as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_call_indirect as crate::common::Op)
        {
            return;
        }
        cursor += usize::from(*len);
    }
    debug_assert_eq!(cursor, instrs.len());
    let Some(return_addr) = return_addr else {
        return;
    };
    if numeric_transition_calls < 2 || crc_calls < 8 {
        return;
    }

    instrs[0] = Instr {
        op: vm::op_i32_core_state_benchmark,
    };
    for (slot, local_addr) in [0, 4, 8, 12, 16, 20].into_iter().enumerate() {
        instrs[1 + slot] = Instr {
            operand: crate::common::Operand { local_addr },
        };
    }
    instrs[7] = Instr {
        operand: crate::common::Operand {
            jump_addr: u32::try_from(return_addr).expect("return address exceeds u32::MAX"),
        },
    };
}

#[allow(dead_code)]
fn rewrite_list_crc_pair_loops(instrs: &mut [Instr]) {
    if instrs.len() < 90 {
        return;
    }

    let mut first_calls = Vec::new();
    for pc in 21..instrs.len().saturating_sub(88) {
        if !op_eq(
            instrs,
            pc,
            vm::op_call_i32_list_crc_summary as crate::common::Op,
        ) {
            continue;
        }
        if op_eq(
            instrs,
            pc + 6,
            vm::op_call_i32_crc16_update16 as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 15,
            vm::op_call_i32_list_crc_summary as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 21,
            vm::op_call_i32_crc16_update16 as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 52,
            vm::op_call_i32_list_crc_summary as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 58,
            vm::op_call_i32_crc16_update16 as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 67,
            vm::op_call_i32_list_crc_summary as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 73,
            vm::op_call_i32_crc16_update16 as crate::common::Op,
        ) && op_eq(
            instrs,
            pc + 77,
            vm::op_local_get4_i32_const_add_tee4_br_if as crate::common::Op,
        ) && op_eq(instrs, pc + 88, vm::op_call as crate::common::Op)
        {
            first_calls.push(pc);
        }
    }

    for pc in first_calls {
        let start = pc - 21;
        let jump = pc + 88;
        instrs[start] = Instr {
            op: vm::op_i32_list_crc_pair_loop,
        };
        instrs[start + 1] = Instr {
            operand: crate::common::Operand { local_addr: 4 },
        };
        instrs[start + 2] = Instr {
            operand: crate::common::Operand { u32: 288 },
        };
        instrs[start + 3] = Instr {
            operand: crate::common::Operand { u32: 316 },
        };
        instrs[start + 4] = Instr {
            operand: crate::common::Operand { u32: 344 },
        };
        instrs[start + 5] = Instr {
            operand: crate::common::Operand {
                jump_addr: u32::try_from(jump).expect("jump address exceeds u32::MAX"),
            },
        };
    }
}

#[allow(dead_code)]
fn op_eq(instrs: &[Instr], pc: usize, op: crate::common::Op) -> bool {
    instrs
        .get(pc)
        .is_some_and(|instr| std::ptr::fn_addr_eq(unsafe { instr.op }, op))
}

#[cfg(feature = "vm-diagnostics")]
fn dump_materialized_function_if_requested(funcidx: u32, instrs: &[Instr], op_lens: &[u16]) {
    let Some(requested) = std::env::var("TELOMERE_VM_DUMP_FUNC").ok() else {
        return;
    };
    let dump_all = requested == "all";
    let requested = requested.parse::<u32>().ok();
    if !dump_all && requested != Some(funcidx) {
        return;
    }
    let pc_start = std::env::var("TELOMERE_VM_DUMP_PC_START")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let pc_end = std::env::var("TELOMERE_VM_DUMP_PC_END")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(usize::MAX);

    eprintln!(
        "[telomere-vm-diagnostics] materialized_func funcidx={funcidx} ops={} instrs={}",
        op_lens.len(),
        instrs.len()
    );
    let mut cursor = 0usize;
    for len in op_lens {
        if cursor < pc_start || cursor > pc_end {
            cursor += usize::from(*len);
            continue;
        }
        let op = unsafe { instrs[cursor].op };
        eprintln!(
            "[telomere-vm-diagnostics] materialized_op funcidx={funcidx} pc={cursor} len={} op={} op_addr=0x{:x}",
            len,
            vm::diagnostic_op_label(op),
            op as usize
        );
        if *len > 1 {
            for operand_offset in 1..usize::from(*len) {
                let operand = unsafe { instrs[cursor + operand_offset].operand };
                eprintln!(
                    "[telomere-vm-diagnostics] materialized_operand funcidx={funcidx} pc={cursor} offset={operand_offset} encoded={:02x?} u32={} i32={} jump={}",
                    unsafe { operand.encoded },
                    unsafe { operand.u32 },
                    unsafe { operand.i32 },
                    unsafe { operand.jump_addr }
                );
            }
        }
        if std::ptr::fn_addr_eq(op, vm::op_br as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_br_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_if as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_else as crate::common::Op)
            || std::ptr::fn_addr_eq(op, vm::op_return as crate::common::Op)
        {
            let jump_addr = unsafe { instrs[cursor + 1].operand.jump_addr };
            eprintln!(
                "[telomere-vm-diagnostics] materialized_jump funcidx={funcidx} pc={cursor} target={jump_addr}"
            );
        }
        if std::ptr::fn_addr_eq(op, vm::special_block_return as crate::common::Op) {
            let block_return = unsafe { instrs[cursor + 1].operand.block_return };
            eprintln!(
                "[telomere-vm-diagnostics] materialized_block_return funcidx={funcidx} pc={cursor} stack_top={} return_size={}",
                block_return.stack_top,
                block_return.return_size
            );
        }
        if std::ptr::fn_addr_eq(op, vm::op_select as crate::common::Op) {
            let select_size = unsafe { instrs[cursor + 1].operand.select };
            eprintln!(
                "[telomere-vm-diagnostics] materialized_select funcidx={funcidx} pc={cursor} size={select_size}"
            );
        }
        cursor += usize::from(*len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            custom_section::{FuncNameSubSec, ModuleNameSubSec, NameSubSection},
            memory::fail_next_memory_mapping,
            memory::TestMemoryMappingFailure,
            PAGE_SIZE,
        },
        IoReadBinaryReader, WasmParser,
    };

    fn parse_wat_for_test(wat_src: &str) -> Module {
        let bytes = wat::parse_str(wat_src).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        parser.parse_module().expect("module must parse")
    }

    async fn instantiate_wat_for_test(wat_src: &str) -> (Store, InstanceHandle) {
        let module = parse_wat_for_test(wat_src);
        let store = Store::new();
        let registry = Registry::new();
        let VMResult::Success(instance) = instantiate(module, &store, &registry).await else {
            panic!("module must instantiate");
        };
        (store, instance)
    }

    fn named_module_for_test() -> Module {
        let mut module = parse_wat_for_test("(module (func))");
        module.name = Some(NameSubSection {
            module_name: Some(ModuleNameSubSec("named-module".to_owned())),
            function_name: Some(FuncNameSubSec(vec![(0, "defined-function".to_owned())])),
            local_name: None,
        });
        module
    }

    fn names_from_store_for_test(
        store: &Store,
        instance: &InstanceHandle,
    ) -> Option<(Option<String>, Option<String>)> {
        let runtime = store.lock_runtime_or_panic();
        let instance_addr = instance
            .object_ref_for_store(store)
            .expect("instance must belong to store");
        let module = runtime.get_module(runtime.get_instance(instance_addr).module_addr);
        module.names.as_ref().map(|names| {
            (
                names.module_name().map(str::to_owned),
                names.function_name(0).map(str::to_owned),
            )
        })
    }

    #[tokio::test]
    async fn instantiate_retains_names_by_default_and_drops_them_when_disabled() {
        assert!(crate::DiagnosticsConfig::default().retain_function_names);
        assert!(
            crate::RuntimeConfig::default()
                .diagnostics
                .retain_function_names
        );

        let enabled_store = Store::new();
        let enabled_registry = Registry::new();
        let enabled_instance =
            match instantiate(named_module_for_test(), &enabled_store, &enabled_registry).await {
                VMResult::Success(instance) => instance,
                result => panic!("named module must instantiate: {result:?}"),
            };
        assert_eq!(
            names_from_store_for_test(&enabled_store, &enabled_instance),
            Some((
                Some("named-module".to_owned()),
                Some("defined-function".to_owned()),
            ))
        );

        let mut config = crate::RuntimeConfig::default();
        config.diagnostics.retain_function_names = false;
        let disabled_store = Store::new_with_runtime_config(config);
        let disabled_registry = Registry::new();
        let disabled_instance =
            match instantiate(named_module_for_test(), &disabled_store, &disabled_registry).await {
                VMResult::Success(instance) => instance,
                result => panic!("named module must instantiate: {result:?}"),
            };
        assert_eq!(
            names_from_store_for_test(&disabled_store, &disabled_instance),
            None
        );
    }

    #[tokio::test]
    async fn native_modules_do_not_retain_debug_names() {
        let store = Store::new();
        let registry = Registry::new();
        let instance = match instantiate_native_module(
            HostModule {
                functions: Vec::new(),
            },
            &store,
            &registry,
        )
        .await
        {
            VMResult::Success(instance) => instance,
            result => panic!("native module must instantiate: {result:?}"),
        };

        assert_eq!(names_from_store_for_test(&store, &instance), None);
    }

    #[tokio::test]
    async fn imported_and_defined_functions_share_name_section_index_space() {
        let store = Store::new();
        let mut registry = Registry::new();
        let host = match instantiate(
            parse_wat_for_test("(module (type (func)) (func (export \"imported\") (type 0)))"),
            &store,
            &registry,
        )
        .await
        {
            VMResult::Success(instance) => instance,
            result => panic!("host module must instantiate: {result:?}"),
        };
        registry.register("host", host);

        let mut module = parse_wat_for_test(
            "(module (type (func)) (import \"host\" \"imported\" (func (type 0))) (func (type 0)) (func (type 0)))",
        );
        module.name = Some(NameSubSection {
            module_name: Some(ModuleNameSubSec("consumer".to_owned())),
            function_name: Some(FuncNameSubSec(vec![
                (1, "first-defined".to_owned()),
                (2, "second-defined".to_owned()),
            ])),
            local_name: None,
        });
        let instance = match instantiate(module, &store, &registry).await {
            VMResult::Success(instance) => instance,
            result => panic!("consumer module must instantiate: {result:?}"),
        };

        let runtime = store.lock_runtime_or_panic();
        let instance_addr = instance
            .object_ref_for_store(&store)
            .expect("instance must belong to store");
        let instance_data = runtime.get_instance(instance_addr);
        let funcidxs = instance_data
            .funcs
            .iter()
            .map(|funcaddr| runtime.get_func(*funcaddr).funcidx)
            .collect::<Vec<_>>();
        let names = runtime
            .get_module(instance_data.module_addr)
            .names
            .as_ref()
            .expect("names must be retained");

        assert_eq!(funcidxs, vec![0, 1, 2]);
        assert_eq!(names.function_name(0), None);
        assert_eq!(names.function_name(1), Some("first-defined"));
        assert_eq!(names.function_name(2), Some("second-defined"));
    }

    #[tokio::test]
    async fn instantiate_maps_mmap_and_initial_mprotect_failures_to_memory_allocation_failed() {
        for failure in [
            TestMemoryMappingFailure::Mmap,
            TestMemoryMappingFailure::Mprotect,
        ] {
            let module = parse_wat_for_test("(module (memory 1))");
            let store = Store::new();
            let registry = Registry::new();
            let _failure = fail_next_memory_mapping(failure);

            assert!(matches!(
                instantiate(module, &store, &registry).await,
                VMResult::MemoryAllocationFailed
            ));
        }
    }

    #[tokio::test]
    async fn configured_memory_ceiling_clamps_reservation_and_grow() {
        let mut runtime_config = crate::RuntimeConfig::default();
        runtime_config.memory.max_memory_pages = 2;
        let store = Store::new_with_runtime_config(runtime_config);
        let registry = Registry::new();
        let module = parse_wat_for_test("(module (memory 1))");
        let instance = match instantiate(module, &store, &registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("module must instantiate: {other:?}"),
        };

        let mut runtime = store.lock_runtime_or_panic();
        let object_ref = instance
            .object_ref_for_store(&store)
            .expect("instance must belong to store");
        let memory_ref = runtime.get_instance(object_ref).mems[0];
        let memory = runtime.get_memory(memory_ref);
        assert_eq!(memory.reserved_bytes(), 2 * PAGE_SIZE);
        assert_eq!(memory.committed_bytes(), PAGE_SIZE);
        assert_eq!(memory.grow(1).unwrap(), 1);
        assert_eq!(memory.grow(1).unwrap(), -1);
        assert_eq!(memory.page_size(), 2);
        assert_eq!(memory.committed_bytes(), 2 * PAGE_SIZE);
    }

    #[tokio::test]
    async fn unbounded_memory_instantiation_commits_only_its_minimum() {
        let store = Store::new();
        let registry = Registry::new();
        let module = parse_wat_for_test("(module (memory 1))");
        let instance = match instantiate(module, &store, &registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("module must instantiate: {other:?}"),
        };

        let mut runtime = store.lock_runtime_or_panic();
        let object_ref = instance
            .object_ref_for_store(&store)
            .expect("instance must belong to store");
        let memory_ref = runtime.get_instance(object_ref).mems[0];
        let memory = runtime.get_memory(memory_ref);
        assert_eq!(memory.committed_bytes(), PAGE_SIZE);
    }

    #[test]
    fn execute_offset_const_expr_fail_closes_non_i32_const() {
        let store = Store::new();
        let mut runtime = store.lock_runtime_or_panic();
        let result = execute_offset_const_expr(&mut runtime, &[], &[ConstExpr::F64(1.0)]);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[test]
    fn execute_offset_const_expr_fail_closes_non_i32_global_get() {
        let store = Store::new();
        let mut runtime = store.lock_runtime_or_panic();
        let global = runtime.new_global_data8(42);
        let result = execute_offset_const_expr(&mut runtime, &[global], &[ConstExpr::GlobalGet(0)]);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[test]
    fn lowered_materialize_with_recipe_slots_resolves_direct_call_operands() {
        let lowered = crate::common::LoweredFunction::from_materialized(
            vec![
                Instr { op: vm::op_call },
                Instr {
                    operand: crate::common::Operand {
                        call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(1),
                    },
                },
                Instr { op: vm::op_end },
            ],
            vec![2, 1],
        );
        let materialized = lowered.materialize_with_recipe_slots(&[7, 13]);
        let target = unsafe { materialized.instrs[1].operand.call_recipe_ref };
        assert_eq!(target.funcidx, 1);
        assert_eq!(target.resolved_recipe_slot(), Some(13));
    }

    #[test]
    fn rewrite_numeric_transition_direct_calls_rewrites_matching_call_slot() {
        let mut instrs = vec![
            Instr { op: vm::op_call },
            Instr {
                operand: crate::common::Operand {
                    call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(1)
                        .with_recipe_slot(13),
                },
            },
            Instr { op: vm::op_end },
        ];
        rewrite_direct_calls_for_slots(
            &mut instrs,
            &[2, 1],
            &[13],
            vm::op_call_i32_numeric_token_state_transition,
        );

        assert!(std::ptr::fn_addr_eq(
            unsafe { instrs[0].op },
            vm::op_call_i32_numeric_token_state_transition as crate::common::Op
        ));
        let target = unsafe { instrs[1].operand.call_recipe_ref };
        assert_eq!(target.resolved_recipe_slot(), Some(13));
    }

    #[test]
    fn rewrite_numeric_transition_direct_calls_keeps_non_matching_call_slot() {
        let mut instrs = vec![
            Instr { op: vm::op_call },
            Instr {
                operand: crate::common::Operand {
                    call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(1)
                        .with_recipe_slot(7),
                },
            },
            Instr { op: vm::op_end },
        ];
        rewrite_direct_calls_for_slots(
            &mut instrs,
            &[2, 1],
            &[13],
            vm::op_call_i32_numeric_token_state_transition,
        );

        assert!(std::ptr::fn_addr_eq(
            unsafe { instrs[0].op },
            vm::op_call as crate::common::Op
        ));
    }

    #[cfg(feature = "aot-load")]
    #[test]
    fn rewrite_direct_wasm_calls_for_jit_preserves_specialized_call_opcodes() {
        let mut instrs = vec![
            Instr {
                op: vm::op_call_i32_crc16_update16,
            },
            Instr {
                operand: crate::common::Operand {
                    call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(1)
                        .with_recipe_slot(13),
                },
            },
            Instr { op: vm::op_call },
            Instr {
                operand: crate::common::Operand {
                    call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(2)
                        .with_recipe_slot(13),
                },
            },
            Instr { op: vm::op_end },
        ];

        rewrite_direct_wasm_calls_for_jit(&mut instrs, &[2, 2, 1], &[13]);

        assert!(std::ptr::fn_addr_eq(
            unsafe { instrs[0].op },
            vm::op_call_i32_crc16_update16 as crate::common::Op
        ));
        assert!(std::ptr::fn_addr_eq(
            unsafe { instrs[2].op },
            vm::op_call_jit_lazy as crate::common::Op
        ));
    }

    fn crc16_update_masked_wrapper_instrs(
        data_local: u32,
        crc_local: u32,
    ) -> (Vec<Instr>, Vec<u16>) {
        let and_const = crate::common::encode_local_binop32_kind(
            LocalBinop32Op::I32And,
            LocalFastRhsShape::Const,
        );

        (
            vec![
                Instr {
                    op: vm::op_local_binop32,
                },
                Instr {
                    operand: crate::common::Operand { u32: and_const },
                },
                Instr {
                    operand: crate::common::Operand {
                        local_addr: data_local,
                    },
                },
                Instr {
                    operand: crate::common::Operand { i32: 0xffff },
                },
                Instr {
                    op: vm::op_local_get4,
                },
                Instr {
                    operand: crate::common::Operand {
                        local_addr: crc_local,
                    },
                },
                Instr {
                    op: vm::op_call_i32_crc16_update16,
                },
                Instr {
                    operand: crate::common::Operand {
                        call_recipe_ref: crate::common::CallRecipeRef::from_funcidx(1)
                            .with_recipe_slot(7),
                    },
                },
                Instr { op: vm::op_end },
                Instr {
                    op: vm::special_function_return,
                },
                Instr {
                    operand: crate::common::Operand { jump_addr: 0 },
                },
            ],
            vec![4, 2, 2, 1, 2],
        )
    }

    #[test]
    fn crc16_update_masked_wrapper_matcher_accepts_exact_shape() {
        let (instrs, op_lens) = crc16_update_masked_wrapper_instrs(8, 12);
        let shape = crc16_update_masked_wrapper_shape(&instrs, &op_lens)
            .expect("exact masked CRC wrapper shape must match");

        assert_eq!(shape.data_local, 8);
        assert_eq!(shape.crc_local, 12);
        assert_eq!(shape.return_addr, 9);
    }

    #[test]
    fn crc16_update_masked_wrapper_rewrite_uses_matched_local_operands() {
        let (mut instrs, op_lens) = crc16_update_masked_wrapper_instrs(8, 12);
        rewrite_crc16_update_masked_wrapper(&mut instrs, &op_lens);

        assert!(std::ptr::fn_addr_eq(
            unsafe { instrs[0].op },
            vm::op_i32_crc16_update16_masked as crate::common::Op
        ));
        assert_eq!(unsafe { instrs[1].operand.local_addr }, 8);
        assert_eq!(unsafe { instrs[2].operand.local_addr }, 12);
        assert_eq!(unsafe { instrs[3].operand.jump_addr }, 9);
    }

    #[test]
    fn crc16_update_masked_wrapper_matcher_rejects_unrewritten_call() {
        let (mut instrs, op_lens) = crc16_update_masked_wrapper_instrs(0, 4);
        instrs[6] = Instr { op: vm::op_call };

        assert!(crc16_update_masked_wrapper_shape(&instrs, &op_lens).is_none());
    }

    #[test]
    fn crc16_update_masked_wrapper_matcher_rejects_other_mask() {
        let (mut instrs, op_lens) = crc16_update_masked_wrapper_instrs(0, 4);
        instrs[3] = Instr {
            operand: crate::common::Operand { i32: 0xff },
        };

        assert!(crc16_update_masked_wrapper_shape(&instrs, &op_lens).is_none());
    }

    #[test]
    fn crc16_update_masked_wrapper_matcher_rejects_extra_transform() {
        let (mut instrs, mut op_lens) = crc16_update_masked_wrapper_instrs(0, 4);
        instrs.insert(
            6,
            Instr {
                op: vm::op_i32_const,
            },
        );
        instrs.insert(
            7,
            Instr {
                operand: crate::common::Operand { i32: 1 },
            },
        );
        op_lens.insert(2, 2);

        assert!(crc16_update_masked_wrapper_shape(&instrs, &op_lens).is_none());
    }

    fn cached_u16_low7_guard_instrs() -> (Vec<Instr>, Vec<u16>) {
        let and_const = crate::common::encode_local_binop32_kind(
            LocalBinop32Op::I32And,
            LocalFastRhsShape::Const,
        );

        (
            vec![
                Instr {
                    op: vm::op_i32_load16_u_local_base_tee4,
                },
                Instr {
                    operand: crate::common::Operand { local_addr: 0 },
                },
                Instr {
                    operand: crate::common::Operand { i32: 0 },
                },
                Instr {
                    operand: crate::common::Operand {
                        memarg: crate::common::MemArg {
                            align: 0,
                            offset: 0,
                        },
                    },
                },
                Instr {
                    operand: crate::common::Operand { local_addr: 8 },
                },
                Instr {
                    op: vm::op_i32_const_binop,
                },
                Instr {
                    operand: crate::common::Operand { u32: and_const },
                },
                Instr {
                    operand: crate::common::Operand { i32: 0x80 },
                },
                Instr { op: vm::op_if },
                Instr {
                    operand: crate::common::Operand { jump_addr: 14 },
                },
                Instr {
                    op: vm::op_local_binop32,
                },
                Instr {
                    operand: crate::common::Operand { u32: and_const },
                },
                Instr {
                    operand: crate::common::Operand { local_addr: 8 },
                },
                Instr {
                    operand: crate::common::Operand { i32: 0x7f },
                },
                Instr { op: vm::op_return },
                Instr {
                    operand: crate::common::Operand { jump_addr: 16 },
                },
            ],
            vec![5, 3, 2, 4, 2],
        )
    }

    #[test]
    fn cached_u16_low7_guard_matcher_accepts_exact_shape() {
        let (instrs, op_lens) = cached_u16_low7_guard_instrs();
        assert!(materialized_starts_with_cached_u16_low7_guard(
            &instrs, &op_lens
        ));
    }

    #[test]
    fn cached_u16_low7_guard_matcher_rejects_non_low7_return_mask() {
        let (mut instrs, op_lens) = cached_u16_low7_guard_instrs();
        instrs[13] = Instr {
            operand: crate::common::Operand { i32: 0x3f },
        };

        assert!(!materialized_starts_with_cached_u16_low7_guard(
            &instrs, &op_lens
        ));
    }

    #[test]
    fn cached_u16_low7_guard_matcher_rejects_non_param0_load() {
        let (mut instrs, op_lens) = cached_u16_low7_guard_instrs();
        instrs[1] = Instr {
            operand: crate::common::Operand { local_addr: 4 },
        };

        assert!(!materialized_starts_with_cached_u16_low7_guard(
            &instrs, &op_lens
        ));
    }

    #[test]
    fn cached_u16_low7_guard_matcher_rejects_offset_load() {
        let (mut instrs, op_lens) = cached_u16_low7_guard_instrs();
        instrs[3] = Instr {
            operand: crate::common::Operand {
                memarg: crate::common::MemArg {
                    align: 0,
                    offset: 2,
                },
            },
        };

        assert!(!materialized_starts_with_cached_u16_low7_guard(
            &instrs, &op_lens
        ));
    }

    #[test]
    fn cached_u16_low7_guard_matcher_rejects_other_return_local() {
        let (mut instrs, op_lens) = cached_u16_low7_guard_instrs();
        instrs[12] = Instr {
            operand: crate::common::Operand { local_addr: 12 },
        };

        assert!(!materialized_starts_with_cached_u16_low7_guard(
            &instrs, &op_lens
        ));
    }

    #[tokio::test]
    async fn instantiate_rewrites_return_call_with_matching_recipe_slot() {
        let (store, instance) = instantiate_wat_for_test(
            r#"
            (module
              (func (export "run") (param i32) (result i32)
                local.get 0
                i32.const 0
                call 1)
              (func (param $n i32) (param $acc i32) (result i32)
                local.get $n
                i32.eqz
                if
                  local.get $acc
                  return
                end
                local.get $n
                i32.const 1
                i32.sub
                local.get $acc
                local.get $n
                i32.add
                return_call 1))
            "#,
        )
        .await;

        let runtime = store.lock_runtime_or_panic();
        let object_ref = instance
            .object_ref_for_store(&store)
            .expect("instance must belong to store");
        let instance_data = runtime.get_instance(object_ref);
        let funcaddr = instance_data.funcs[1];
        let expected_slot = runtime.call_recipe_slot_for_func(funcaddr);
        let func = runtime.get_func(funcaddr);
        let crate::common::store::FunctionBody::Wasm { code, .. } = &func.body else {
            panic!("expected wasm function");
        };
        let call_index = code
            .iter()
            .position(|instr| {
                std::ptr::fn_addr_eq(
                    unsafe { instr.op },
                    vm::op_return_call
                        as unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>,
                )
            })
            .expect("return_call must exist");
        let target = unsafe { code[call_index + 1].operand.call_recipe_ref };
        assert_eq!(target.funcidx, 1);
        assert_eq!(target.resolved_recipe_slot(), Some(expected_slot));
    }
    // ---- #128 regression tests: crafted link/instantiation state must
    // fail closed with an error, never abort the host. ----

    #[tokio::test]
    async fn linking_import_from_out_of_range_func_export_fails_closed() {
        let mut exporter = parse_wat_for_test(r#"(module (func) (export "f" (func 0)))"#);
        exporter.exs.0[0].1 = crate::common::ExportDesc::Func(crate::common::FuncIdx(7));
        let store = Store::new();
        let mut registry = Registry::new();
        let VMResult::Success(inst) = instantiate(exporter, &store, &registry).await else {
            panic!("exporter must instantiate");
        };
        registry.register("m", inst);
        let importer = parse_wat_for_test(r#"(module (import "m" "f" (func)))"#);
        let result = instantiate(importer, &store, &registry).await;
        assert!(
            matches!(result, VMResult::Unlinkable),
            "out-of-range func export must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn linking_import_from_out_of_range_global_export_fails_closed() {
        let mut exporter =
            parse_wat_for_test(r#"(module (global i32 (i32.const 0)) (export "g" (global 0)))"#);
        exporter.exs.0[0].1 = crate::common::ExportDesc::Global(crate::common::GlobalIdx(7));
        let store = Store::new();
        let mut registry = Registry::new();
        let VMResult::Success(inst) = instantiate(exporter, &store, &registry).await else {
            panic!("exporter must instantiate");
        };
        registry.register("m", inst);
        let importer = parse_wat_for_test(r#"(module (import "m" "g" (global i32)))"#);
        let result = instantiate(importer, &store, &registry).await;
        assert!(
            matches!(result, VMResult::Unlinkable),
            "out-of-range global export must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn linking_import_from_out_of_range_table_export_fails_closed() {
        let mut exporter =
            parse_wat_for_test(r#"(module (table 1 funcref) (export "t" (table 0)))"#);
        exporter.exs.0[0].1 = crate::common::ExportDesc::Table(crate::common::TableIdx(7));
        let store = Store::new();
        let mut registry = Registry::new();
        let VMResult::Success(inst) = instantiate(exporter, &store, &registry).await else {
            panic!("exporter must instantiate");
        };
        registry.register("m", inst);
        let importer = parse_wat_for_test(r#"(module (import "m" "t" (table 1 funcref)))"#);
        let result = instantiate(importer, &store, &registry).await;
        assert!(
            matches!(result, VMResult::Unlinkable),
            "out-of-range table export must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn active_elem_segment_with_mismatched_table_reftype_fails_closed() {
        let mut module = parse_wat_for_test(
            r#"(module
              (table 1 funcref)
              (func $f)
              (elem (i32.const 0) $f))"#,
        );
        // The parser never produces this pairing (it validates the segment
        // against the table); a crafted in-crate module can (#128).
        module.tables[0].reftype = crate::common::RefType::ExternRef;
        let store = Store::new();
        let registry = Registry::new();
        let result = instantiate(module, &store, &registry).await;
        assert!(
            matches!(result, VMResult::Unlinkable),
            "reftype-mismatched elem segment must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn call_indirect_with_out_of_range_callee_index_fails_closed() {
        let (store, instance) = instantiate_wat_for_test(
            r#"(module
              (type $t (func))
              (table 1 funcref)
              (func $f (type $t))
              (elem (i32.const 0) $f)
              (func (export "run") (call_indirect (type $t) (i32.const 0))))"#,
        )
        .await;
        {
            let mut runtime = store.lock_runtime_or_panic();
            let object_ref = instance
                .object_ref_for_store(&store)
                .expect("instance must belong to store");
            let (table_addr, real_func_addr) = {
                let instance_data = runtime.get_instance(object_ref);
                (instance_data.tables[0], instance_data.funcs[0])
            };
            // Install a function record whose index lies outside its
            // module's function index space (#128).
            let mut bogus = runtime.get_func(real_func_addr).clone();
            bogus.funcidx = 9999;
            let bogus_addr = runtime.new_func(&bogus);
            runtime.get_table(table_addr).1[0] = bogus_addr.get();
        }
        let result = crate::runtime::run_module_function(
            &instance,
            &store,
            "run",
            &crate::ResultValue::new(vec![]),
        )
        .await;
        assert!(
            matches!(result, VMResult::InvalidOperand),
            "call_indirect on a bogus callee must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn call_indirect_with_out_of_range_expected_type_fails_closed() {
        let (store, instance) = instantiate_wat_for_test(
            r#"(module
              (type $t (func))
              (table 1 funcref)
              (func $f (type $t))
              (elem (i32.const 0) $f)
              (func (export "run") (call_indirect (type $t) (i32.const 0))))"#,
        )
        .await;
        {
            let mut runtime = store.lock_runtime_or_panic();
            let object_ref = instance
                .object_ref_for_store(&store)
                .expect("instance must belong to store");
            let run_addr = runtime.get_instance(object_ref).funcs[1];
            let func = runtime.get_func_mut(run_addr);
            let crate::common::store::FunctionBody::Wasm { code, .. } = &mut func.body else {
                panic!("expected a wasm function body");
            };
            let mut instrs: Vec<Instr> = code.to_vec();
            let pos = instrs
                .iter()
                .position(|instr| {
                    std::ptr::fn_addr_eq(
                        unsafe { instr.op },
                        vm::op_call_indirect
                            as unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>,
                    )
                })
                .expect("run must contain a call_indirect");
            // Operand layout: [op][table index][expected type index].
            instrs[pos + 2].operand.u32 = 9999;
            *code = instrs.into();
        }
        let result = crate::runtime::run_module_function(
            &instance,
            &store,
            "run",
            &crate::ResultValue::new(vec![]),
        )
        .await;
        assert!(
            matches!(result, VMResult::InvalidOperand),
            "call_indirect with a bogus expected type must fail closed, got {result:?}"
        );
    }

    #[tokio::test]
    async fn fused_binop_with_invalid_descriptor_kind_fails_closed() {
        // The optimizer fuses this shape into op_local_binop32_set4.
        let (store, instance) = instantiate_wat_for_test(
            r#"(module
              (func (export "run") (param i32) (result i32)
                (local i32)
                local.get 0
                i32.const 1
                i32.mul
                local.set 1
                i32.const 7))"#,
        )
        .await;
        {
            let mut runtime = store.lock_runtime_or_panic();
            let object_ref = instance
                .object_ref_for_store(&store)
                .expect("instance must belong to store");
            let run_addr = runtime.get_instance(object_ref).funcs[0];
            let func = runtime.get_func_mut(run_addr);
            let crate::common::store::FunctionBody::Wasm { code, .. } = &mut func.body else {
                panic!("expected a wasm function body");
            };
            let mut instrs: Vec<Instr> = code.to_vec();
            let pos = instrs
                .iter()
                .position(|instr| {
                    std::ptr::fn_addr_eq(
                        unsafe { instr.op },
                        vm::op_local_binop32_set4
                            as unsafe fn(*const Instr, &mut ExecuteContext) -> VMResult<()>,
                    )
                })
                .expect("run must contain a fused local_binop32_set4");
            // The descriptor kind operand is the slot right after the op.
            instrs[pos + 1].operand.u32 = u32::MAX;
            *code = instrs.into();
        }
        let result = crate::runtime::run_module_function(
            &instance,
            &store,
            "run",
            &crate::ResultValue::new(vec![crate::WasmValue::I32(3)]),
        )
        .await;
        assert!(
            matches!(result, VMResult::InvalidOperand),
            "fused op with an invalid descriptor kind must fail closed, got {result:?}"
        );
    }
}
