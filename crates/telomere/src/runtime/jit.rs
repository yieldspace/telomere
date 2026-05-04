#[cfg(feature = "jit")]
mod abi;
#[cfg(feature = "jit")]
mod backend;
#[cfg(feature = "jit")]
mod cache;
#[cfg(feature = "jit")]
mod code_memory;
#[cfg(feature = "jit")]
mod stubs;

#[cfg(feature = "jit")]
use crate::{
    common::{store::FunctionBody, ExecuteContext, VMResult},
    runtime::vm,
};

#[cfg(feature = "jit")]
pub use abi::JitNativeExit;
#[cfg(feature = "jit")]
pub(crate) use cache::StoreJitCache;

pub fn supported() -> bool {
    cfg!(all(
        feature = "jit",
        target_os = "macos",
        target_arch = "aarch64"
    ))
}

#[cfg(feature = "jit")]
pub(crate) unsafe fn enter_current_frame(ctx: &mut ExecuteContext<'_>) -> VMResult<()> {
    let code_base = ctx.code();
    let exit = vm_try!(unsafe { enter_current_frame_raw(ctx) });
    unsafe { handle_exit(exit, code_base, ctx) }
}

#[cfg(feature = "jit")]
pub(crate) unsafe fn enter_current_frame_from_jit_call(
    ctx: &mut ExecuteContext<'_>,
) -> JitNativeExit {
    let code_base = ctx.code();
    match unsafe { enter_current_frame_raw(ctx) } {
        VMResult::Success(exit) => unsafe { absolutize_fallback_index(exit, code_base) },
        other => JitNativeExit::trap(other),
    }
}

#[cfg(feature = "jit")]
unsafe fn absolutize_fallback_index(
    exit: JitNativeExit,
    code_base: *const crate::common::Instr,
) -> JitNativeExit {
    if exit.kind == JitNativeExit::FALLBACK_INDEX {
        unsafe { JitNativeExit::fallback_pc(code_base.add(exit.value as usize)) }
    } else {
        exit
    }
}

#[cfg(feature = "jit")]
unsafe fn enter_current_frame_raw(ctx: &mut ExecuteContext<'_>) -> VMResult<JitNativeExit> {
    if !backend::supported() || !ctx.store.runtime_config().jit.enabled {
        return VMResult::Unimplemented;
    }
    let funcaddr = ctx.current_frame.code_addr;
    let (compiled, code_base) = {
        let func = ctx.gc.get_func(funcaddr);
        let FunctionBody::Wasm { code, .. } = &func.body else {
            return VMResult::Unimplemented;
        };
        let max_bytes = ctx.store.runtime_config().jit.code_cache_max_bytes;
        (
            vm_try!(ctx
                .store
                .jit_cache()
                .get_or_compile(funcaddr, &func.body, max_bytes)),
            code.as_ptr(),
        )
    };
    VMResult::Success(unsafe {
        (compiled.entry())(
            ctx as *mut ExecuteContext<'_>,
            code_base,
            ctx.local_base_ptr,
        )
    })
}

#[cfg(feature = "jit")]
unsafe fn handle_exit(
    exit: JitNativeExit,
    code_base: *const crate::common::Instr,
    ctx: &mut ExecuteContext<'_>,
) -> VMResult<()> {
    match exit.kind {
        JitNativeExit::FALLBACK_INDEX => unsafe {
            vm::call_code(code_base.add(exit.value as usize), ctx)
        },
        JitNativeExit::FALLBACK_PTR => unsafe { vm::call_code(exit.value as *const _, ctx) },
        JitNativeExit::CONTINUE_PTR => unsafe { vm::call_code(exit.value as *const _, ctx) },
        JitNativeExit::DONE => VMResult::Success(()),
        JitNativeExit::TRAP => abi::vm_result_from_code(exit.value),
        JitNativeExit::KEEP_GOING => VMResult::InvalidOperand,
        _ => VMResult::InvalidOperand,
    }
}
