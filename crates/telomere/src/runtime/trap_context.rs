//! Cold, crate-private trap context capture.
//!
//! Frame zero identifies the dispatch unit that trapped. Older frames identify
//! return continuations, so their program counters point after the call site.
//! Tail calls replace their caller frame and are therefore intentionally absent.

use crate::{
    common::{store::FunctionBody, Instr, LocalReference, ObjectRef, StablePc, Stack, StoreInner},
    runtime::vm::instr_index_from_base,
    VMResult,
};

const HEAD_FRAMES: usize = 48;
const TAIL_FRAMES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CapturedFrameKind {
    Wasm,
    Host,
    AsyncHost,
    Unresolved,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CapturedFrame {
    pub(crate) depth: u32,
    pub(crate) code_addr: ObjectRef,
    pub(crate) funcidx: Option<u32>,
    /// Frame zero is the faulting dispatch unit; older entries are return PCs.
    /// Fused interpreter dispatch units report their first instruction.
    /// Native JIT traps recover decoded-instruction attribution through the trap-site table;
    /// only an unknown site reports `None`.
    pub(crate) pc_index: Option<u32>,
    pub(crate) kind: CapturedFrameKind,
}

#[derive(Debug)]
pub(crate) struct TrapContext {
    pub(crate) result: VMResult<()>,
    pub(crate) task_id: u32,
    /// Innermost first. When truncated, this contains the innermost 48 then
    /// the outermost 16 frames. Frames elided by tail calls are not present.
    pub(crate) frames: Vec<CapturedFrame>,
    pub(crate) total_frames: u32,
    pub(crate) truncated: bool,
}

/// Captures the scheduler-visible call stack at the instant a trap materializes.
///
/// This is deliberately cold and non-inlined: it allocates only while a guest
/// call is already terminating. The bounded walk tolerates malformed raw stack
/// references by returning the valid prefix rather than panicking.
///
/// Capture allocates a `Box<TrapContext>` and one `Vec<CapturedFrame>`, including
/// for `MemoryAllocationFailed`: that guest allocation failure is distinct from
/// the host allocator used here. Host allocator OOM is not recovered into empty
/// frames; it follows the allocator's normal behavior.
#[cold]
#[inline(never)]
pub(crate) fn capture_trap_context(
    runtime: &StoreInner,
    stack: &Stack,
    local_reference: LocalReference,
    innermost_pc: Option<*const Instr>,
    result: &VMResult<()>,
    task_id: u32,
) -> Box<TrapContext> {
    let mut head = Vec::with_capacity(HEAD_FRAMES);
    let mut tail = [None; TAIL_FRAMES];
    let mut tail_next = 0usize;
    let mut tail_len = 0usize;
    let mut total_frames = 0u32;
    let mut truncated = false;
    let mut local_reference = local_reference;
    let mut pending_return_pc: Option<StablePc> = None;

    // Every valid frame occupies at least one packed CallStackInfo. The extra
    // iteration makes a corrupted self-cycle terminate rather than loop forever.
    let walk_limit = stack
        .jit_memory_len()
        .checked_div(std::mem::size_of::<crate::common::stack::CallStackInfo>())
        .unwrap_or(0)
        .saturating_add(1);
    let mut walked = 0usize;
    while walked < walk_limit {
        let Some(record) = stack.frame_record(&local_reference) else {
            break;
        };
        // `StackFrameRecord` retains this for the runtime stack layout. Trap
        // reporting deliberately resolves names from the function record instead.
        let _ = record.instance;
        let pc = match pending_return_pc {
            Some(return_pc) => return_pc.resolve_optional(runtime, stack, local_reference),
            None => innermost_pc,
        };
        let (funcidx, kind) = match runtime.try_get_func(record.code_addr) {
            Some(function) => {
                let kind = match &function.body {
                    FunctionBody::Wasm { .. } => CapturedFrameKind::Wasm,
                    FunctionBody::Host(_) => CapturedFrameKind::Host,
                    FunctionBody::AsyncHost(_) => CapturedFrameKind::AsyncHost,
                };
                (Some(function.funcidx), kind)
            }
            None => (None, CapturedFrameKind::Unresolved),
        };
        let frame = CapturedFrame {
            depth: total_frames,
            code_addr: record.code_addr,
            funcidx,
            pc_index: pc.and_then(|pc| instr_index_from_base(pc, record.code_base)),
            kind,
        };

        total_frames = total_frames.saturating_add(1);
        if total_frames as usize <= HEAD_FRAMES {
            head.push(frame);
        } else {
            tail[tail_next] = Some(frame);
            tail_next = (tail_next + 1) % TAIL_FRAMES;
            tail_len = (tail_len + 1).min(TAIL_FRAMES);
        }
        pending_return_pc = Some(record.return_pc);
        local_reference = record.prev;
        walked += 1;
    }
    if walked == walk_limit {
        truncated = true;
    }
    if total_frames as usize > HEAD_FRAMES + TAIL_FRAMES {
        truncated = true;
    }

    let mut frames = head;
    let tail_start = if tail_len == TAIL_FRAMES {
        tail_next
    } else {
        0
    };
    for offset in 0..tail_len {
        let index = (tail_start + offset) % TAIL_FRAMES;
        frames.push(tail[index].take().expect("tail frame must be initialized"));
    }

    Box::new(TrapContext {
        result: result.clone(),
        task_id,
        frames,
        total_frames,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{ExecuteContext, InstanceHandle},
        IoReadBinaryReader, Registry, ResultValue, Store, WasmParser, WasmValue,
    };

    const HOST_REENTER_RETURN: [Instr; 2] = [
        Instr {
            op: crate::runtime::vm::special_function_return,
        },
        Instr {
            operand: crate::common::Operand { u32: 0 },
        },
    ];

    fn parse_wat(wat_src: &str) -> crate::Module {
        let bytes = wat::parse_str(wat_src).expect("test WAT must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        WasmParser::new(&mut reader)
            .parse_module()
            .expect("test module must parse")
    }

    async fn instantiate_wat(wat_src: &str) -> (Store, InstanceHandle) {
        let store = Store::new();
        let registry = Registry::new();
        let instance = match crate::instantiate(parse_wat(wat_src), &store, &registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("test module must instantiate: {other:?}"),
        };
        (store, instance)
    }

    fn take_context(store: &Store) -> Box<TrapContext> {
        store
            .lock_runtime_or_panic()
            .take_last_trap()
            .expect("trapping guest call must publish a context")
    }

    fn assert_innermost_pc_in_range(context: &TrapContext, store: &Store) {
        let frame = context.frames.first().expect("trapping frame must exist");
        let index = frame.pc_index.expect("wasm trap must have a pc index");
        let runtime = store.lock_runtime_or_panic();
        let function = runtime
            .try_get_func(frame.code_addr)
            .expect("frame must resolve to a function");
        assert!(
            (index as usize) < function.code().expect("frame must be wasm").len(),
            "pc index {index} must name a decoded instruction"
        );
    }

    #[tokio::test]
    async fn trap_context_unreachable_deep_chain() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (func $entry (export "run") call $f1)
              (func $f1 call $f2)
              (func $f2 call $f3)
              (func $f3 unreachable))
            "#,
        )
        .await;

        let result =
            crate::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        assert!(matches!(result, VMResult::Unreachable));

        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::Unreachable));
        assert_eq!(context.task_id, 0);
        assert!(!context.truncated);
        assert_eq!(context.total_frames, 4);
        assert_eq!(context.frames.len(), 4);
        assert_eq!(
            context
                .frames
                .iter()
                .map(|frame| frame.funcidx)
                .collect::<Vec<_>>(),
            vec![Some(3), Some(2), Some(1), Some(0)]
        );
        assert_eq!(context.frames[0].pc_index, Some(0));
        assert!(context
            .frames
            .iter()
            .all(|frame| frame.kind == CapturedFrameKind::Wasm));
    }

    #[tokio::test]
    async fn trap_context_oob_memory_access_deep_chain() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (memory 1)
              (func $entry (export "run") (param i32) local.get 0 call $f1)
              (func $f1 (param i32) local.get 0 call $f2)
              (func $f2 (param i32) local.get 0 call $f3)
              (func $f3 (param i32) local.get 0 i32.load drop))
            "#,
        )
        .await;

        let result = crate::run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(65_536)]),
        )
        .await;
        assert!(matches!(result, VMResult::MemoryIndexOutOfRange));

        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::MemoryIndexOutOfRange));
        assert_eq!(context.task_id, 0);
        assert_eq!(
            context
                .frames
                .iter()
                .map(|frame| frame.funcidx)
                .collect::<Vec<_>>(),
            vec![Some(3), Some(2), Some(1), Some(0)]
        );
        assert_innermost_pc_in_range(&context, &store);
    }

    #[tokio::test]
    async fn trap_context_call_indirect_type_mismatch_uses_caller_frame() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (type $actual (func))
              (type $expected (func (param i32)))
              (table 1 funcref)
              (elem (i32.const 0) $target)
              (func $entry (export "run") call $f1)
              (func $f1 call $f2)
              (func $f2 i32.const 7 i32.const 0 call_indirect (type $expected))
              (func $target (type $actual)))
            "#,
        )
        .await;

        let result =
            crate::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        assert!(matches!(result, VMResult::CallIndirectInvalidType));

        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::CallIndirectInvalidType));
        assert_eq!(context.task_id, 0);
        assert_eq!(
            context
                .frames
                .iter()
                .map(|frame| frame.funcidx)
                .collect::<Vec<_>>(),
            vec![Some(2), Some(1), Some(0)],
            "the type check traps before a callee frame exists"
        );
        assert_innermost_pc_in_range(&context, &store);
    }

    fn reenter_guest_then_trap(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
        let target = ctx.instance().funcs[2];
        let (locals_size, entry_pc) = {
            let function = ctx.gc.get_func(target);
            (
                function.locals().byte_size(),
                function
                    .code_pointer()
                    .expect("test re-entry target must be wasm"),
            )
        };
        let local_reference = match ctx.stack.function_call(
            0,
            locals_size,
            target,
            ctx.local_reference,
            HOST_REENTER_RETURN.as_ptr(),
            ctx.gc,
        ) {
            VMResult::Success(local_reference) => local_reference,
            VMResult::Unreachable => return VMResult::Unreachable,
            VMResult::StackOverflow => return VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => return VMResult::MemoryIndexOutOfRange,
            VMResult::UnalignedAtomic => return VMResult::UnalignedAtomic,
            VMResult::TableIndexOutOfRange => return VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => return VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => return VMResult::TableUninitialized,
            VMResult::Unlinkable => return VMResult::Unlinkable,
            VMResult::MemoryAllocationFailed => return VMResult::MemoryAllocationFailed,
            VMResult::InvalidOperand => return VMResult::InvalidOperand,
            VMResult::Unimplemented => return VMResult::Unimplemented,
            VMResult::FuelExhausted => return VMResult::FuelExhausted,
            VMResult::Cancelled => return VMResult::Cancelled,
        };
        ctx.set_local_reference(local_reference);
        VMResult::Success(entry_pc)
    }

    fn async_host_unreachable(_ctx: &mut ExecuteContext<'_>) -> crate::common::AsyncHostFuture {
        Box::pin(async { VMResult::Unreachable })
    }

    #[tokio::test]
    async fn trap_context_retains_mid_chain_host_frame() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (func $host)
              (func $entry (export "run") call $host)
              (func $inner unreachable))
            "#,
        )
        .await;
        crate::runtime::instantiate::link_host_function_with_function_idx(
            &instance,
            0,
            reenter_guest_then_trap,
            &store,
        );

        let result =
            crate::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        assert!(matches!(result, VMResult::Unreachable));

        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::Unreachable));
        assert_eq!(context.task_id, 0);
        assert_eq!(context.frames[0].funcidx, Some(2));
        assert_eq!(context.frames[0].kind, CapturedFrameKind::Wasm);
        assert_eq!(context.frames[1].funcidx, Some(0));
        assert_eq!(context.frames[1].kind, CapturedFrameKind::Host);
        assert_eq!(context.frames[1].pc_index, None);
        assert_eq!(context.frames[2].funcidx, Some(1));
        assert_eq!(context.frames[2].kind, CapturedFrameKind::Wasm);
    }

    #[tokio::test]
    async fn async_host_trap_publishes_context_after_completion() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (func (export "run")))
            "#,
        )
        .await;
        crate::runtime::instantiate::link_async_host_function_with_function_idx(
            &instance,
            0,
            async_host_unreachable,
            &store,
        );

        let result =
            crate::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        assert!(matches!(result, VMResult::Unreachable));
        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::Unreachable));
        assert_eq!(context.task_id, 0);
        assert_eq!(context.frames[0].kind, CapturedFrameKind::AsyncHost);
        assert_eq!(context.frames[0].pc_index, None);
    }

    #[tokio::test]
    async fn trap_context_stack_overflow_keeps_innermost_and_outermost_frames() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (func $entry (export "run") call $recurse)
              (func $recurse call $recurse))
            "#,
        )
        .await;

        let result =
            crate::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        assert!(matches!(result, VMResult::StackOverflow));

        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::StackOverflow));
        assert_eq!(context.task_id, 0);
        assert!(context.truncated);
        assert!(context.total_frames > 64);
        assert_eq!(context.frames.len(), 64);
        assert_eq!(context.frames[0].funcidx, Some(1));
        assert_eq!(context.frames[47].funcidx, Some(1));
        assert_eq!(context.frames[48].funcidx, Some(1));
        assert_eq!(context.frames.last().unwrap().funcidx, Some(0));
    }

    #[tokio::test]
    async fn trap_context_clears_after_success_and_post_lock_unlinkable() {
        let (store, instance) = instantiate_wat(
            r#"
            (module
              (func (export "trap") unreachable)
              (func (export "ok")))
            "#,
        )
        .await;

        let trapped =
            crate::run_module_function(&instance, &store, "trap", &ResultValue::new(vec![])).await;
        assert!(matches!(trapped, VMResult::Unreachable));
        let succeeded =
            crate::run_module_function(&instance, &store, "ok", &ResultValue::new(vec![])).await;
        assert!(matches!(succeeded, VMResult::Success(_)));
        assert!(store.lock_runtime_or_panic().take_last_trap().is_none());

        let trapped =
            crate::run_module_function(&instance, &store, "trap", &ResultValue::new(vec![])).await;
        assert!(matches!(trapped, VMResult::Unreachable));
        let missing =
            crate::run_module_function(&instance, &store, "missing", &ResultValue::new(vec![]))
                .await;
        assert!(matches!(missing, VMResult::Unlinkable));
        assert!(store.lock_runtime_or_panic().take_last_trap().is_none());
    }

    #[tokio::test]
    async fn start_function_trap_publishes_context() {
        let store = Store::new();
        let registry = Registry::new();
        let result = crate::instantiate(
            parse_wat(
                r#"
                (module
                  (func $start unreachable)
                  (start $start))
                "#,
            ),
            &store,
            &registry,
        )
        .await;
        assert!(matches!(result, VMResult::Unreachable));
        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::Unreachable));
        assert_eq!(context.task_id, 0);
    }

    #[cfg(feature = "jit")]
    #[tokio::test]
    async fn jit_trap_omits_unjustified_pc() {
        if !crate::jit_supported() {
            return;
        }
        let mut runtime_config = crate::RuntimeConfig::default();
        runtime_config.jit.enabled = true;
        let store = Store::new_with_runtime_config(runtime_config);
        let registry = Registry::new();
        let instance = match crate::instantiate(
            parse_wat(
                r#"
                (module
                  (func (export "run") unreachable))
                "#,
            ),
            &store,
            &registry,
        )
        .await
        {
            VMResult::Success(instance) => instance,
            other => panic!("JIT test module must instantiate: {other:?}"),
        };

        let result =
            crate::run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
        assert!(matches!(result, VMResult::Unreachable));
        let context = take_context(&store);
        assert!(matches!(&context.result, VMResult::Unreachable));
        assert_eq!(context.frames[0].pc_index, None);
    }
}
