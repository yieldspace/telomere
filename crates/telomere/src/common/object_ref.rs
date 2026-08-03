#[repr(transparent)]
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// A stable numeric reference to an object allocated in a [`crate::Store`].
///
/// Most embedders use [`crate::common::InstanceHandle`] instead. This type is
/// exposed for integrations that need to retain runtime object identities.
pub struct ObjectRef(pub u32);

impl ObjectRef {
    /// Returns the raw store-local object number.
    pub fn get(&self) -> u32 {
        self.0
    }

    /// Returns whether this is the null object reference.
    pub fn is_null(&self) -> bool {
        self.0 == 0
    }
}
