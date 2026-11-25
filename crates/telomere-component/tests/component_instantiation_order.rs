//! Regression tests for #333: instances defined in a component must be
//! created eagerly, in definition order, when an instance of that
//! component is created — not lazily in the order their exports happen
//! to be resolved first, and not grouped by kind (all core instances
//! before all component instances). Core start functions observe the
//! creation order, so a shared ticket service must hand rank 1 to the
//! first-defined instance (upstream `linking/unit.wast` lines 645-647).

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

async fn instantiate_root(wat: &str) -> (telomere::Store, telomere_component::ComponentInstance) {
    let bytes = compile_component(wat);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    (store, instance)
}

async fn call_u32(
    store: &telomere::Store,
    instance: &telomere_component::ComponentInstance,
    name: &str,
) -> u32 {
    let result = instance
        .call(store, name, &[])
        .await
        .expect("call should succeed");
    match result.as_slice() {
        [ComponentValue::U32(value)] => *value,
        other => panic!("expected a single u32 result from {name}, got {other:?}"),
    }
}

/// A ticket service plus a child component whose core start function
/// takes a ticket and records it as its rank.
const TICKET_AND_CHILD: &str = r#"
  (component $Ticket
    (core module $M
      (global $n (mut i32) (i32.const 0))
      (func (export "next") (result i32)
        (global.set $n (i32.add (global.get $n) (i32.const 1)))
        (global.get $n)))
    (core instance $m (instantiate $M))
    (func (export "next") (result u32) (canon lift (core func $m "next"))))
  (component $Child
    (import "ticket" (instance $t (export "next" (func (result u32)))))
    (core func $next (canon lower (func $t "next")))
    (core module $M
      (import "t" "next" (func $next (result i32)))
      (global $rank (mut i32) (i32.const 0))
      (func $start (global.set $rank (call $next)))
      (start $start)
      (func (export "rank") (result i32) (global.get $rank)))
    (core instance $m (instantiate $M
      (with "t" (instance (export "next" (func $next))))))
    (func (export "rank") (result u32) (canon lift (core func $m "rank"))))
  (instance $ticket (instantiate $Ticket))
"#;

/// The parent's own core module with a start function that takes a
/// ticket the same way the child does.
const PARENT_CORE: &str = r#"
  (alias export $ticket "next" (func $nextf))
  (core func $next (canon lower (func $nextf)))
  (core module $PM
    (import "t" "next" (func $next (result i32)))
    (global $rank (mut i32) (i32.const 0))
    (func $start (global.set $rank (call $next)))
    (start $start)
    (func (export "rank") (result i32) (global.get $rank)))
  (core instance $pm (instantiate $PM
    (with "t" (instance (export "next" (func $next))))))
"#;

/// The upstream case: three sibling instances of one component, ranks
/// read in definition order, must be 1, 2, 3.
#[tokio::test]
async fn sibling_instances_take_tickets_in_definition_order() {
    let wat = format!(
        "(component{TICKET_AND_CHILD}
  (instance $a (instantiate $Child (with \"ticket\" (instance $ticket))))
  (instance $b (instantiate $Child (with \"ticket\" (instance $ticket))))
  (instance $c (instantiate $Child (with \"ticket\" (instance $ticket))))
  (func (export \"rank-a\") (alias export $a \"rank\"))
  (func (export \"rank-b\") (alias export $b \"rank\"))
  (func (export \"rank-c\") (alias export $c \"rank\")))"
    );
    let (store, instance) = instantiate_root(&wat).await;
    assert_eq!(call_u32(&store, &instance, "rank-a").await, 1);
    assert_eq!(call_u32(&store, &instance, "rank-b").await, 2);
    assert_eq!(call_u32(&store, &instance, "rank-c").await, 3);
}

/// Eagerness is the point: the ranks are fixed when the parent instance
/// is created, so reading them in reverse order must still observe
/// 1, 2, 3. A lazy implementation assigns tickets in first-use order
/// and fails this.
#[tokio::test]
async fn sibling_instance_ranks_do_not_depend_on_first_use_order() {
    let wat = format!(
        "(component{TICKET_AND_CHILD}
  (instance $a (instantiate $Child (with \"ticket\" (instance $ticket))))
  (instance $b (instantiate $Child (with \"ticket\" (instance $ticket))))
  (instance $c (instantiate $Child (with \"ticket\" (instance $ticket))))
  (func (export \"rank-a\") (alias export $a \"rank\"))
  (func (export \"rank-b\") (alias export $b \"rank\"))
  (func (export \"rank-c\") (alias export $c \"rank\")))"
    );
    let (store, instance) = instantiate_root(&wat).await;
    assert_eq!(call_u32(&store, &instance, "rank-c").await, 3);
    assert_eq!(call_u32(&store, &instance, "rank-a").await, 1);
    assert_eq!(call_u32(&store, &instance, "rank-b").await, 2);
}

/// Definition order runs across instance kinds: a component instance
/// defined before the parent's own core instance must be created first,
/// so its start function takes the earlier ticket.
#[tokio::test]
async fn component_instance_defined_before_core_instance_starts_first() {
    let wat = format!(
        "(component{TICKET_AND_CHILD}
  (instance $a (instantiate $Child (with \"ticket\" (instance $ticket))))
{PARENT_CORE}
  (func (export \"rank-a\") (alias export $a \"rank\"))
  (func (export \"rank-core\") (result u32) (canon lift (core func $pm \"rank\"))))"
    );
    let (store, instance) = instantiate_root(&wat).await;
    assert_eq!(call_u32(&store, &instance, "rank-a").await, 1);
    assert_eq!(call_u32(&store, &instance, "rank-core").await, 2);
}

/// The mirror image: a core instance defined before a component
/// instance must be created first.
#[tokio::test]
async fn core_instance_defined_before_component_instance_starts_first() {
    let wat = format!(
        "(component{TICKET_AND_CHILD}
{PARENT_CORE}
  (instance $a (instantiate $Child (with \"ticket\" (instance $ticket))))
  (func (export \"rank-a\") (alias export $a \"rank\"))
  (func (export \"rank-core\") (result u32) (canon lift (core func $pm \"rank\"))))"
    );
    let (store, instance) = instantiate_root(&wat).await;
    assert_eq!(call_u32(&store, &instance, "rank-core").await, 1);
    assert_eq!(call_u32(&store, &instance, "rank-a").await, 2);
}

/// The boundary from #316 is unchanged: a component that is only
/// defined, never instantiated, stays dormant — its core start function
/// (here, an unconditional trap) must not run.
#[tokio::test]
async fn defined_but_uninstantiated_component_stays_dormant() {
    let wat = format!(
        "(component{TICKET_AND_CHILD}
  (component $NeverUsed
    (core module $M
      (func $start unreachable)
      (start $start))
    (core instance $m (instantiate $M)))
  (instance $a (instantiate $Child (with \"ticket\" (instance $ticket))))
  (func (export \"rank-a\") (alias export $a \"rank\")))"
    );
    let (store, instance) = instantiate_root(&wat).await;
    assert_eq!(call_u32(&store, &instance, "rank-a").await, 1);
}
