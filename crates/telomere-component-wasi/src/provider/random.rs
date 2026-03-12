use super::WasiHost;
use crate::bindings::imports::{
    wasi_random_insecure as random_insecure, wasi_random_insecure_seed as random_insecure_seed,
    wasi_random_random as random_random,
};
use std::rc::Rc;
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

pub(super) fn add_to_linker_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    random_random::add_to_linker(linker, Rc::clone(&host));
    random_insecure::add_to_linker(linker, Rc::clone(&host));
    random_insecure_seed::add_to_linker(linker, host);
}

pub(super) fn add_to_linker_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    random_random::add_to_linker_async(linker, Rc::clone(&host));
    random_insecure::add_to_linker_async(linker, Rc::clone(&host));
    random_insecure_seed::add_to_linker_async(linker, host);
}

impl WasiHost {
    fn next_random_u64(&self) -> u64 {
        let mut inner = self.state.inner.borrow_mut();
        inner.random_seed = inner.random_seed.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = inner.random_seed;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    fn random_bytes(&self, len: u64) -> Vec<u8> {
        let len = usize::try_from(len).unwrap_or(usize::MAX).min(1 << 20);
        let mut bytes = Vec::with_capacity(len);
        while bytes.len() < len {
            bytes.extend_from_slice(&self.next_random_u64().to_le_bytes());
        }
        bytes.truncate(len);
        bytes
    }
}

impl random_random::Host for WasiHost {
    fn get_random_bytes(&self, _store: &mut Store, len: u64) -> Result<Vec<u8>, ComponentError> {
        Ok(self.random_bytes(len))
    }

    fn get_random_u64(&self, _store: &mut Store) -> Result<u64, ComponentError> {
        Ok(self.next_random_u64())
    }
}

impl random_random::HostAsync for WasiHost {
    fn get_random_bytes<'a>(
        &'a self,
        _store: &'a mut Store,
        len: u64,
    ) -> ComponentFuture<'a, Result<Vec<u8>, ComponentError>> {
        Box::pin(async move { Ok(self.random_bytes(len)) })
    }

    fn get_random_u64<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<u64, ComponentError>> {
        Box::pin(async move { Ok(self.next_random_u64()) })
    }
}

impl random_insecure::Host for WasiHost {
    fn get_insecure_random_bytes(
        &self,
        _store: &mut Store,
        len: u64,
    ) -> Result<Vec<u8>, ComponentError> {
        Ok(self.random_bytes(len))
    }

    fn get_insecure_random_u64(&self, _store: &mut Store) -> Result<u64, ComponentError> {
        Ok(self.next_random_u64())
    }
}

impl random_insecure::HostAsync for WasiHost {
    fn get_insecure_random_bytes<'a>(
        &'a self,
        _store: &'a mut Store,
        len: u64,
    ) -> ComponentFuture<'a, Result<Vec<u8>, ComponentError>> {
        Box::pin(async move { Ok(self.random_bytes(len)) })
    }

    fn get_insecure_random_u64<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<u64, ComponentError>> {
        Box::pin(async move { Ok(self.next_random_u64()) })
    }
}

impl random_insecure_seed::Host for WasiHost {
    fn insecure_seed(&self, _store: &mut Store) -> Result<(u64, u64), ComponentError> {
        Ok((self.next_random_u64(), self.next_random_u64()))
    }
}

impl random_insecure_seed::HostAsync for WasiHost {
    fn insecure_seed<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<(u64, u64), ComponentError>> {
        Box::pin(async move { Ok((self.next_random_u64(), self.next_random_u64())) })
    }
}
