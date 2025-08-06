use crate::types::TypeIdx;

pub struct TypeValidator<'a> {
    indexes: Vec<TypeIdx>,
    parent: Option<&'a TypeValidator<'a>>,
}

impl<'a> TypeValidator<'a> {
    pub fn new() -> Self {
        Self {
            indexes: Vec::new(),
            parent: None,
        }
    }

    pub fn with_parent(parent: &'a TypeValidator<'a>) -> Self {
        Self {
            indexes: Vec::new(),
            parent: Some(parent),
        }
    }
}
