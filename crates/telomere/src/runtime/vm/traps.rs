// #207 removed the crate-private trap tables and their expansion target. The
// exported macro remains pending a maintainer decision because its symbol is
// semver-visible even though expansion was already unusable outside this crate.
#[macro_export]
macro_rules! trap_func {
    ($data: expr) => {
        match $data {
            VMResult::Success(v) => v,
            other => return $crate::runtime::vm::traps::trap_func(other),
        }
    };
}
