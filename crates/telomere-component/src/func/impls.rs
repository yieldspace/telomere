use super::traits::ResultPayload;
use super::typecheck::{
    ensure_matches_type, extract_list_element_type, extract_option_payload_type,
    extract_resource_handle_type, extract_result_payload_types, extract_tuple_types,
    ResourceHandleKind, TypeExpectation,
};
#[cfg(feature = "component-gated-feature-async")]
use super::typecheck::{extract_future_payload_type, extract_stream_payload_type};
use super::*;

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

impl<T> ResultPayload for T
where
    T: LowerComponent + LiftComponent,
{
    fn lower_result_payload(self) -> Result<Option<Box<ComponentValue>>, ComponentError> {
        Ok(Some(Box::new(self.lower_component()?)))
    }

    fn lift_result_payload(value: Option<Box<ComponentValue>>) -> Result<Self, ComponentError> {
        let value = value
            .ok_or_else(|| ComponentError::InvalidArgument("result payload missing".to_owned()))?;
        T::lift_component(*value)
    }

    fn matches_result_payload(
        ty: Option<&ValType>,
        program: &ComponentProgram,
    ) -> Result<(), ComponentError> {
        let ty = ty.ok_or_else(|| {
            ComponentError::Link("typed component binding expects result payload".to_owned())
        })?;
        <T as LowerComponent>::matches_type(ty, program)
    }
}

impl ResultPayload for () {
    fn lower_result_payload(self) -> Result<Option<Box<ComponentValue>>, ComponentError> {
        Ok(Some(Box::new(ComponentValue::Tuple(Vec::new()))))
    }

    fn lift_result_payload(value: Option<Box<ComponentValue>>) -> Result<Self, ComponentError> {
        match value.as_deref() {
            None => Ok(()),
            Some(ComponentValue::Tuple(values)) if values.is_empty() => Ok(()),
            _ => Err(ComponentError::InvalidArgument(
                "unexpected result payload".to_owned(),
            )),
        }
    }

    fn matches_result_payload(
        ty: Option<&ValType>,
        _program: &ComponentProgram,
    ) -> Result<(), ComponentError> {
        if ty.is_none() {
            Ok(())
        } else {
            Err(ComponentError::Link(
                "typed component binding expects payloadless result case".to_owned(),
            ))
        }
    }
}

impl<T, E> LowerComponent for Result<T, E>
where
    T: ResultPayload,
    E: ResultPayload,
{
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(match self {
            Ok(value) => ComponentValue::Result {
                ok: value.lower_result_payload()?,
                err: None,
            },
            Err(error) => ComponentValue::Result {
                ok: None,
                err: error.lower_result_payload()?,
            },
        })
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let (ok, err) = extract_result_payload_types(ty, program)?;
        T::matches_result_payload(ok, program)?;
        E::matches_result_payload(err, program)
    }
}

impl<T, E> LiftComponent for Result<T, E>
where
    T: ResultPayload,
    E: ResultPayload,
{
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Result { ok, err: None } => Ok(Ok(T::lift_result_payload(ok)?)),
            ComponentValue::Result { ok: None, err } => Ok(Err(E::lift_result_payload(err)?)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected result value, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let (ok, err) = extract_result_payload_types(ty, program)?;
        T::matches_result_payload(ok, program)?;
        E::matches_result_payload(err, program)
    }
}

impl<T> LowerComponent for Own<T> {
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::Own(self.handle()))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        match extract_resource_handle_type(ty, program)? {
            ResourceHandleKind::Own => Ok(()),
            ResourceHandleKind::Borrow => Err(ComponentError::Link(
                "typed component binding expects own resource".to_owned(),
            )),
        }
    }
}

impl<T> LiftComponent for Own<T> {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Own(handle) => Ok(Self::new(handle)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected own resource result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        <Self as LowerComponent>::matches_type(ty, program)
    }
}

impl<T> LowerComponent for Borrow<T> {
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::Borrow(self.handle()))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        match extract_resource_handle_type(ty, program)? {
            ResourceHandleKind::Borrow => Ok(()),
            ResourceHandleKind::Own => Err(ComponentError::Link(
                "typed component binding expects borrow resource".to_owned(),
            )),
        }
    }
}

impl<T> LiftComponent for Borrow<T> {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Borrow(handle) => Ok(Self::new(handle)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected borrow resource result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        <Self as LowerComponent>::matches_type(ty, program)
    }
}

#[cfg(feature = "component-gated-feature-async")]
impl LowerComponent for ComponentErrorContext {
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::ErrorContext(self.handle()))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        ensure_matches_type::<Self>(ty, program, TypeExpectation::ErrorContext)
    }
}

#[cfg(feature = "component-gated-feature-async")]
impl LiftComponent for ComponentErrorContext {
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::ErrorContext(handle) => Ok(Self::new(handle)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected error-context result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        <Self as LowerComponent>::matches_type(ty, program)
    }
}

#[cfg(feature = "component-gated-feature-async")]
impl<T> LowerComponent for ComponentFutureHandle<T>
where
    T: LowerComponent,
{
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::Future(self.handle()))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let payload = extract_future_payload_type(ty, program)?;
        match payload {
            Some(payload) => T::matches_type(payload, program),
            None => Ok(()),
        }
    }
}

#[cfg(feature = "component-gated-feature-async")]
impl<T> LiftComponent for ComponentFutureHandle<T>
where
    T: LiftComponent,
{
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Future(handle) => Ok(Self::new(handle)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected future result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let payload = extract_future_payload_type(ty, program)?;
        match payload {
            Some(payload) => T::matches_type(payload, program),
            None => Ok(()),
        }
    }
}

#[cfg(feature = "component-gated-feature-async")]
impl<T> LowerComponent for ComponentStreamHandle<T>
where
    T: LowerComponent,
{
    fn lower_component(self) -> Result<ComponentValue, ComponentError> {
        Ok(ComponentValue::Stream(self.handle()))
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let payload = extract_stream_payload_type(ty, program)?;
        match payload {
            Some(payload) => T::matches_type(payload, program),
            None => Ok(()),
        }
    }
}

#[cfg(feature = "component-gated-feature-async")]
impl<T> LiftComponent for ComponentStreamHandle<T>
where
    T: LiftComponent,
{
    fn lift_component(value: ComponentValue) -> Result<Self, ComponentError> {
        match value {
            ComponentValue::Stream(handle) => Ok(Self::new(handle)),
            other => Err(ComponentError::InvalidArgument(format!(
                "expected stream result, got {other:?}"
            ))),
        }
    }

    fn matches_type(ty: &ValType, program: &ComponentProgram) -> Result<(), ComponentError> {
        let payload = extract_stream_payload_type(ty, program)?;
        match payload {
            Some(payload) => T::matches_type(payload, program),
            None => Ok(()),
        }
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
