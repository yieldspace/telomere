use crate::component_model::id::TypeId;
use crate::component_model::{Alias, CoreType};
use crate::parser::leb128::compile_i32;
use num_derive::FromPrimitive;

pub enum Type {
    DefVal(DefValType),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
}

#[derive(FromPrimitive)]
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
    #[cfg(feature = "async")]
    ErrorContext = compile_i32([0x64]),
}

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
    Own(TypeId),
    Borrow(TypeId),
    #[cfg(feature = "async")]
    Stream(Option<ValType>),
    #[cfg(feature = "async")]
    Future(Option<ValType>),
}

pub struct LabelValType {
    pub label: Label,
    pub t: ValType,
}

pub struct Case {
    pub label: Label,
    pub t: Option<ValType>,
}

pub struct Label {
    pub len: usize,
    pub label: String, // TODO: check label format https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md#import-and-export-definitions
}

pub enum ValType {
    TypeId(TypeId),
    Primitive(PrimValType),
}

pub enum ResourceType {
    Resource(Option<usize>),
    ResourceWithAsyncCallback(usize, Option<usize>),
}

pub struct FuncType {
    pub params: Vec<LabelValType>,
    pub result: Option<ValType>,
}

pub struct ComponentType(pub Vec<ComponentDecl>);

pub enum ComponentDecl {
    Import(ImportDecl),
    Instance(InstanceDecl),
}

pub struct InstanceType(pub Vec<InstanceDecl>);

pub enum InstanceDecl {
    CoreType(CoreType),
    Type(Type),
    Alias(Alias),
    ExportDecl(ExportDecl),
}

pub struct ImportDecl {
    pub name: String,
    pub ed: ExternDesc,
}

pub struct ExportDecl {
    pub name: String,
    pub ed: ExternDesc,
}

pub enum ExternDesc {
    Core(usize),
    Func(usize),
    #[cfg(feature = "import_export")]
    Value(ValueBound),
    Type(TypeBound),
    Component(usize),
    Instance(usize),
}

pub enum TypeBound {
    Eq(TypeId),
    Sub,
}

#[cfg(feature = "import_export")]
pub enum ValueBound {
    Eq(usize),
    Type(ValType),
}

#[cfg(test)]
mod tests {
    use crate::component_model::types::PrimValType;
    use crate::parser::leb128::compile_i32;
    use num_traits::FromPrimitive;

    #[test]
    fn test_prim() {
        assert_eq!(compile_i32([0x7f]), PrimValType::Bool as i32);
    }
}
