use super::MemoryPool;

// GC のトレース用トレイト
pub trait GcView {
    fn trace(&self, pool: &mut MemoryPool);
    fn update(&mut self, pool: &mut MemoryPool);
}
