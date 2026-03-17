use super::*;
use crate::ir::types::LabelValType;
use crate::ir::{Component, TypeId};
use crate::{ComponentEngine, ComponentLinker};
use futures::executor::block_on;
use std::collections::HashMap;

fn dummy_program(types: Vec<Type>) -> ComponentProgram {
    ComponentProgram {
        type_infos: Vec::new(),
        imports: Vec::new(),
        callable_imports: Vec::new(),
        exports: Vec::new(),
        callable_exports: Vec::new(),
        ops: Vec::new(),
        bytes: Vec::new(),
        root: Component {
            imports: HashMap::new(),
            exports: HashMap::new(),
        },
        types: types.into_boxed_slice(),
        component_store: HashMap::new(),
        instance_store: HashMap::new(),
        func_store: HashMap::new(),
        core_module_store: HashMap::new(),
        core_type_store: HashMap::new(),
        core_instance_store: HashMap::new(),
        core_func_store: HashMap::new(),
        core_memory_store: HashMap::new(),
        core_global_store: HashMap::new(),
        core_table_store: HashMap::new(),
    }
}

#[test]
fn lower_and_lift_string_and_vec_round_trip() {
    let lowered = vec!["a".to_owned(), "b".to_owned()]
        .lower_component()
        .unwrap();
    assert_eq!(
        lowered,
        ComponentValue::List(vec![
            ComponentValue::String("a".to_owned()),
            ComponentValue::String("b".to_owned())
        ])
    );

    let lifted = Vec::<String>::lift_component(lowered).unwrap();
    assert_eq!(lifted, vec!["a".to_owned(), "b".to_owned()]);
}

#[test]
fn option_and_result_round_trip() {
    let some =
        Option::<u32>::lift_component(Option::<u32>::Some(42).lower_component().unwrap()).unwrap();
    assert_eq!(some, Some(42));

    let err = Result::<u32, String>::lift_component(
        Result::<u32, String>::Err("boom".to_owned())
            .lower_component()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(err, Err("boom".to_owned()));
}

#[test]
fn resource_handle_type_checks_reject_mismatch() {
    let resource = Type::Resource(crate::ir::ResourceId::synthetic());
    let own_ty = Type::DefVal(DefValType::Own(TypeId::from_index(0)));
    let borrow_ty = Type::DefVal(DefValType::Borrow(TypeId::from_index(0)));
    let program = dummy_program(vec![resource, own_ty, borrow_ty]);

    let own_val = ValType::Type(TypeId::from_index(1));
    let borrow_val = ValType::Type(TypeId::from_index(2));

    assert!(<Own<()> as LowerComponent>::matches_type(&own_val, &program).is_ok());
    assert!(<Borrow<()> as LowerComponent>::matches_type(&borrow_val, &program).is_ok());
    assert!(<Borrow<()> as LowerComponent>::matches_type(&own_val, &program).is_err());
    assert!(<Own<()> as LowerComponent>::matches_type(&borrow_val, &program).is_err());
}

#[test]
fn tuple_type_check_rejects_non_tuple_shape() {
    let tuple = Type::DefVal(DefValType::Record(vec![
        LabelValType::new(
            crate::ir::Label::new("0"),
            ValType::Primitive(PrimValType::U32),
        ),
        LabelValType::new(
            crate::ir::Label::new("x"),
            ValType::Primitive(PrimValType::U32),
        ),
    ]));
    let program = dummy_program(vec![tuple]);
    let ty = ValType::Type(TypeId::from_index(0));

    assert!(<(u32, u32) as LowerComponent>::matches_type(&ty, &program).is_err());
}

#[test]
fn get_typed_func_reports_signature_mismatch() {
    let bytes = wat::parse_str(
        r#"
(component
  (type (func (param "value" u32)))
  (import "host" (func (type 0)))
  (export "f" (func 0))
)
"#,
    )
    .unwrap();
    let engine = ComponentEngine::new();
    let compiled = engine.compile(&bytes).unwrap();
    let store = telomere::Store::new();
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, _args| Ok(Vec::new()));
    let instance = block_on(engine.instantiate(&compiled, &store, &linker)).unwrap();
    let mismatch = instance.get_typed_func::<(u32, u32), ()>("f");
    assert!(mismatch.is_err());
}
