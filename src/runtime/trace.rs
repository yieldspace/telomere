#[cfg(debug_assertions)]
macro_rules! trace {
    ($($l:expr),*) => {
        tracing::trace!($($l),*);
    };
}
#[cfg(not(debug_assertions))]
macro_rules! trace {
    ($($l:expr),*) => {};
}
