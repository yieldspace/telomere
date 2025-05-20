use std::collections::HashMap;
use std::hash::Hash;

pub struct UnionFind<T>
where
    T: Hash + Clone + Eq,
{
    parents: HashMap<T, T>,
    rank: HashMap<T, usize>,
}

impl<T> Default for UnionFind<T>
where
    T: Hash + Clone + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> UnionFind<T>
where
    T: Hash + Clone + Eq,
{
    pub fn new() -> Self {
        Self {
            parents: HashMap::default(),
            rank: HashMap::default(),
        }
    }

    pub fn find(&mut self, x: &T) -> T {
        if let Some(y) = self.parents.get(x) {
            let root = self.find(&y.clone()).clone();
            self.parents.insert(x.clone(), root.clone());
            root
        } else {
            x.clone()
        }
    }

    pub fn union(&mut self, x: T, y: T) {
        let rx = self.find(&x);
        let ry = self.find(&y);
        if rx == ry {
            return;
        }

        let rx_rank = self.rank.get(&rx).copied().unwrap_or_default();
        let ry_rank = self.rank.get(&ry).copied().unwrap_or_default();
        if rx_rank < ry_rank {
            self.parents.insert(rx, ry.clone());
            if rx_rank == ry_rank {
                self.rank.insert(ry, ry_rank + 1);
            }
        } else {
            self.parents.insert(ry, rx.clone());
            if rx_rank == ry_rank {
                self.rank.insert(rx, rx_rank + 1);
            }
        };
    }

    pub fn merge(&mut self, other: &Self) {
        for (key, value) in &other.parents {
            self.union(key.clone(), value.clone());
        }
        for (key, value) in &other.rank {
            self.rank.insert(key.clone(), *value);
        }
    }
}
