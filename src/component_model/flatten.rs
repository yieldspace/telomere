/// canonical-abiのdefinitions.pyの関数のRustでの再実装です．
/// 元の関数名と異なる場合はdocに元の名前を記述しています．
use crate::common::ValType as CoreValType;
use crate::component_model::{Case, DefValType, LabelValType, PrimValType, Type, ValType};

/// join
fn join_flat_type(l: CoreValType, r: CoreValType) -> CoreValType {
    match (l, r) {
        (l, r) if l == r => l,
        (CoreValType::I32, CoreValType::F32) => CoreValType::I32,
        (CoreValType::F32, CoreValType::I32) => CoreValType::I32,
        _ => CoreValType::I64,
    }
}

fn discriminant_type(cases: &Vec<Case>) -> PrimValType {
    assert!(0 < cases.len() && cases.len() < (1 << 32));
    match cases.len().ilog2().div_ceil(8) {
        0 | 1 => PrimValType::U8,
        2 => PrimValType::U16,
        3 => PrimValType::U32,
        _ => unreachable!(),
    }
}

fn flatten_variant(cases: &Vec<Case>) -> Vec<CoreValType> {
    let mut flat: Vec<CoreValType> = vec![];
    cases.iter().for_each(|case| {
        if let Some(t) = &case.t {
            t.flat().into_iter().enumerate().for_each(|(i, x)| {
                if i < flat.len() {
                    let nth = flat.get(i).unwrap().clone();
                    flat.insert(i, join_flat_type(nth, x));
                } else {
                    flat.push(x);
                }
            });
        }
    });
    discriminant_type(cases)
        .flat()
        .into_iter()
        .chain(flat)
        .collect()
}

fn flatten_list(ty: &ValType, size: Option<usize>) -> Vec<CoreValType> {
    if let Some(size) = size {
        ty.flat().repeat(size)
    } else {
        vec![CoreValType::I32, CoreValType::I32]
    }
}

pub trait Flattenable {
    fn flat(&self) -> Vec<CoreValType>;
}

impl Flattenable for Type {
    fn flat(&self) -> Vec<CoreValType> {
        match self {
            Type::DefVal(ty) => ty.flat(),
            Type::Func(ty) => todo!(),
            Type::Component(_) => todo!(),
            Type::Instance(_) => todo!(),
            Type::Resource(_) => todo!(),
            Type::UniqueResource(_) => todo!(),
        }
    }
}

impl Flattenable for DefValType {
    fn flat(&self) -> Vec<CoreValType> {
        match self {
            DefValType::Primitive(prim) => prim.flat(),
            DefValType::Record(labels) => labels
                .into_iter()
                .map(|x| x.flat())
                .flatten()
                .collect::<Vec<_>>(),
            DefValType::Variant(cases) => flatten_variant(cases),
            DefValType::List(ty, size) => flatten_list(ty, *size),
            DefValType::Tuple(_) => todo!(),
            DefValType::Flags(_) => todo!(),
            DefValType::Enum(_) => todo!(),
            DefValType::Option(_) => todo!(),
            DefValType::Result(_, _) => todo!(),
            DefValType::Own(_) => todo!(),
            DefValType::Borrow(_) => todo!(),
        }
    }
}

impl Flattenable for LabelValType {
    fn flat(&self) -> Vec<CoreValType> {
        self.t.flat()
    }
}

impl Flattenable for ValType {
    fn flat(&self) -> Vec<CoreValType> {
        match self {
            ValType::Type(ty) => ty.flat(),
            ValType::Primitive(prim) => prim.flat(),
        }
    }
}

impl Flattenable for PrimValType {
    fn flat(&self) -> Vec<CoreValType> {
        match self {
            PrimValType::Bool => vec![CoreValType::I32],
            PrimValType::S8 => vec![CoreValType::I32],
            PrimValType::U8 => vec![CoreValType::I32],
            PrimValType::S16 => vec![CoreValType::I32],
            PrimValType::U16 => vec![CoreValType::I32],
            PrimValType::S32 => vec![CoreValType::I32],
            PrimValType::U32 => vec![CoreValType::I32],
            PrimValType::S64 => vec![CoreValType::I64],
            PrimValType::U64 => vec![CoreValType::I64],
            PrimValType::F32 => vec![CoreValType::F32],
            PrimValType::F64 => vec![CoreValType::F64],
            PrimValType::Char => vec![CoreValType::I32],
            PrimValType::String => vec![CoreValType::I32, CoreValType::I32],
        }
    }
}
