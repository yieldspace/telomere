use std::rc::Rc;

use bindings::imports::example_counterdemo_service as service;
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

telomere_component_bindgen::bindgen!({
    inline: r#"
        package example:counterdemo;

        interface service {
            resource counter {
                constructor(seed: u32);
                clone: static func(other: borrow<counter>) -> counter;
                value: func() -> u32;
            }
        }

        world demo {
            import ping: func() -> u32;
            import service;
        }
    "#,
    world: "demo",
    module: "bindings",
    host_mode: "both"
});

struct SyncHost;

impl bindings::Imports for SyncHost {
    fn ping(&self, _store: &mut Store) -> Result<u32, ComponentError> {
        Ok(7)
    }
}

impl service::Host for SyncHost {
    fn counter_new(
        &self,
        _store: &mut Store,
        seed: u32,
    ) -> Result<service::Counter, ComponentError> {
        Ok(service::Counter::new(seed))
    }

    fn counter_clone(
        &self,
        _store: &mut Store,
        other: service::CounterBorrow,
    ) -> Result<service::Counter, ComponentError> {
        Ok(service::Counter::new(other.handle()))
    }

    fn counter_value(
        &self,
        _store: &mut Store,
        self_: service::CounterBorrow,
    ) -> Result<u32, ComponentError> {
        Ok(self_.handle())
    }
}

struct AsyncHost;

impl bindings::ImportsAsync for AsyncHost {
    fn ping<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<u32, ComponentError>> {
        Box::pin(async move { Ok(9) })
    }
}

impl service::HostAsync for AsyncHost {
    fn counter_new<'a>(
        &'a self,
        _store: &'a mut Store,
        seed: u32,
    ) -> ComponentFuture<'a, Result<service::Counter, ComponentError>> {
        Box::pin(async move { Ok(service::Counter::new(seed)) })
    }

    fn counter_clone<'a>(
        &'a self,
        _store: &'a mut Store,
        other: service::CounterBorrow,
    ) -> ComponentFuture<'a, Result<service::Counter, ComponentError>> {
        Box::pin(async move { Ok(service::Counter::new(other.handle())) })
    }

    fn counter_value<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: service::CounterBorrow,
    ) -> ComponentFuture<'a, Result<u32, ComponentError>> {
        Box::pin(async move { Ok(self_.handle()) })
    }
}

#[test]
fn bindgen_supports_resources_and_both_host_modes() {
    let mut linker = ComponentLinker::new();
    bindings::add_root_imports_to_linker(&mut linker, Rc::new(SyncHost));
    service::add_to_linker(&mut linker, Rc::new(SyncHost));
    bindings::add_root_imports_to_linker_async(&mut linker, Rc::new(AsyncHost));
    service::add_to_linker_async(&mut linker, Rc::new(AsyncHost));

    let own = service::Counter::new(11);
    let borrow = service::CounterBorrow::new(own.handle());
    assert_eq!(borrow.handle(), 11);
}
