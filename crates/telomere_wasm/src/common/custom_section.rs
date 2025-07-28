#[derive(Debug, Clone)]
pub struct ModuleNameSubSec(pub String);
#[derive(Debug, Clone)]
pub struct FuncNameSubSec(pub Vec<(u32, String)>);
#[derive(Debug, Clone)]
pub struct LocalNameSubSec(pub Vec<(u32, Vec<(u32, String)>)>);
#[derive(Debug, Clone)]
pub struct NameSubSection {
    pub module_name: Option<ModuleNameSubSec>,
    pub function_name: Option<FuncNameSubSec>,
    pub local_name: Option<LocalNameSubSec>,
}
