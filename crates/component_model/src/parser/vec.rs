use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait RawIdx: Copy + 'static + Eq + PartialEq + Debug + Hash {
    fn new(index: u32) -> Self;
    fn new_outer(outer: u32, index: u32) -> Self;
    fn index(&self) -> crate::Result<usize>;
}

pub(crate) struct RawIndexVec<I: RawIdx, T> {
    pub raw: Vec<T>,
    _marker: PhantomData<fn(&I)>,
}

impl<I: RawIdx, T> RawIndexVec<I, T> {
    pub fn new() -> Self {
        Self {
            raw: Vec::new(),
            _marker: PhantomData,
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            raw: Vec::with_capacity(capacity),
            _marker: PhantomData,
        }
    }

    pub fn push(&mut self, item: T) -> Result<I, ()> {
        let index = self.raw.len() as u32;
        self.raw.push(item);
        Ok(I::new(index))
    }

    pub fn get(&self, idx: &I) -> Option<&T> {
        let index = idx.index().ok()?;
        self.raw.get(index)
    }

    pub fn get_mut(&mut self, idx: &I) -> Option<&mut T> {
        let index = idx.index().ok()?;
        self.raw.get_mut(index)
    }

    pub fn is_valid(&self, idx: &I) -> bool {
        idx.index().map_or(false, |index| index < self.raw.len())
    }
}
