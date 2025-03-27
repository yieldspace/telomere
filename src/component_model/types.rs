use crate::component_model::{Alias, CoreType};

pub enum Type {
    DefVal(DefValType),
    Func(FuncType),
    Component(ComponentType),
    Instance(InstanceType),
    Resource(ResourceType),
}

pub enum PrimValType {
    Bool = 0x7f,
    S8 = 0x7e,
    U8 = 0x7d,
    S16 = 0x7c,
    U16 = 0x7b,
    S32 = 0x7a,
    U32 = 0x79,
    S64 = 0x78,
    U64 = 0x77,
    F32 = 0x76,
    F64 = 0x75,
    Char = 0x74,
    String = 0x73,
    #[cfg(feature = "async")]
    ErrorContext = 0x64,
}

pub enum DefValType {
    Primitive(PrimValType),
    Record(Vec<LabelValType>),
    Variant(Vec<Case>),
    List(Vec<ValType>, Option<usize>),
    Tuple(Vec<ValType>),
    Flags(Vec<Label>),
    Enum(Vec<Label>),
    Option(ValType),
    Result(Option<ValType>, Option<ValType>),
    Own(usize),
    Borrow(usize),
    #[cfg(feature = "async")]
    Stream(Option<ValType>),
    #[cfg(feature = "async")]
    Future(Option<ValType>),
}

pub struct LabelValType {
    label: Label,
    t: ValType,
}

pub struct Case {
    label: Label,
    t: Option<ValType>,
}

pub struct Label {
    len: usize,
    label: String, // TODO: check label format https://github.com/WebAssembly/component-model/blob/main/design/mvp/Explainer.md#import-and-export-definitions
}

pub enum ValType {
    TypeId(usize),
    Primitive(PrimValType),
}

pub enum ResourceType {
    Resource(Option<usize>),
    ResourceWithAsyncCallback(usize, Option<usize>),
}

pub struct FuncType {
    params: Vec<LabelValType>,
    result: Option<ValType>,
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
    name: String,
    ed: ExterDesc,
}

pub struct ExportDecl {
    name: String,
    ed: ExterDesc,
}

pub enum ExterDesc {
    Core(usize),
    Func(usize),
    #[cfg(feature = "import_export")]
    Value(ValueBound),
    Type(TypeBound),
    Component(usize),
    Instance(usize),
}

pub enum TypeBound {
    Eq(usize),
    Sub,
}

#[cfg(feature = "import_export")]
pub enum ValueBound {
    Eq(usize),
    Type(ValType),
}
