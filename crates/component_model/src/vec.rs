use crate::Result;
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait Idx: Copy + 'static + Eq + PartialEq + Debug + Hash + From<u32> {
    fn new(value: u32) -> Self;
    fn index(&self) -> usize;
}

pub(crate) struct IndexVec<I: Idx, T> {
    pub raw: Vec<T>,
    _marker: PhantomData<fn(&I)>,
}

impl<I: Idx, T> IndexVec<I, T> {
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

    pub fn push(&mut self, item: T) -> Result<I> {
        let index = self.raw.len() as u32;
        self.raw.push(item);
        Ok(I::new(index))
    }

    pub fn is_valid(&self, idx: &I) -> bool {
        idx.index() < self.raw.len()
    }

    #[track_caller]
    pub fn get(&self, idx: &I) -> Result<&T> {
        if !self.is_valid(idx) {
            return Err(crate::ComponentParseError::IndexError(
                "Invalid index".into(),
            ));
        }
        Ok(self.raw.get(idx.index()).unwrap())
    }
}

impl<I: Idx, T> Default for IndexVec<I, T> {
    fn default() -> Self {
        Self::new()
    }
}
