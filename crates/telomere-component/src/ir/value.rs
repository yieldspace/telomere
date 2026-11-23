use crate::ir::types::ValType;
use crate::value::ComponentValue;

/// A component-model value in the value index space.
///
/// Defined values (the value section, sort `0x02`) are statically encoded
/// literals, so the payload is the canonical ABI value representation
/// ([`ComponentValue`]) together with its declared type. Imported, exported,
/// and aliased values reuse the same representation once resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Value {
    pub ty: ValType,
    pub value: ComponentValue,
}
