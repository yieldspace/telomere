use crate::ir::types::{DefValType, PrimValType, Type, ValType};
use crate::runtime::RuntimeInstance;
use crate::support::Store;
use crate::{ComponentError, ComponentInstance, ComponentProgram, ComponentValue};
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Clone)]
pub struct ComponentFunc {
    runtime: RuntimeInstance,
    program: Rc<ComponentProgram>,
    type_id: crate::ir::TypeId,
    name: String,
}

impl ComponentFunc {
    pub(crate) fn new(
        runtime: RuntimeInstance,
        program: Rc<ComponentProgram>,
        name: impl Into<String>,
        type_id: crate::ir::TypeId,
    ) -> Self {
        Self {
            runtime,
            program,
            type_id,
            name: name.into(),
        }
    }

    pub async fn call(
        &self,
        store: &mut Store,
        args: &[ComponentValue],
    ) -> Result<Vec<ComponentValue>, ComponentError> {
        self.runtime.call(store, &self.name, args).await
    }

    pub fn typed<P, R>(&self) -> Result<TypedComponentFunc<P, R>, ComponentError>
    where
        P: ComponentParams,
        R: ComponentReturn,
    {
        let func_type = match self.program.get_type(self.type_id) {
            Some(Type::Func(func_type)) => func_type,
            _ => {
                return Err(ComponentError::Link(format!(
                    "component export '{}' is not a function",
                    self.name
                )))
            }
        };
        P::matches_params(&func_type.params, &self.program)?;
        R::matches_result(func_type.result.as_ref(), &self.program)?;
        Ok(TypedComponentFunc {
            func: self.clone(),
            _marker: PhantomData,
        })
    }
}

#[derive(Clone)]
pub struct TypedComponentFunc<P, R> {
    func: ComponentFunc,
    _marker: PhantomData<fn(P) -> R>,
}

impl<P, R> TypedComponentFunc<P, R>
where
    P: ComponentParams,
    R: ComponentReturn,
{
    pub async fn call(&self, store: &mut Store, params: P) -> Result<R, ComponentError> {
        let results = self
            .func
            .call(store, &params.into_component_args()?)
            .await?;
        R::from_component_results(results)
    }
}

pub trait LowerComponent: Sized {
    fn lower_component(self) -> Result<ComponentValue, ComponentError>;

    #[doc(hidden)]
    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError>;
}

pub trait LiftComponent: Sized {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError>;

    #[doc(hidden)]
    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError>;
}

#[doc(hidden)]
pub trait ComponentParams: Sized {
    fn from_component_args(args: &[ComponentValue]) -> Result<Self, ComponentError>;
    fn into_component_args(self) -> Result<Vec<ComponentValue>, ComponentError>;
    fn matches_params(params: &[ValType], program: &ComponentProgram)
        -> Result<(), ComponentError>;
}

#[doc(hidden)]
pub trait ComponentReturn: Sized {
    fn from_component_results(results: Vec<ComponentValue>) -> Result<Self, ComponentError>;
    fn into_component_results(self) -> Result<Vec<ComponentValue>, ComponentError>;
    fn matches_result(
        result: Option<&ValType>,
        program: &ComponentProgram,
    ) -> Result<(), ComponentError>;
}

impl LowerComponent for ComponentValue {
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(self)
    }

    fn matches_type(_ty: &ValType, _program: &ComponentProgram) -> Result<(), ComponentError> {
        Ok(())
    }
}

impl LiftComponent for ComponentValue {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        Ok(value)
    }

    fn matches_type(_ty: &ValType, _program: &ComponentProgram) -> Result<(), ComponentError> {
        Ok(())
    }
}

macro_rules! impl_scalar_component {
    ($ty:ty, $lower_variant:ident, $matcher:ident) => {
        impl LowerComponent for $ty {
            fn lower_component(self) -> Result<ComponentValue, ComponentError> {
                Ok(ComponentValue::$lower_variant(self))
            }

            fn matches_type(
                ty: &ValType,
                program: &ComponentProgram,
            ) -> Result<(), ComponentError> {
                ensure_matches_type::<Self>(ty, program, TypeExpectation::$matcher)
            }
        }

        impl LiftComponent for $ty {
            fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
                match value {
                    ComponentValue::$lower_variant(v) => Ok(v),
                    other => Err(ComponentError::InvalidArgument(format!(
                        "expected {} result, got {other:?}",
                        stringify!($lower_variant)
                    ))),
                }
            }

            fn matches_type(
                ty: &ValType,
                program: &ComponentProgram,
            ) -> Result<(), ComponentError> {
                ensure_matches_type::<Self>(ty, program, TypeExpectation::$matcher)
            }
        }
    };
}

impl_scalar_component!(bool, Bool, Bool);
impl_scalar_component!(u8, U8, U8);
impl_scalar_component!(i8, S8, S8);
impl_scalar_component!(u16, U16, U16);
impl_scalar_component!(i16, S16, S16);
impl_scalar_component!(u32, U32, U32);
impl_scalar_component!(i32, I32, S32OrI32);
impl_scalar_component!(u64, U64, U64);
impl_scalar_component!(i64, I64, S64OrI64);
impl_scalar_component!(f32, F32, F32);
impl_scalar_component!(f64, F64, F64);
impl_scalar_component!(char, Char, Char);

impl LowerComponent for String {
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::String(self))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        ensure_matches_type::<Self>(ty, program, TypeExpectation::String)
    }
}

impl LiftComponent for String {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::String(v) => Ok(v),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected String result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        ensure_matches_type::<Self>(ty, program, TypeExpectation::String)
    }
}

impl LowerComponent for &str {
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::String(self.to_owned()))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        ensure_matches_type::<Self>(ty, program, TypeExpectation::String)
    }
}

impl<T> LowerComponent for Vec<T>
where
    T: LowerComponent,
{
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::List(
            self.into_iter()
                .map(T::lower_component)
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let elem = extract_list_element_type(ty, program)?;
        T::matches_type(elem, program)
    }
}

impl<T> LiftComponent for Vec<T>
where
    T: LiftComponent,
{
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::List(values) => values
                .into_iter()
                .map(T::lift_component)
                .collect::<Result<Vec<_>, _>>(),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected list result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let elem = extract_list_element_type(ty, program)?;
        T::matches_type(elem, program)
    }
}

impl<T> LowerComponent for Option<T>
where
    T: LowerComponent,
{
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::Option(match self {
            Some(value) => Some(Box::new(value.lower_component()?)),
            None => None,
        }))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let some = extract_option_payload_type(ty, program)?;
        T::matches_type(some, program)
    }
}

impl<T> LiftComponent for Option<T>
where
    T: LiftComponent,
{
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Option(value) => {
                value.map(|value| T::lift_component(*value)).transpose()
            }
            other => Err(ComponentError::InvalidArgument(format!(
                "expected option result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let some = extract_option_payload_type(ty, program)?;
        T::matches_type(some, program)
    }
}

impl<T, E> LowerComponent for Result<T, E>
where
    T: LowerComponent,
    E: LowerComponent,
{
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(match self {
            Ok(value) => ComponentValue::Result {
                ok: Some(Box::new(value.lower_component()?)),
                err: None,
            },
            Err(error) => ComponentValue::Result {
                ok: None,
                err: Some(Box::new(error.lower_component()?)),
            },
        })
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let (ok, err) = extract_result_payload_types(ty, program)?;
        T::matches_type(ok, program)?;
        E::matches_type(err, program)
    }
}

impl<T, E> LiftComponent for Result<T, E>
where
    T: LiftComponent,
    E: LiftComponent,
{
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Result {
                ok: Some(value),
                err: None,
            } => Ok(Ok(T::lift_component(*value)?)),
            ComponentValue::Result {
                ok: None,
                err: Some(error),
            } => Ok(Err(E::lift_component(*error)?)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected result value, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let (ok, err) = extract_result_payload_types(ty, program)?;
        T::matches_type(ok, program)?;
        E::matches_type(err, program)
    }
}

impl ComponentParams for () {
    fn from_component_args(args: &[ComponentValue]) -> Result<Self, ComponentError> {
        if args.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::InvalidArgument(format!(
                "expected 0 component arguments, got {}",
                args.len()
            )))
        }
    }

    fn into_component_args(self) -> Result<Vec<ComponentValue>, ComponentError> {
        Ok(Vec::new())
    }

    fn matches_params(
        params: &[ValType],
        _program: &ComponentProgram,
    ) -> Result<(), ComponentError> {
        if params.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::Link(format!(
                "typed function expects 0 params but component func has {}",
                params.len()
            )))
        }
    }
}

impl ComponentReturn for () {
    fn from_component_results(results: Vec<ComponentValue>) -> Result<Self, ComponentError> {
        if results.is_empty() {
            Ok(())
        } else {
            Err(ComponentError::InvalidArgument(format!(
                "expected no component results, got {}",
                results.len()
            )))
        }
    }

    fn into_component_results(self) -> Result<Vec<ComponentValue>, ComponentError> {
        Ok(Vec::new())
    }

    fn matches_result(
        result: Option<&ValType>,
        _program: &ComponentProgram,
    ) -> Result<(), ComponentError> {
        if result.is_none() {
            Ok(())
        } else {
            Err(ComponentError::Link(
                "typed function expects no result".to_owned(),
            ))
        }
    }
}

impl<T> ComponentReturn for T
where
    T: LowerComponent + LiftComponent,
{
    fn from_component_results(results: Vec<ComponentValue>) -> Result<Self, ComponentError> {
        match results.as_slice() {
            [value] => T::lift_component(value.clone()),
            _ => Err(ComponentError::InvalidArgument(format!(
                "expected 1 component result, got {}",
                results.len()
            ))),
        }
    }

    fn into_component_results(self) -> Result<Vec<ComponentValue>, ComponentError> {
        Ok(vec![self.lower_component()?])
    }

    fn matches_result(
        result: Option<&ValType>,
        program: &ComponentProgram,
    ) -> Result<(), ComponentError> {
        let Some(result) = result else {
            return Err(ComponentError::Link(
                "typed function expects one result".to_owned(),
            ));
        };
        <T as LowerComponent>::matches_type(result, program)
    }
}

macro_rules! impl_component_tuple_value {
    ($($arity:expr => ($(($ty:ident, $value:ident)),+)),+ $(,)?) => {
        $(
            impl<$($ty),+> LowerComponent for ($($ty,)+)
            where
                $($ty: LowerComponent),+
            {
                fn lower_component(self) -> Result<ComponentValue, ComponentError> {
                    let ($($value,)+) = self;
                    Ok(ComponentValue::Tuple(vec![$($value.lower_component()?,)+]))
                }

                fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
                    let fields = extract_tuple_types(ty, program, $arity)?;
                    let mut iter = fields.iter();
                    $(
                        $ty::matches_type(iter.next().unwrap(), program)?;
                    )+
                    Ok(())
                }
            }

            impl<$($ty),+> LiftComponent for ($($ty,)+)
            where
                $($ty: LowerComponent + LiftComponent),+
            {
                fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
                    let values = match value {
                        ComponentValue::Tuple(values) => values,
                        other => {
                            return Err(ComponentError::InvalidArgument(format!(
                                "expected tuple result, got {other:?}"
                            )))
                        }
                    };
                    let mut iter = values.into_iter();
                    Ok(($(
                        $ty::lift_component(iter.next().ok_or_else(|| {
                            ComponentError::InvalidArgument("tuple result arity mismatch".to_owned())
                        })?)?,
                    )+))
                }

                fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
                    <($($ty,)+) as LowerComponent>::matches_type(ty, program)
                }
            }
        )+
    };
}

macro_rules! impl_component_params_tuple {
    ($($arity:expr => ($(($ty:ident, $value:ident)),+)),+ $(,)?) => {
        $(
            impl<$($ty),+> ComponentParams for ($($ty,)+)
            where
                $($ty: LowerComponent + LiftComponent),+
            {
                fn from_component_args(args: &[ComponentValue]) -> Result<Self, ComponentError> {
                    let mut iter = args.iter();
                    Ok(($(
                        $ty::lift_component(iter.next().ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "expected {} component arguments, got {}",
                                $arity,
                                args.len()
                            ))
                        })?.clone())?,
                    )+))
                }

                fn into_component_args(self) -> Result<Vec<ComponentValue>, ComponentError> {
                    let ($($value,)+) = self;
                    Ok(vec![$($value.lower_component()?,)+])
                }

                fn matches_params(params: &[ValType], program: &ComponentProgram) -> Result<(), ComponentError> {
                    if params.len() != $arity {
                        return Err(ComponentError::Link(format!(
                            "typed function expects {} params but component func has {}",
                            $arity,
                            params.len()
                        )));
                    }
                    let mut iter = params.iter();
                    $(
                        <$ty as LowerComponent>::matches_type(iter.next().unwrap(), program)?;
                    )+
                    Ok(())
                }
            }
        )+
    };
}

impl_component_tuple_value!(
    1 => ((T0, v0)),
    2 => ((T0, v0), (T1, v1)),
    3 => ((T0, v0), (T1, v1), (T2, v2)),
    4 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3)),
    5 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4)),
    6 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4), (T5, v5)),
    7 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4), (T5, v5), (T6, v6)),
    8 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4), (T5, v5), (T6, v6), (T7, v7))
);
impl_component_params_tuple!(
    1 => ((T0, v0)),
    2 => ((T0, v0), (T1, v1)),
    3 => ((T0, v0), (T1, v1), (T2, v2)),
    4 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3)),
    5 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4)),
    6 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4), (T5, v5)),
    7 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4), (T5, v5), (T6, v6)),
    8 => ((T0, v0), (T1, v1), (T2, v2), (T3, v3), (T4, v4), (T5, v5), (T6, v6), (T7, v7))
);

#[derive(Clone, Copy)]
enum TypeExpectation {
    Bool,
    U8,
    S8,
    U16,
    S16,
    U32,
    S32OrI32,
    U64,
    S64OrI64,
    F32,
    F64,
    Char,
    String,
}

fn ensure_matches_type<T>(
    ty: &ValType,
    program: &ComponentProgram,
    expected: TypeExpectation,
) -> Result<(), ComponentError> {
    if type_matches(ty, program, expected) {
        Ok(())
    } else {
        Err(ComponentError::Link(format!(
            "typed component binding does not match {}",
            std::any::type_name::<T>()
        )))
    }
}

fn type_matches(ty: &ValType, program: &ComponentProgram, expected: TypeExpectation) -> bool {
    match ty {
        ValType::Primitive(prim) => primitive_matches(prim, expected),
        ValType::Type(type_id) => match program.get_type(*type_id) {
            Some(Type::DefVal(DefValType::Primitive(prim))) => primitive_matches(prim, expected),
            _ => false,
        },
    }
}

fn primitive_matches(prim: &PrimValType, expected: TypeExpectation) -> bool {
    matches!(
        (prim, expected),
        (PrimValType::Bool, TypeExpectation::Bool)
            | (PrimValType::U8, TypeExpectation::U8)
            | (PrimValType::S8, TypeExpectation::S8)
            | (PrimValType::U16, TypeExpectation::U16)
            | (PrimValType::S16, TypeExpectation::S16)
            | (PrimValType::U32, TypeExpectation::U32)
            | (PrimValType::S32, TypeExpectation::S32OrI32)
            | (PrimValType::S64, TypeExpectation::S64OrI64)
            | (PrimValType::U64, TypeExpectation::U64)
            | (PrimValType::F32, TypeExpectation::F32)
            | (PrimValType::F64, TypeExpectation::F64)
            | (PrimValType::Char, TypeExpectation::Char)
            | (PrimValType::String, TypeExpectation::String)
    )
}

fn resolve_defined_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<&'a Type, ComponentError> {
    match ty {
        ValType::Type(type_id) => program
            .get_type(*type_id)
            .ok_or_else(|| ComponentError::Link("type id not found".to_owned())),
        ValType::Primitive(_) => Err(ComponentError::Link(
            "expected defined component type".to_owned(),
        )),
    }
}

fn extract_list_element_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<&'a ValType, ComponentError> {
    match resolve_defined_type(ty, program)? {
        Type::DefVal(DefValType::List(elem, _)) => Ok(elem),
        _ => Err(ComponentError::Link(
            "typed component binding expects list".to_owned(),
        )),
    }
}

fn extract_tuple_types<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
    expected_len: usize,
) -> Result<Vec<&'a ValType>, ComponentError> {
    let Type::DefVal(DefValType::Record(fields)) = resolve_defined_type(ty, program)? else {
        return Err(ComponentError::Link(
            "typed component binding expects tuple".to_owned(),
        ));
    };
    if fields.len() != expected_len
        || fields
            .iter()
            .enumerate()
            .any(|(index, field)| field.label.0 != index.to_string())
    {
        return Err(ComponentError::Link(
            "typed component binding expects tuple".to_owned(),
        ));
    }
    Ok(fields.iter().map(|field| &field.ty).collect())
}

fn extract_option_payload_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<&'a ValType, ComponentError> {
    let Type::DefVal(DefValType::Variant(cases)) = resolve_defined_type(ty, program)? else {
        return Err(ComponentError::Link(
            "typed component binding expects option".to_owned(),
        ));
    };
    match cases.as_slice() {
        [none, some] if none.label.0 == "none" && none.ty.is_none() && some.label.0 == "some" => {
            some.ty.as_ref().ok_or_else(|| {
                ComponentError::Link("typed component binding expects option payload".to_owned())
            })
        }
        _ => Err(ComponentError::Link(
            "typed component binding expects option".to_owned(),
        )),
    }
}

fn extract_result_payload_types<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<(&'a ValType, &'a ValType), ComponentError> {
    let Type::DefVal(DefValType::Variant(cases)) = resolve_defined_type(ty, program)? else {
        return Err(ComponentError::Link(
            "typed component binding expects result".to_owned(),
        ));
    };
    match cases.as_slice() {
        [ok, err] if ok.label.0 == "ok" && err.label.0 == "err" => Ok((
            ok.ty.as_ref().ok_or_else(|| {
                ComponentError::Link("typed component binding expects ok payload".to_owned())
            })?,
            err.ty.as_ref().ok_or_else(|| {
                ComponentError::Link("typed component binding expects err payload".to_owned())
            })?,
        )),
        _ => Err(ComponentError::Link(
            "typed component binding expects result".to_owned(),
        )),
    }
}

impl ComponentInstance {
    pub fn get_func(&self, name: &str) -> Result<ComponentFunc, ComponentError> {
        let type_id = self
            ._program
            .get_root_func_type_id(name)
            .ok_or_else(|| ComponentError::ExportNotFound(name.to_owned()))?;
        Ok(ComponentFunc::new(
            self.runtime.clone(),
            Rc::clone(&self._program),
            name,
            type_id,
        ))
    }

    pub fn get_typed_func<P, R>(
        &self,
        name: &str,
    ) -> Result<TypedComponentFunc<P, R>, ComponentError>
    where
        P: ComponentParams,
        R: ComponentReturn,
    {
        self.get_func(name)?.typed()
    }
}
