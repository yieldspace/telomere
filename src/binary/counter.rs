pub struct Counter(pub usize);

impl Counter {
    pub fn new() -> Self {
        Counter(0)
    }

    pub fn get_count(&self) -> usize {
        self.0
    }
}

pub trait Countable<T> {
    fn count(self, counter: &mut Counter) -> T;
}

impl<T> Countable<T> for (usize, T) {
    #[inline(always)]
    fn count(self, counter: &mut Counter) -> T {
        let (count, value) = self;
        counter.0 += count;
        value
    }
}

impl Countable<u8> for u8 {
    fn count(self, counter: &mut Counter) -> u8 {
        counter.0 += 1;
        self
    }
}
