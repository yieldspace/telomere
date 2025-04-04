use crate::binary::{BinaryReader, Countable, Counter};
use crate::component_model::{CanonOpt, CanonicalFuncKind};
use crate::parser::component::parser::core::{
    parse_core_func_id, parse_core_memory_id, parse_core_table_id,
};
use crate::parser::component::parser::id::{parse_func_idx, parse_type_idx};
use crate::parser::component::parser::types::parse_resultlist;
use crate::parser::component::parser::{parse_option, ComponentModelParserError};
use crate::parser::component::ParseContext;
use crate::parser::core::{parse_u32, parse_vec};

type Result<R> = std::result::Result<R, ComponentModelParserError>;

pub fn parse_canon(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(usize, CanonicalFuncKind)> {
    let mut counter = Counter::new();
    let kind = match ctx.reader.read_exact_one()?.count(&mut counter) {
        0x00 => {
            assert_eq!(ctx.reader.read_exact_one()?.count(&mut counter), 0x00);
            let func_id = parse_func_idx(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            let ft = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::CanonLift(func_id, opts, ft)
        }
        0x01 => {
            assert_eq!(ctx.reader.read_exact_one()?.count(&mut counter), 0x00);
            let f = parse_func_idx(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::CanonLower(f, opts)
        }
        0x02 => {
            let rt = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::ResourceNew(rt)
        }
        0x03 => {
            let rt = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::ResourceDrop(rt)
        }
        #[cfg(feature = "async")]
        0x07 => {
            let rt = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::ResourceDropAsync(rt)
        }
        0x04 => {
            let rt = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::ResourceRep(rt)
        }
        #[cfg(feature = "async")]
        0x08 => CanonicalFuncKind::BackPressureSet,
        #[cfg(feature = "async")]
        0x09 => {
            let rs = parse_resultlist(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::TaskReturn(rs, opts)
        }
        #[cfg(feature = "async")]
        0x0a => {
            assert_eq!(ctx.reader.read_exact_one()?.count(&mut counter), 0x7f);
            let i = parse_u32(ctx.reader)?.count(&mut counter);
            CanonicalFuncKind::ContextGet(i)
        }
        #[cfg(feature = "async")]
        0x0b => {
            assert_eq!(ctx.reader.read_exact_one()?.count(&mut counter), 0x7f);
            let i = parse_u32(ctx.reader)?.count(&mut counter);
            CanonicalFuncKind::ContextSet(i)
        }
        #[cfg(feature = "async")]
        0x0c => {
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            CanonicalFuncKind::YieldAsync(is_async)
        }
        #[cfg(feature = "async")]
        0x0d => CanonicalFuncKind::SubtaskDrop,
        #[cfg(feature = "async")]
        0x0e => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::StreamNew(t)
        }
        #[cfg(feature = "async")]
        0x0f => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::StreamRead(t, opts)
        }
        #[cfg(feature = "async")]
        0x10 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::StreamWrite(t, opts)
        }
        #[cfg(feature = "async")]
        0x11 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            CanonicalFuncKind::StreamCancelRead(t, is_async)
        }
        #[cfg(feature = "async")]
        0x12 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            CanonicalFuncKind::StreamCancelWrite(t, is_async)
        }
        #[cfg(feature = "async")]
        0x13 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::StreamCloseReadable(t)
        }
        #[cfg(feature = "async")]
        0x14 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::StreamCloseWritable(t)
        }
        #[cfg(feature = "async")]
        0x15 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::FutureNew(t)
        }
        #[cfg(feature = "async")]
        0x16 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::FutureRead(t, opts)
        }
        #[cfg(feature = "async")]
        0x17 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::FutureWrite(t, opts)
        }
        #[cfg(feature = "async")]
        0x18 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            CanonicalFuncKind::FutureCancelRead(t, is_async)
        }
        #[cfg(feature = "async")]
        0x19 => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            CanonicalFuncKind::FutureCancelWrite(t, is_async)
        }
        #[cfg(feature = "async")]
        0x1a => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::FutureCloseReadable(t)
        }
        #[cfg(feature = "async")]
        0x1b => {
            let t = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::FutureCloseWritable(t)
        }
        0x1c => {
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::ErrorContextNew(opts)
        }
        0x1d => {
            let opts = parse_vec(ctx, |v| v.reader, parse_canon_opt)?.count(&mut counter);
            CanonicalFuncKind::ErrorContextDebugMessage(opts)
        }
        0x1e => CanonicalFuncKind::ErrorContextDrop,
        0x1f => CanonicalFuncKind::WaitableSetNew,
        0x20 => {
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            let m = parse_core_memory_id(ctx)?.count(&mut counter);
            CanonicalFuncKind::WaitableSetWait(is_async, m)
        }
        0x21 => {
            let is_async = parse_option(ctx, parse_async)?.count(&mut counter);
            let m = parse_core_memory_id(ctx)?.count(&mut counter);
            CanonicalFuncKind::WaitableSetPoll(is_async, m)
        }
        0x22 => CanonicalFuncKind::WaitableSetDrop,
        0x23 => CanonicalFuncKind::WaitableJoin,
        0x40 => {
            let ft = parse_type_idx(ctx)?.count(&mut counter);
            CanonicalFuncKind::ThreadSpawnRef(ft)
        }
        0x41 => {
            let ft = parse_type_idx(ctx)?.count(&mut counter);
            let tbl = parse_core_table_id(ctx)?.count(&mut counter);
            CanonicalFuncKind::ThreadSpawnIndirect(ft, tbl)
        }
        0x42 => CanonicalFuncKind::ThreadAvailableParallelism,
        _ => todo!(),
    };
    Ok((counter.count(), kind))
}

fn parse_async(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, bool)> {
    let a = match ctx.reader.read_exact_one()? {
        0x00 => false,
        0x01 => true,
        _ => todo!(),
    };
    Ok((1, a))
}

fn parse_canon_opt(ctx: &mut ParseContext<impl BinaryReader>) -> Result<(usize, CanonOpt)> {
    let mut counter = Counter::new();
    let opt = match ctx.reader.read_exact_one()?.count(&mut counter) {
        0x00 => CanonOpt::StringEncodingUtf8,
        0x01 => CanonOpt::StringEncodingUtf16,
        0x02 => CanonOpt::StringEncodingLatin1Utf16,
        0x03 => CanonOpt::Memory(parse_core_memory_id(ctx)?.count(&mut counter)),
        0x04 => CanonOpt::Realloc(parse_core_func_id(ctx)?.count(&mut counter)),
        0x05 => CanonOpt::PostReturn(parse_core_func_id(ctx)?.count(&mut counter)),
        #[cfg(feature = "async")]
        0x06 => CanonOpt::Async,
        #[cfg(feature = "async")]
        0x07 => CanonOpt::Callback(parse_core_func_id(ctx)?.count(&mut counter)),
        #[cfg(feature = "async")]
        0x08 => CanonOpt::AlwaysTaskReturn,
        _ => todo!(),
    };
    Ok((counter.count(), opt))
}
