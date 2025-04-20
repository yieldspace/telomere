use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-async")]
use crate::component_model::CanonicalFuncKind;
use crate::component_model::{Binding, CanonOpt, ComponentFunction, CoreFunction, Idx};
#[cfg(feature = "component-gated-feature-threading-builtins")]
use crate::parser::component_model::parse_core_table_idx;
use crate::parser::component_model::{
    parse_core_func_idx, parse_core_memory_idx, parse_func_idx, parse_type_idx,
    ComponentParseError, ParseContext, SizedResult,
};
#[cfg(any(
    feature = "component-gated-feature-async",
    feature = "component-gated-feature-threading-builtins"
))]
use crate::parser::component_model::{parse_option, parse_resultlist, parse_u32};
use crate::parser::core::parse_vec;
use crate::runtime::component_model::instantiate::{
    instantiate_core_function, instantiate_function, InstantiateInstr, InstantiateOperand,
};

pub fn parse_canon(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    match ctx.reader.read_exact_one()? {
        // canon lift
        0x00 => {
            assert_eq!(ctx.reader.read_exact_one()?, 0x00);
            let (_, func_idx) = parse_core_func_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            let (_, ft) = parse_type_idx(ctx)?;
            let idx = ctx
                .validator
                .add_func(Binding::Real(ComponentFunction::CanonLift {
                    core_func_idx: func_idx,
                    opts,
                    ty: ft,
                }))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_function,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    func_idx: idx.global(),
                },
            });
        }
        // canon lower
        0x01 => {
            assert_eq!(ctx.reader.read_exact_one()?, 0x00);
            let (_, f) = parse_func_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            let idx = ctx
                .validator
                .add_core_func(Binding::Real(CoreFunction::CanonLower(f, opts)))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_core_function,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    core_func_idx: idx.global(),
                },
            });
        }
        0x02 => {
            let (_, rt) = parse_type_idx(ctx)?;
            let idx = ctx
                .validator
                .add_core_func(Binding::Real(CoreFunction::ResourceNew(rt)))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_core_function,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    type_idx: idx.global(),
                },
            });
        }
        0x03 => {
            let (_, rt) = parse_type_idx(ctx)?;
            let idx = ctx
                .validator
                .add_core_func(Binding::Real(CoreFunction::ResourceDrop(rt)))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_core_function,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    type_idx: idx.global(),
                },
            });
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x07 => {
            let (_, rt) = parse_type_idx(ctx)?;
            CanonicalFuncKind::ResourceDropAsync(rt);
            todo!();
        }
        0x04 => {
            let (_, rt) = parse_type_idx(ctx)?;
            let idx = ctx
                .validator
                .add_core_func(Binding::Real(CoreFunction::ResourceRep(rt)))?;
            ctx.push_instr(InstantiateInstr {
                op: instantiate_core_function,
            });
            ctx.push_instr(InstantiateInstr {
                operand: InstantiateOperand {
                    type_idx: idx.global(),
                },
            });
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x08 => {
            CanonicalFuncKind::BackPressureSet;
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x09 => {
            let (_, rs) = parse_resultlist(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::TaskReturn(rs, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0a => {
            assert_eq!(ctx.reader.read_exact_one()?, 0x7f);
            let (_, i) = parse_u32(ctx.reader)?;
            CanonicalFuncKind::ContextGet(i);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0b => {
            assert_eq!(ctx.reader.read_exact_one()?, 0x7f);
            let (_, i) = parse_u32(ctx.reader)?;
            CanonicalFuncKind::ContextSet(i);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0c => {
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::YieldAsync(is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0d => {
            CanonicalFuncKind::SubtaskDrop;
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0e => {
            let (_, t) = parse_type_idx(ctx)?;
            CanonicalFuncKind::StreamNew(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0f => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::StreamRead(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x10 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::StreamWrite(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x11 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::StreamCancelRead(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x12 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::StreamCancelWrite(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x13 => {
            let (_, t) = parse_type_idx(ctx)?;
            CanonicalFuncKind::StreamCloseReadable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x14 => {
            let (_, t) = parse_type_idx(ctx)?;
            CanonicalFuncKind::StreamCloseWritable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x15 => {
            let (_, t) = parse_type_idx(ctx)?;
            CanonicalFuncKind::FutureNew(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x16 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::FutureRead(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x17 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::FutureWrite(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x18 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::FutureCancelRead(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x19 => {
            let (_, t) = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::FutureCancelWrite(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x1a => {
            let (_, t) = parse_type_idx(ctx)?;
            CanonicalFuncKind::FutureCloseReadable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x1b => {
            let (_, t) = parse_type_idx(ctx)?;
            CanonicalFuncKind::FutureCloseWritable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-error-context-type")]
        0x1c => {
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::ErrorContextNew(opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-error-context-type")]
        0x1d => {
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::ErrorContextDebugMessage(opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-error-context-type")]
        0x1e => {
            CanonicalFuncKind::ErrorContextDrop;
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x1f => {
            CanonicalFuncKind::WaitableSetNew;
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x20 => {
            let (_, is_async) = parse_option(ctx, parse_async)?;
            let (_, m) = parse_core_memory_idx(ctx)?;
            CanonicalFuncKind::WaitableSetWait(is_async, m);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x21 => {
            let (_, is_async) = parse_option(ctx, parse_async)?;
            let (_, m) = parse_core_memory_idx(ctx)?;
            CanonicalFuncKind::WaitableSetPoll(is_async, m);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x22 => {
            CanonicalFuncKind::WaitableSetDrop;
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x23 => {
            CanonicalFuncKind::WaitableJoin;
            todo!();
        }
        #[cfg(feature = "component-gated-feature-threading-builtins")]
        0x40 => {
            let (_, ft) = parse_type_idx(ctx)?;
            CanonicalFuncKind::ThreadSpawnRef(ft);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-threading-builtins")]
        0x41 => {
            let (_, ft) = parse_type_idx(ctx)?;
            let (_, tbl) = parse_core_table_idx(ctx)?;
            CanonicalFuncKind::ThreadSpawnIndirect(ft, tbl);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-threading-builtins")]
        0x42 => {
            CanonicalFuncKind::ThreadAvailableParallelism;
            todo!();
        }
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "Invalid Canonical Function Op: {x}"
            )));
        }
    };

    Ok((ctx.reader.read_count() - start_count, ()))
}

fn parse_async(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<bool> {
    let a = match ctx.reader.read_exact_one()? {
        0x00 => false,
        0x01 => true,
        _ => todo!(),
    };
    Ok((1, a))
}

fn parse_canon_opt(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CanonOpt> {
    let start_count = ctx.reader.read_count();
    let opt = match ctx.reader.read_exact_one()? {
        0x00 => CanonOpt::StringEncodingUtf8,
        0x01 => CanonOpt::StringEncodingUtf16,
        0x02 => CanonOpt::StringEncodingLatin1Utf16,
        0x03 => CanonOpt::Memory(parse_core_memory_idx(ctx)?.1),
        0x04 => CanonOpt::Realloc(parse_core_func_idx(ctx)?.1),
        0x05 => CanonOpt::PostReturn(parse_core_func_idx(ctx)?.1),
        #[cfg(feature = "component-gated-feature-async")]
        0x06 => CanonOpt::Async,
        #[cfg(feature = "component-gated-feature-async")]
        0x07 => CanonOpt::Callback(parse_core_func_idx(ctx)?.1),
        #[cfg(feature = "component-gated-feature-async")]
        0x08 => CanonOpt::AlwaysTaskReturn,
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, opt))
}
