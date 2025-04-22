use crate::component_model::{
    AliasIdx, ComponentExportSlot, ComponentFunction, CoreModule, CoreType, CoreTypeIdx, FuncIdx,
    InlineComponent, Instance, InstanceIdx, Reference, Slot, TypeIdx,
};
use crate::parser::component_model::{ComponentParseError, Validator};
use crate::parser::leb128::compile_i32;
use num_derive::FromPrimitive;
use std::collections::HashMap;

macro_rules! impl_try_into_type {
    ($from:ident, $variant:ident) => {
        impl TryFrom<Type> for $from {
            type Error = ComponentParseError;
            fn try_from(value: Type) -> Result<Self, Self::Error> {
                if let Type::$variant(value) = value {
                    Ok(value)
                } else {
                    Err(ComponentParseError::InvalidType(
                        stringify!($variant).to_string(),
                    ))
                }
            }
        }
    };
}

#[derive(Debug, Clone)]
pub enum Type {
    DefVal(DefValType),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
    // from (sub resource)
    UniqueResource,
    Eq(TypeIdx),
    SuperTypedUniqueResource(TypeIdx),
    Referenced(Box<Type>, Reference),
}

impl_try_into_type!(FuncType, Func);
impl_try_into_type!(ComponentType, Component);
impl_try_into_type!(InstanceType, Instance);

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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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
pub struct ComponentExportType {}

#[derive(Debug, Clone)]
pub struct ComponentImportType {}

#[derive(Debug, Clone)]
pub struct ComponentType {
    pub(crate) imports: HashMap<String, ComponentImportType>,
    pub(crate) exports: HashMap<String, ComponentExportType>,
    pub(crate) core_types: Vec<CoreTypeIdx>,
    pub(crate) types: Vec<TypeIdx>,
    pub(crate) instances: Vec<InstanceIdx>,
}

impl From<Vec<ComponentDecl>> for ComponentType {
    fn from(value: Vec<ComponentDecl>) -> Self {
        let mut imports = HashMap::new();
        let mut exports = HashMap::new();
        let mut core_types = vec![];
        let mut types = vec![];
        let mut instances = vec![];

        for decl in value {
            match decl {
                ComponentDecl::Import(import_decl) => {
                    imports.insert(import_decl.name, ComponentImportType {});
                }
                ComponentDecl::Instance(instance_decl) => match instance_decl {
                    InstanceDecl::CoreType(idx) => core_types.push(idx),
                    InstanceDecl::Type(idx) => types.push(idx),
                    InstanceDecl::Alias(idx) => match idx {
                        AliasIdx::Type(idx) => types.push(idx),
                        AliasIdx::Instance(idx) => instances.push(idx),
                        _ => unreachable!(),
                    },
                    InstanceDecl::ExportDecl(export_decl) => {
                        exports.insert(export_decl.name, ComponentExportType {});
                    }
                },
            }
        }

        Self {
            imports,
            exports,
            core_types,
            types,
            instances,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ComponentDecl {
    Import(ImportDecl),
    Instance(InstanceDecl),
}

#[derive(Debug, Clone)]
pub struct InstanceType {
    pub(crate) core_types: Vec<CoreTypeIdx>,
    pub(crate) types: Vec<TypeIdx>,
    pub(crate) instances: Vec<InstanceIdx>,
    pub(crate) exports: HashMap<String, InstanceExportType>,
}

impl From<Vec<InstanceDecl>> for InstanceType {
    fn from(value: Vec<InstanceDecl>) -> Self {
        let mut core_types = vec![];
        let mut types = vec![];
        let mut instances = vec![];
        let mut exports = HashMap::new();

        for decl in value {
            match decl {
                InstanceDecl::CoreType(idx) => core_types.push(idx),
                InstanceDecl::Type(idx) => types.push(idx),
                InstanceDecl::Alias(idx) => match idx {
                    AliasIdx::Type(idx) => types.push(idx),
                    AliasIdx::Instance(idx) => instances.push(idx),
                    _ => unreachable!(),
                },
                InstanceDecl::ExportDecl(export_decl) => {
                    exports.insert(
                        export_decl.name,
                        InstanceExportType {
                            desc: export_decl.ed,
                        },
                    );
                }
            }
        }

        Self {
            core_types,
            types,
            instances,
            exports,
        }
    }
}

#[derive(Debug, Clone)]
pub struct InstanceExportType {
    pub(crate) desc: ExternDesc,
}

impl InstanceType {
    pub fn get_export(&self, name: &String) -> Result<InstanceExportType, ComponentParseError> {
        self.exports
            .get(name)
            .cloned()
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
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
