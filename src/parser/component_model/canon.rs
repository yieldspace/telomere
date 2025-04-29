use crate::binary::BinaryReader;
#[cfg(feature = "component-gated-feature-async")]
use crate::component_model::CanonicalFuncKind;
use crate::component_model::{
    CanonOpt, CanonicalFuncType, CoreFunc, CoreFuncType, Func, FuncType, GlobalIdx, Relation,
    ResourceType,
};
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
use tracing::trace;

pub fn parse_canon(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<()> {
    let start_count = ctx.reader.read_count();
    match ctx.reader.read_exact_one()? {
        // canon lift
        0x00 => {
            trace!("parse canon lift");
            assert_eq!(ctx.reader.read_exact_one()?, 0x00);
            let func_idx = parse_core_func_idx(ctx)?;
            let func_global_idx = ctx.validator.get_global_core_func(func_idx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            let idx = parse_type_idx(ctx)?;
            let ty: FuncType = ctx.validator.get_type(idx)?.try_into()?;
            let value = Func::CanonLift {
                core_func_idx: func_global_idx,
                opts,
                ty: ty.clone(),
            };
            let idx = ctx.validator.add_func_type(ty)?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_func(global_idx, Relation::Defined(value));
            ctx.validator.register_global_func(idx, global_idx)?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_function,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         func_idx: idx.global(),
            //     },
            // });
        }
        // canon lower
        0x01 => {
            trace!("parse canon lower");
            assert_eq!(ctx.reader.read_exact_one()?, 0x00);
            let func_idx = parse_func_idx(ctx)?;
            let func_global_idx = ctx.validator.get_global_func(func_idx)?;
            let ft = ctx.validator.get_func_type(func_idx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            let idx = ctx
                .validator
                .add_core_func_type(CoreFuncType::canon_lower(ft))?;
            let value = CoreFunc::CanonLower(func_global_idx, opts);
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_func(global_idx, Relation::Defined(value));
            ctx.validator.register_global_core_func(idx, global_idx)?;
            // let idx = ctx
            //     .validator
            //     .state
            //     .add_core_func(Binding::Real(CoreFunc::CanonLower(f, opts)))?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_core_function,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         core_func_idx: idx.global(),
            //     },
            // });
        }
        0x02 => {
            trace!("parse canon resource new");
            let idx = parse_type_idx(ctx)?;
            let resource_type: ResourceType = ctx.validator.get_type(idx)?.try_into()?;
            let idx = ctx
                .validator
                .add_core_func_type(CoreFuncType::canon_resource_new(resource_type.clone()))?;
            let value = CoreFunc::ResourceNew(resource_type);
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_func(global_idx, Relation::Defined(value));
            ctx.validator.register_global_core_func(idx, global_idx)?;
            // let idx = ctx
            //     .validator
            //     .state
            //     .add_core_func(Binding::Real(CoreFunc::ResourceNew(rt)))?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_core_function,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         type_idx: idx.global(),
            //     },
            // });
        }
        0x03 => {
            trace!("parse canon resource drop");
            let idx = parse_type_idx(ctx)?;
            let resource_type: ResourceType = ctx.validator.get_type(idx)?.try_into()?;
            let idx = ctx
                .validator
                .add_core_func_type(CoreFuncType::canon_resource_drop(resource_type.clone()))?;
            let value = CoreFunc::ResourceDrop(resource_type);
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_func(global_idx, Relation::Defined(value));
            ctx.validator.register_global_core_func(idx, global_idx)?;
            // let idx = ctx
            //     .validator
            //     .state
            //     .add_core_func(Binding::Real(CoreFunc::ResourceDrop(rt)))?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_core_function,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         type_idx: idx.global(),
            //     },
            // });
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x07 => {
            let rt = parse_type_idx(ctx)?;
            CanonicalFuncKind::ResourceDropAsync(rt);
            todo!();
        }
        0x04 => {
            trace!("parse resource rep");
            let idx = parse_type_idx(ctx)?;
            let resource_type: ResourceType = ctx.validator.get_type(idx)?.try_into()?;
            let idx = ctx
                .validator
                .add_core_func_type(CoreFuncType::canon_resource_rep(resource_type.clone()))?;
            let value = CoreFunc::ResourceRep(resource_type);
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_func(global_idx, Relation::Defined(value));
            ctx.validator.register_global_core_func(idx, global_idx)?;
            // let idx = ctx
            //     .validator
            //     .state
            //     .add_core_func(Binding::Real(CoreFunc::ResourceRep(rt)))?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_core_function,
            // });
            // ctx.push_instr(InstantiateInstr {
            //     operand: InstantiateOperand {
            //         type_idx: idx.global(),
            //     },
            // });
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
            let t = parse_type_idx(ctx)?;
            CanonicalFuncKind::StreamNew(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x0f => {
            let t = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::StreamRead(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x10 => {
            let t = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::StreamWrite(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x11 => {
            let t = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::StreamCancelRead(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x12 => {
            let t = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::StreamCancelWrite(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x13 => {
            let t = parse_type_idx(ctx)?;
            CanonicalFuncKind::StreamCloseReadable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x14 => {
            let t = parse_type_idx(ctx)?;
            CanonicalFuncKind::StreamCloseWritable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x15 => {
            let t = parse_type_idx(ctx)?;
            CanonicalFuncKind::FutureNew(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x16 => {
            let t = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::FutureRead(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x17 => {
            let t = parse_type_idx(ctx)?;
            let (_, opts) = parse_vec(ctx, |v| v.reader, parse_canon_opt)?;
            CanonicalFuncKind::FutureWrite(t, opts);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x18 => {
            let t = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::FutureCancelRead(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x19 => {
            let t = parse_type_idx(ctx)?;
            let (_, is_async) = parse_option(ctx, parse_async)?;
            CanonicalFuncKind::FutureCancelWrite(t, is_async);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x1a => {
            let t = parse_type_idx(ctx)?;
            CanonicalFuncKind::FutureCloseReadable(t);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x1b => {
            let t = parse_type_idx(ctx)?;
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
            let m = parse_core_memory_idx(ctx)?;
            CanonicalFuncKind::WaitableSetWait(is_async, m);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x21 => {
            let (_, is_async) = parse_option(ctx, parse_async)?;
            let m = parse_core_memory_idx(ctx)?;
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
            let m = parse_type_idx(ctx)?;
            CanonicalFuncKind::ThreadSpawnRef(ft);
            todo!();
        }
        #[cfg(feature = "component-gated-feature-threading-builtins")]
        0x41 => {
            let ft = parse_type_idx(ctx)?;
            let tbl = parse_core_table_idx(ctx)?;
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
        0x03 => {
            let idx = parse_core_memory_idx(ctx)?;
            CanonOpt::Memory(ctx.validator.get_global_core_memory(idx)?)
        }
        0x04 => {
            let idx = parse_core_func_idx(ctx)?;
            CanonOpt::Realloc(ctx.validator.get_global_core_func(idx)?)
        }
        0x05 => {
            let idx = parse_core_func_idx(ctx)?;
            CanonOpt::PostReturn(ctx.validator.get_global_core_func(idx)?)
        }
        #[cfg(feature = "component-gated-feature-async")]
        0x06 => CanonOpt::Async,
        #[cfg(feature = "component-gated-feature-async")]
        0x07 => CanonOpt::Callback(parse_core_func_idx(ctx)?),
        #[cfg(feature = "component-gated-feature-async")]
        0x08 => CanonOpt::AlwaysTaskReturn,
        _ => todo!(),
    };
    Ok((ctx.reader.read_count() - start_count, opt))
}
