use crate::common::InstanceAddr;

pub struct CoreModuleInstance {
    addr: InstanceAddr,
}

impl CoreModuleInstance {
    pub fn new(addr: InstanceAddr) -> Self {
        Self { addr }
    }
}
