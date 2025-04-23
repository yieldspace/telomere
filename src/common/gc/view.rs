use super::MemoryPool;

// GC のトレース用トレイト
pub trait GCView {
    fn trace(&self, pool: &mut MemoryPool);
    fn update(&mut self, pool: &mut MemoryPool);
}