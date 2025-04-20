use crate::component_model::{
    AliasIdx, Component, ComponentExportSlot, ComponentFunction, CoreModule, CoreType, CoreTypeIdx,
    FuncIdx, Instance, InstanceIdx, Reference, Slot, TypeIdx,
};
use crate::parser::component_model::{ComponentParseError, Validator};
use crate::parser::leb128::compile_i32;
use num_derive::FromPrimitive;

#[derive(Debug)]
pub enum Type {
    DefVal(DefValType),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
    // from (sub resource)
    UniqueResource,
    SuperTypedUniqueResource(TypeIdx),
}

#[derive(Debug, FromPrimitive, Clone)]
#[repr(i32)]
pub enum PrimValType {
    Bool = compile_i32([0x7f]),
    S8 = compile_i32([0x7e]),
    U8 = compile_i32([0x7d]),
    S16 = compile_i32([0x7c]),
    U16 = compile_i32([0x7b]),
    S32 = compile_i32([0x7a]),
    U32 = compile_i32([0x79]),
    S64 = compile_i32([0x78]),
    U64 = compile_i32([0x77]),
    F32 = compile_i32([0x76]),
    F64 = compile_i32([0x75]),
    Char = compile_i32([0x74]),
    String = compile_i32([0x73]),
    #[cfg(feature = "component-gated-feature-async")]
    ErrorContext = compile_i32([0x64]),
}

#[derive(Debug)]
pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(ValType, Option<usize>),
    Tuple(Vec<ValType>),
    Flags(Vec<Label>),
    Enum(Vec<Label>),
    Option(ValType),
    Result(Option<ValType>, Option<ValType>),
    Own(TypeIdx),
    Borrow(TypeIdx),
    #[cfg(feature = "component-gated-feature-async")]
    Stream(Option<ValType>),
    #[cfg(feature = "component-gated-feature-async")]
    Future(Option<ValType>),
}

#[derive(Debug, Clone)]
pub struct LabelValType {
    pub label: Label,
    pub t: ValType,
}

#[derive(Debug)]
pub struct Case {
    pub label: Label,
    pub t: Option<ValType>,
}

#[derive(Debug, Clone)]
pub struct Label {
    pub len: usize,
    pub label: String, // TODO: check label format https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md#import-and-export-definitions
}

#[derive(Debug, Clone)]
pub enum ValType {
    TypeId(TypeIdx),
    Primitive(PrimValType),
}

#[derive(Debug)]
pub enum ResourceType {
    Resource(Option<FuncIdx>),
    ResourceWithAsyncCallback(FuncIdx, Option<FuncIdx>),
}

#[derive(Debug, Clone)]
pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<ValType>,
}

#[derive(Debug, Clone)]
pub struct ComponentType(pub Vec<ComponentDecl>);

#[derive(Debug, Clone)]
pub enum ComponentDecl {
    Import(ImportDecl),
    Instance(InstanceDecl),
}

#[derive(Debug, Clone)]
pub struct InstanceType(pub Vec<InstanceDecl>);

impl InstanceType {
    pub fn get_export(
        &self,
        validator: &dyn Validator,
        self_idx: InstanceIdx,
        name: String,
    ) -> Result<ComponentExportSlot, ComponentParseError> {
        let decl = self
            .0
            .iter()
            .find_map(|decl| match decl {
                InstanceDecl::ExportDecl(decl) if decl.name == name => Some(decl),
                _ => None,
            })
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))?;
        match &decl.ed {
            ExternDesc::Core(idx) => {
                let ty = validator.get_core_type(idx);
                if let CoreType::ModuleType(ty) = ty {
                    Ok(ComponentExportSlot::CoreModule(Slot::Value(
                        CoreModule::Typed(ty.clone(), Reference::Instance(self_idx, name)),
                    )))
                } else {
                    panic!("Expected a module type");
                }
            }
            ExternDesc::Func(idx) => Ok(ComponentExportSlot::Func(Slot::Value(
                ComponentFunction::Typed(idx.clone(), Reference::Instance(self_idx, name)),
            ))),
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            ExternDesc::Value(_) => {
                todo!();
            }
            ExternDesc::Type(ty) => match ty {
                TypeBound::Eq(idx) => Ok(ComponentExportSlot::Type(Slot::Idx(idx.clone()))),
                TypeBound::Sub => Ok(ComponentExportSlot::Type(Slot::Value(Type::UniqueResource))),
            },
            ExternDesc::Component(ty) => {
                let ty = validator.get_type(ty);
                if let Type::Component(ty) = ty {
                    Ok(ComponentExportSlot::Component(Slot::Value(
                        Component::Typed(ty.clone(), Reference::Instance(self_idx, name)),
                    )))
                } else {
                    panic!("Expected a component type");
                }
            }
            ExternDesc::Instance(ty) => {
                let ty = validator.get_type(ty);
                if let Type::Instance(ty) = ty {
                    Ok(ComponentExportSlot::Instance(Slot::Value(Instance::Typed(
                        ty.clone(),
                    ))))
                } else {
                    panic!("Expected an instance type");
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum InstanceDecl {
    CoreType(CoreTypeIdx),
    Type(TypeIdx),
    Alias(AliasIdx),
    ExportDecl(ExportDecl),
}

#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub name: String,
    pub ed: ExternDesc,
}

#[derive(Debug, Clone)]
pub struct ExportDecl {
    pub name: String,
    pub ed: ExternDesc,
}

#[derive(Debug, Clone)]
pub enum ExternDesc {
    Core(CoreTypeIdx),
    Func(TypeIdx),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(ValueBound),
    Type(TypeBound),
    Component(TypeIdx),
    Instance(TypeIdx),
}

#[derive(Debug, Clone)]
pub enum TypeBound {
    Eq(TypeIdx),
    Sub,
}

#[derive(Debug, Clone)]
#[cfg(feature = "component-gated-feature-value-imports-exports")]
pub enum ValueBound {
    Eq(usize),
    Type(ValType),
}

#[cfg(test)]
mod tests {
    use crate::component_model::types::PrimValType;
    use crate::parser::leb128::compile_i32;

    #[test]
    fn test_prim() {
        assert_eq!(compile_i32([0x7f]), PrimValType::Bool as i32);
    }
}
