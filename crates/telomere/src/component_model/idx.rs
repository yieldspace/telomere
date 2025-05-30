use std::fmt::{Debug, Formatter};
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

macro_rules! impl_idx {
    ($name:ident, $value:ty) => {
        pub struct $name<T>($value, PhantomData<T>);

        impl<T> Clone for $name<T> {
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<T> Copy for $name<T> {}

        impl<T> Debug for $name<T> {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl<T> Hash for $name<T> {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }

        impl<T> PartialEq for $name<T> {
            fn eq(&self, other: &Self) -> bool {
                self.0 == other.0
            }
        }

        impl<T> Eq for $name<T> {}
    };
}

impl_idx!(LocalIdx, u32);
impl_idx!(GlobalIdx, u32);

impl<T> LocalIdx<T> {
    pub fn new(value: u32) -> Self {
        Self(value, PhantomData)
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

impl<T> From<usize> for LocalIdx<T> {
    fn from(value: usize) -> Self {
        Self::new(value as u32)
    }
}

static GLOBAL_IDX_COUNTER: AtomicU32 = AtomicU32::new(0);

impl<T> Default for GlobalIdx<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GlobalIdx<T> {
    pub fn new() -> Self {
        Self(
            GLOBAL_IDX_COUNTER.fetch_add(1, Ordering::Relaxed),
            PhantomData,
        )
    }
}
