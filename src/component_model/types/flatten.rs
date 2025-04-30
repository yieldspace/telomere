/// canonical-abiのdefinitions.pyの関数のRustでの再実装です．
/// 元の関数名と異なる場合はdocに元の名前を記述しています．
use crate::common::{ResultType, ValType as CoreValType};
use crate::component_model::{
    CanonicalOptions, Case, CoreFuncType, DefValType, FuncType, Label, LabelValType, PrimValType,
    Type, ValType,
};

#[derive(Debug, Clone, Copy)]
pub enum FlatType {
    Lift,
    Lower,
}

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

pub trait Flattenable {
    fn flat(&self, opt: &CanonicalOptions, flat_type: FlatType) -> Vec<CoreValType>;
}

impl Flattenable for Type {
    fn flat(&self, opt: &CanonicalOptions, flat_type: FlatType) -> Vec<CoreValType> {
        match self {
            Type::DefVal(defval) => defval.flat(opt, flat_type),
            _ => panic!("cannot pass into canonical function directly"),
        }
    }
}

impl Flattenable for DefValType {
    fn flat(&self, opt: &CanonicalOptions, flat_type: FlatType) -> Vec<CoreValType> {
        match self {
            DefValType::Primitive(prim) => prim.flat(opt, flat_type),
            DefValType::Record(labels) => labels
                .into_iter()
                .map(|x| x.flat(opt, flat_type))
                .flatten()
                .collect::<Vec<_>>(),
            DefValType::Variant(cases) => {
                let mut flat: Vec<CoreValType> = vec![];
                cases.iter().for_each(|case| {
                    if let Some(t) = &case.t {
                        t.flat(opt, flat_type)
                            .into_iter()
                            .enumerate()
                            .for_each(|(i, x)| {
                                if i < flat.len() {
                                    let nth = flat.get(i).unwrap().clone();
                                    flat[i] = join_flat_type(nth, x);
                                } else {
                                    flat.push(x);
                                }
                            });
                    }
                });
                discriminant_type(cases)
                    .flat(opt, flat_type)
                    .into_iter()
                    .chain(flat)
                    .collect()
            }
            DefValType::List(ty, size) => {
                if let Some(size) = size {
                    ty.flat(opt, flat_type).repeat(*size)
                } else {
                    vec![CoreValType::I32, CoreValType::I32]
                }
            }
            DefValType::Tuple(items) => items
                .iter()
                .map(|x| x.flat(opt, flat_type))
                .flatten()
                .collect(),
            DefValType::Flags(_) => vec![CoreValType::I32],
            DefValType::Enum(labels) => DefValType::Variant(
                labels
                    .iter()
                    .map(|label| Case::new(label.clone(), None))
                    .collect(),
            )
            .flat(opt, flat_type),
            DefValType::Option(ty) => DefValType::Variant(vec![
                Case::new(Label::new("none"), None),
                Case::new(Label::new("some"), Some(ty.clone())),
            ])
            .flat(opt, flat_type),
            DefValType::Result(ok, err) => DefValType::Variant(vec![
                Case::new(Label::new("ok"), ok.clone()),
                Case::new(Label::new("error"), err.clone()),
            ])
            .flat(opt, flat_type),
            DefValType::Own(_) => vec![CoreValType::I32],
            DefValType::Borrow(_) => vec![CoreValType::I32],
        }
    }
}

impl Flattenable for LabelValType {
    fn flat(&self, opt: &CanonicalOptions, flat_type: FlatType) -> Vec<CoreValType> {
        self.t.flat(opt, flat_type)
    }
}

impl Flattenable for ValType {
    fn flat(&self, opt: &CanonicalOptions, flat_type: FlatType) -> Vec<CoreValType> {
        match self {
            ValType::Type(ty) => ty.flat(opt, flat_type),
            ValType::Primitive(prim) => prim.flat(opt, flat_type),
        }
    }
}

impl Flattenable for PrimValType {
    fn flat(&self, _opt: &CanonicalOptions, _flat_type: FlatType) -> Vec<CoreValType> {
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

const MAX_FLAT_PARAMS: usize = 16;
const MAX_FLAT_RESULTS: usize = 1;

impl FuncType {
    pub(crate) fn flat(&self, opt: &CanonicalOptions, flat_type: FlatType) -> CoreFuncType {
        let mut flat_params = self
            .params
            .iter()
            .map(|param| param.flat(opt, flat_type))
            .flatten()
            .collect::<Vec<_>>();
        let mut flat_results = self
            .result
            .as_ref()
            .map(|x| x.flat(opt, flat_type))
            .unwrap_or_default();
        if opt.is_sync() {
            if flat_params.len() > MAX_FLAT_PARAMS {
                flat_params = vec![CoreValType::I32];
            }
            if flat_results.len() > MAX_FLAT_RESULTS {
                match flat_type {
                    FlatType::Lift => {
                        flat_results = vec![CoreValType::I32];
                    }
                    FlatType::Lower => {
                        flat_params.push(CoreValType::I32);
                        flat_results = vec![];
                    }
                }
            }
            CoreFuncType(ResultType(flat_params), ResultType(flat_results))
        } else {
            #[cfg(not(feature = "component-gated-feature-async"))]
            unreachable!();
            #[cfg(feature = "component-gated-feature-async")]
            todo!();
        }
    }
}
