#[derive(Clone, Debug)]
pub struct ComponentTypeInfo {
    pub id: u32,
}

#[derive(Clone, Debug)]
pub enum ComponentOp {
    Instantiate { component_idx: u32 },
    Alias { source_idx: u32, target_idx: u32 },
    CanonLower { func_idx: u32 },
    CanonLift { func_idx: u32 },
    Export { name: String },
}

#[derive(Clone, Debug)]
pub struct ComponentProgram {
    pub types: Vec<ComponentTypeInfo>,
    pub imports: Vec<String>,
    pub callable_imports: Vec<String>,
    pub exports: Vec<String>,
    pub callable_exports: Vec<String>,
    pub ops: Vec<ComponentOp>,
    pub bytes: Vec<u8>,
}
