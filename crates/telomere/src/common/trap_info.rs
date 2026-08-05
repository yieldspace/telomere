//! Public, owned diagnostics for a guest trap.

use super::{store::StoreInner, VMResult};
use crate::runtime::trap_context::{CapturedFrameKind, TrapContext};
use std::fmt;

/// An owned diagnostic record captured when guest execution traps.
///
/// The record owns its frame and resolved-name data, so it can outlive the
/// [`crate::Store`] that produced it. Its labels are Telomere diagnostics, not
/// WebAssembly spec-test trap-message strings.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrapInfo {
    /// The runtime failure that ended guest execution.
    pub kind: TrapKind,
    /// Captured frames, innermost first.
    ///
    /// When [`Self::truncated`] is true, this contains the innermost 48 frames
    /// followed by the outermost 16 frames.
    pub frames: Vec<TrapFrame>,
    /// Frames the capture walked, including frames not present in [`Self::frames`].
    pub total_frames: u32,
    /// Whether the bounded frame walk or the retained frame list was truncated.
    pub truncated: bool,
}

impl TrapInfo {
    pub(crate) fn from_context(store: &StoreInner, context: &TrapContext) -> Option<Self> {
        // Scheduler task identity remains crate-private diagnostic metadata.
        let _ = context.task_id;
        let kind = TrapKind::from_vm_result(&context.result)?;
        let frames = context
            .frames
            .iter()
            .map(|frame| {
                let (func_name, module_name) = symbolize_frame(store, frame);
                TrapFrame {
                    depth: frame.depth,
                    funcidx: frame.funcidx,
                    func_name,
                    module_name,
                    pc_index: frame.pc_index,
                    kind: match frame.kind {
                        CapturedFrameKind::Wasm => TrapFrameKind::Wasm,
                        CapturedFrameKind::Host => TrapFrameKind::Host,
                        CapturedFrameKind::AsyncHost => TrapFrameKind::AsyncHost,
                        CapturedFrameKind::Unresolved => TrapFrameKind::Unresolved,
                    },
                }
            })
            .collect();

        Some(Self {
            kind,
            frames,
            total_frames: context.total_frames,
            truncated: context.truncated,
        })
    }
}

fn symbolize_frame(
    store: &StoreInner,
    frame: &crate::runtime::trap_context::CapturedFrame,
) -> (Option<String>, Option<String>) {
    if frame.funcidx.is_none() {
        return (None, None);
    }

    let Some(function) = store.try_get_func(frame.code_addr) else {
        return (None, None);
    };
    let Some(instance) = store.try_instance(function.instance) else {
        return (None, None);
    };
    let Some(module) = store.try_get_module(instance.module_addr) else {
        return (None, None);
    };
    let Some(names) = module.names.as_deref() else {
        return (None, None);
    };

    (
        names.function_name(function.funcidx).map(str::to_owned),
        names.module_name().map(str::to_owned),
    )
}

/// One captured function or host-call frame in a [`TrapInfo`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrapFrame {
    /// Distance from the trapping frame; zero identifies the frame that trapped.
    pub depth: u32,
    /// Core module function index, or `None` when the frame could not resolve to a function.
    pub funcidx: Option<u32>,
    /// Producer-supplied function name retained from a WebAssembly `name` section.
    pub func_name: Option<String>,
    /// Producer-supplied module name retained from a WebAssembly `name` section.
    pub module_name: Option<String>,
    /// Index of an instruction in the frame's decoded body, when justified.
    ///
    /// Frame zero identifies the faulting instruction; older frames identify
    /// return addresses, the instruction after their call sites. A fused
    /// superinstruction reports its first instruction, native JIT traps report
    /// `None` when no interpreter attribution is justified, and tail calls
    /// elide frames by design.
    pub pc_index: Option<u32>,
    /// The kind of runtime frame that was captured.
    pub kind: TrapFrameKind,
}

/// The runtime category of a [`TrapFrame`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapFrameKind {
    /// A decoded or compiled WebAssembly function frame.
    Wasm,
    /// A synchronous host-function frame.
    Host,
    /// An asynchronous host-function frame.
    AsyncHost,
    /// A frame whose function record could not be resolved safely.
    Unresolved,
}

/// A Telomere diagnostic label for a guest trap.
///
/// These labels are Telomere diagnostics, not WebAssembly spec-test
/// trap-message strings.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapKind {
    /// Guest execution reached an `unreachable` instruction.
    Unreachable,
    /// Guest execution exhausted the interpreter stack capacity.
    StackOverflow,
    /// A guest memory instruction referenced a missing memory.
    MemoryIndexOutOfRange,
    /// A guest atomic instruction used an address with insufficient alignment.
    UnalignedAtomic,
    /// A guest table instruction referenced a missing table.
    TableIndexOutOfRange,
    /// An indirect call's runtime type did not match its expected type.
    CallIndirectInvalidType,
    /// An indirect call selected an uninitialized table entry.
    TableUninitialized,
    /// Imports, exports, or initialization could not be linked into an instance.
    Unlinkable,
    /// The runtime could not reserve or grow guest memory.
    MemoryAllocationFailed,
    /// A guest instruction received an invalid dynamic operand.
    InvalidOperand,
    /// The runtime does not implement this execution path yet.
    Unimplemented,
    /// Finite Store fuel was exhausted at a metering checkpoint.
    FuelExhausted,
    /// A watchdog requested Store execution cancellation at a metering checkpoint.
    Cancelled,
}

impl TrapKind {
    /// Returns this diagnostic label in its stable display form.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::StackOverflow => "stack overflow",
            Self::MemoryIndexOutOfRange => "memory index out of range",
            Self::UnalignedAtomic => "unaligned atomic",
            Self::TableIndexOutOfRange => "table index out of range",
            Self::CallIndirectInvalidType => "call indirect invalid type",
            Self::TableUninitialized => "table uninitialized",
            Self::Unlinkable => "unlinkable",
            Self::MemoryAllocationFailed => "memory allocation failed",
            Self::InvalidOperand => "invalid operand",
            Self::Unimplemented => "unimplemented",
            Self::FuelExhausted => "fuel exhausted",
            Self::Cancelled => "cancelled",
        }
    }

    fn from_vm_result(result: &VMResult<()>) -> Option<Self> {
        match result {
            VMResult::Success(()) => None,
            VMResult::Unreachable => Some(Self::Unreachable),
            VMResult::StackOverflow => Some(Self::StackOverflow),
            VMResult::MemoryIndexOutOfRange => Some(Self::MemoryIndexOutOfRange),
            VMResult::UnalignedAtomic => Some(Self::UnalignedAtomic),
            VMResult::TableIndexOutOfRange => Some(Self::TableIndexOutOfRange),
            VMResult::CallIndirectInvalidType => Some(Self::CallIndirectInvalidType),
            VMResult::TableUninitialized => Some(Self::TableUninitialized),
            VMResult::Unlinkable => Some(Self::Unlinkable),
            VMResult::MemoryAllocationFailed => Some(Self::MemoryAllocationFailed),
            VMResult::InvalidOperand => Some(Self::InvalidOperand),
            VMResult::Unimplemented => Some(Self::Unimplemented),
            VMResult::FuelExhausted => Some(Self::FuelExhausted),
            VMResult::Cancelled => Some(Self::Cancelled),
        }
    }
}

impl fmt::Display for TrapInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "trap: {}", self.kind.as_str())?;

        let mut previous_depth = None;
        let mut emitted_elision = false;
        for frame in &self.frames {
            if let Some(previous_depth) = previous_depth {
                let gap = frame
                    .depth
                    .checked_sub(previous_depth)
                    .and_then(|distance| distance.checked_sub(1));
                if let Some(gap) = gap.filter(|gap| *gap > 0) {
                    write!(f, "\n  ... {gap} frames elided ...")?;
                    emitted_elision = true;
                }
            }

            write!(f, "\n  {}: ", frame.depth)?;
            if frame.kind == TrapFrameKind::Unresolved {
                f.write_str("<unknown>")?;
            } else if let Some(func_name) = &frame.func_name {
                if let Some(module_name) =
                    frame.module_name.as_deref().filter(|name| !name.is_empty())
                {
                    write!(f, "{module_name}::{func_name}")?;
                } else {
                    f.write_str(func_name)?;
                }
            } else {
                f.write_str("<unnamed>")?;
            }

            match frame.funcidx {
                Some(funcidx) => write!(f, " (func {funcidx})")?,
                None => f.write_str(" (func ?)")?,
            }
            if let Some(pc_index) = frame.pc_index {
                write!(f, " @ pc {pc_index}")?;
            }
            match frame.kind {
                TrapFrameKind::Wasm => {}
                TrapFrameKind::Host => f.write_str(" [host]")?,
                TrapFrameKind::AsyncHost => f.write_str(" [async host]")?,
                TrapFrameKind::Unresolved => f.write_str(" [unresolved]")?,
            }

            previous_depth = Some(frame.depth);
        }

        if self.truncated && !self.frames.is_empty() && !emitted_elision {
            f.write_str("\n  ... frame walk truncated ...")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        common::{
            store::{FunctionBody, FunctionInstanceData, InstanceData, InstanceId},
            ExecuteContext, Instr, ObjectRef,
        },
        runtime::trap_context::{CapturedFrame, CapturedFrameKind},
    };

    fn frame(depth: u32) -> TrapFrame {
        TrapFrame {
            depth,
            funcidx: Some(depth),
            func_name: None,
            module_name: None,
            pc_index: None,
            kind: TrapFrameKind::Wasm,
        }
    }

    #[test]
    fn display_elides_noncontiguous_depths() {
        let mut innermost = frame(0);
        innermost.funcidx = Some(3);
        innermost.func_name = Some("inner".to_owned());
        innermost.module_name = Some("demo".to_owned());
        innermost.pc_index = Some(0);
        let mut outermost = frame(4);
        outermost.funcidx = Some(0);
        outermost.kind = TrapFrameKind::Host;

        let info = TrapInfo {
            kind: TrapKind::Unreachable,
            frames: vec![innermost, outermost],
            total_frames: 5,
            truncated: true,
        };

        assert_eq!(
            info.to_string(),
            "trap: unreachable\n  0: demo::inner (func 3) @ pc 0\n  ... 3 frames elided ...\n  4: <unnamed> (func 0) [host]"
        );
    }

    #[test]
    fn display_marks_walk_truncation_without_elision() {
        let info = TrapInfo {
            kind: TrapKind::StackOverflow,
            frames: vec![frame(0)],
            total_frames: 1,
            truncated: true,
        };

        assert_eq!(
            info.to_string(),
            "trap: stack overflow\n  0: <unnamed> (func 0)\n  ... frame walk truncated ..."
        );
    }

    #[test]
    fn display_is_total_for_nonmonotonic_depths() {
        let info = TrapInfo {
            kind: TrapKind::Unreachable,
            frames: vec![frame(2), frame(1)],
            total_frames: 0,
            truncated: true,
        };

        let rendered = std::panic::catch_unwind(|| info.to_string())
            .expect("formatting a mutated TrapInfo must not panic");
        assert_eq!(
            rendered,
            "trap: unreachable\n  2: <unnamed> (func 2)\n  1: <unnamed> (func 1)\n  ... frame walk truncated ..."
        );
    }

    fn unused_host(_ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
        VMResult::Unreachable
    }

    #[test]
    fn symbolization_ignores_invalid_store_links() {
        let mut store = StoreInner::new();
        let invalid_module_instance = store.alloc_instance(InstanceData {
            instance_id: 0,
            module_addr: ObjectRef(0),
            globals: Vec::new(),
            funcs: Vec::new(),
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });
        let invalid_instance_code_addr = store.new_func(&FunctionInstanceData {
            instance: InstanceId::from_raw(2),
            funcidx: 7,
            body: FunctionBody::Host(unused_host),
        });
        let invalid_module_code_addr = store.new_func(&FunctionInstanceData {
            instance: invalid_module_instance,
            funcidx: 8,
            body: FunctionBody::Host(unused_host),
        });
        let context = TrapContext {
            result: VMResult::Unreachable,
            task_id: 0,
            frames: vec![
                CapturedFrame {
                    depth: 0,
                    code_addr: invalid_instance_code_addr,
                    funcidx: Some(7),
                    pc_index: None,
                    kind: CapturedFrameKind::Host,
                },
                CapturedFrame {
                    depth: 1,
                    code_addr: invalid_module_code_addr,
                    funcidx: Some(8),
                    pc_index: None,
                    kind: CapturedFrameKind::Host,
                },
            ],
            total_frames: 2,
            truncated: false,
        };

        let info = TrapInfo::from_context(&store, &context)
            .expect("a trapping context must become TrapInfo");
        assert_eq!(
            info.frames
                .iter()
                .map(|frame| frame.funcidx)
                .collect::<Vec<_>>(),
            vec![Some(7), Some(8)]
        );
        assert!(info.frames.iter().all(|frame| frame.func_name.is_none()));
        assert!(info.frames.iter().all(|frame| frame.module_name.is_none()));
    }
}
