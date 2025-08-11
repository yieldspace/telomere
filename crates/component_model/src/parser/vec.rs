use crate::Result;
use std::fmt::Debug;
use std::hash::Hash;
use std::marker::PhantomData;

pub trait RawIdx: Copy + 'static + Eq + PartialEq + Debug + Hash {
    fn new(index: u32) -> Self;
    fn new_outer(outer: u32, index: u32) -> Self;
    fn index(&self) -> Result<usize>;
}

pub enum Relation<T, I: RawIdx> {
    Direct(T),
    Alias(I),
}

pub(crate) struct RawIndexVec<I: RawIdx, T> {
    pub raw: Vec<Relation<T, I>>,
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

    pub fn push(&mut self, item: T) -> Result<I> {
        let index = self.raw.len() as u32;
        self.raw.push(Relation::Direct(item));
        Ok(I::new(index))
    }

    pub fn push_alias(&mut self, alias: I) -> Result<()> {
        if !self.is_valid(&alias) {
            return Err(crate::ComponentParseError::IndexError(
                "Invalid alias index".into(),
            ));
        }
        self.raw.push(Relation::Alias(alias));
        Ok(())
    }

    pub fn is_valid(&self, idx: &I) -> bool {
        idx.index().map_or(true, |index| index < self.raw.len())
    }

    #[track_caller]
    pub fn get(&self, idx: &I) -> Result<&T> {
        if !self.is_valid(idx) {
            return Err(crate::ComponentParseError::IndexError(
                "Invalid index".into(),
            ));
        }
        if let Some(relation) = self.raw.get(idx.index()?) {
            match relation {
                Relation::Direct(item) => Ok(item),
                Relation::Alias(i) => self.get(i),
            }
        } else {
            Err(crate::ComponentParseError::IndexError(
                "Not found".into(),
            ))
        }
    }
}
