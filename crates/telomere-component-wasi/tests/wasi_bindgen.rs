use std::rc::Rc;

use telomere_component::ComponentLinker;

telomere_component_wasi::bindgen!({
    path: "tests/wit/user",
    world: "ex:user/app",
    module: "bindings"
});

telomere_component_wasi::bindgen!({
    inline: r#"
        package ex:inlineuser;

        world app {
            include wasi:cli/imports@0.2.6;
        }
    "#,
    world: "ex:inlineuser/app",
    module: "inline_bindings"
});

struct Host;
struct InlineHost;

impl bindings::imports::wasi_cli_environment::Host for Host {}
impl inline_bindings::imports::wasi_cli_environment::Host for InlineHost {}

#[test]
fn wasi_wrapper_bindgen_reuses_provider_bindings() {
    fn assert_host<T: bindings::imports::wasi_cli_environment::Host>() {}

    assert_host::<Host>();

    let mut linker = ComponentLinker::new();
    bindings::imports::wasi_cli_environment::add_to_linker(&mut linker, Rc::new(Host));
}

#[test]
fn wasi_wrapper_bindgen_supports_inline_worlds_with_deps() {
    fn assert_host<T: inline_bindings::imports::wasi_cli_environment::Host>() {}

    assert_host::<InlineHost>();

    let mut linker = ComponentLinker::new();
    inline_bindings::imports::wasi_cli_environment::add_to_linker(&mut linker, Rc::new(InlineHost));
}
