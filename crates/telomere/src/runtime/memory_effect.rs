use std::{fmt, future::Future, pin::Pin};

use crate::{
    common::{Instr, StablePc},
    VMResult,
};
pub type AsyncEffectFuture = Pin<Box<dyn Future<Output = AsyncResult>>>;
#[derive(Debug)]
pub struct AsyncResult {
    pub task_id: u32,
    pub completion: AsyncCompletion,
}
#[derive(Debug)]
pub enum AsyncCompletion {
    #[allow(dead_code)]
    Continue {
        fp: StablePc,
    },
    ContinueWithI32 {
        fp: StablePc,
        value: i32,
    },
    HostCall {
        result: VMResult<*const Instr>,
    },
}
#[cfg(feature = "async-runtime")]
pub struct AsyncEffect {
    pub future: AsyncEffectFuture,
}
#[cfg(feature = "async-runtime")]
impl fmt::Debug for AsyncEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AsyncEffect(..)")
    }
}

#[derive(Debug)]
pub enum Effect {
    #[cfg(feature = "async-runtime")]
    AsyncEffect(AsyncEffect),
}
