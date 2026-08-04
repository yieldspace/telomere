#[derive(Debug, Clone)]
/// The module name stored in a WebAssembly `name` custom section.
// #202 retains parsed names although the runtime does not yet consume them.
#[allow(dead_code)]
pub struct ModuleNameSubSec(
    /// UTF-8 module name supplied by the producer.
    pub String,
);
#[derive(Debug, Clone)]
/// Function names stored in a WebAssembly `name` custom section.
// #202 retains parsed names although the runtime does not yet consume them.
#[allow(dead_code)]
pub struct FuncNameSubSec(
    /// Pairs of function indices and producer-supplied names.
    pub Vec<(u32, String)>,
);
#[derive(Debug, Clone)]
/// Local-variable names stored in a WebAssembly `name` custom section.
// #202 retains parsed names although the runtime does not yet consume them.
#[allow(dead_code)]
pub struct LocalNameSubSec(
    /// Pairs of function indices and their local-index/name pairs.
    pub Vec<(u32, Vec<(u32, String)>)>,
);
#[derive(Debug, Clone)]
/// Parsed optional subsections from a WebAssembly `name` custom section.
// #202 retains parsed names although the runtime does not yet consume them.
#[allow(dead_code)]
pub struct NameSubSection {
    /// The producer-supplied module name, when present.
    pub module_name: Option<ModuleNameSubSec>,
    /// Producer-supplied function names, when present.
    pub function_name: Option<FuncNameSubSec>,
    /// Producer-supplied local-variable names, when present.
    pub local_name: Option<LocalNameSubSec>,
}
