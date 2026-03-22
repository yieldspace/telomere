#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectRef(pub u32);

impl ObjectRef {
    pub fn get(&self) -> u32 {
        self.0
    }

    pub fn is_null(&self) -> bool {
        self.0 == 0
    }
}
