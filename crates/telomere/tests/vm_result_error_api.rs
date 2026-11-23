//! End-to-end coverage for the #143 error surface.
//!
//! A trapping guest call must let the embedder answer "plugin X trapped in
//! function Y because Z" from public API alone:
//!
//! - the returned [`VMResult`] distinguishes the spec trap causes that used
//!   to collapse into `InvalidOperand` (divide by zero, integer overflow,
//!   invalid conversion),
//! - its [`Display`](std::fmt::Display) text is the spec's trap message, and
//!   both `VMResult` and [`TrapKind`] implement [`std::error::Error`],
//! - [`Store::take_last_trap`] reports the faulting function by index and,
//!   when a name section exists, by name, with the matching [`TrapKind`],
//! - a trap raised by a host function keeps its cause across the host-call
//!   boundary and is attributed to the host frame.

mod common;

use common::instantiate_wat;
use telomere::{
    host_abi::{ExecuteContext, Instr},
    link_host_function_with_function_idx, run_module_function, Registry, ResultValue, Store,
    TrapFrameKind, TrapKind, VMResult,
};

const CAUSES_WAT: &str = r#"
(module $causes
  (func $div_zero (export "div_zero") i32.const 1 i32.const 0 i32.div_s drop)
  (func $div_overflow (export "div_overflow") i32.const -2147483648 i32.const -1 i32.div_s drop)
  (func $rem_zero (export "rem_zero") i32.const 7 i32.const 0 i32.rem_u drop)
  (func $bad_conversion (export "bad_conversion") f32.const nan i32.trunc_f32_s drop)
  (func $unreachable (export "unreachable") unreachable))
"#;

fn assert_std_error<E: std::error::Error>(_: &E) {}

/// (export name, expected VMResult, expected Display, expected TrapKind,
/// faulting funcidx, faulting function name)
type CauseCase = (
    &'static str,
    VMResult<ResultValue>,
    &'static str,
    TrapKind,
    u32,
    &'static str,
);

#[tokio::test]
async fn trap_causes_are_distinguished_with_spec_messages_and_faulting_function() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(CAUSES_WAT, &store, &registry).await;

    // (export name, expected VMResult, expected Display, expected TrapKind,
    //  faulting funcidx, faulting function name)
    let cases: Vec<CauseCase> = vec![
        (
            "div_zero",
            VMResult::DivideByZero,
            "integer divide by zero",
            TrapKind::DivideByZero,
            0,
            "div_zero",
        ),
        (
            "div_overflow",
            VMResult::IntegerOverflow,
            "integer overflow",
            TrapKind::IntegerOverflow,
            1,
            "div_overflow",
        ),
        (
            "rem_zero",
            VMResult::DivideByZero,
            "integer divide by zero",
            TrapKind::DivideByZero,
            2,
            "rem_zero",
        ),
        (
            "bad_conversion",
            VMResult::InvalidConversion,
            "invalid conversion to integer",
            TrapKind::InvalidConversion,
            3,
            "bad_conversion",
        ),
        (
            "unreachable",
            VMResult::Unreachable,
            "unreachable",
            TrapKind::Unreachable,
            4,
            "unreachable",
        ),
    ];

    for (export, expected_result, expected_display, expected_kind, funcidx, func_name) in cases {
        let result =
            run_module_function(&instance, &store, export, &ResultValue::new(vec![])).await;
        assert_eq!(
            std::mem::discriminant(&result),
            std::mem::discriminant(&expected_result),
            "{export}: unexpected VMResult variant: {result:?}"
        );
        assert_eq!(result.to_string(), expected_display, "{export}: Display");

        let info = store
            .take_last_trap()
            .unwrap_or_else(|| panic!("{export}: trapping call must publish trap information"));
        assert_eq!(info.kind, expected_kind, "{export}: TrapInfo kind");
        let frame = info
            .frames
            .first()
            .unwrap_or_else(|| panic!("{export}: trap must retain the faulting frame"));
        assert_eq!(frame.funcidx, Some(funcidx), "{export}: faulting funcidx");
        assert_eq!(
            frame.func_name.as_deref(),
            Some(func_name),
            "{export}: faulting function name from the name section"
        );
        assert_eq!(frame.kind, TrapFrameKind::Wasm, "{export}: frame kind");
    }
}

#[tokio::test]
async fn error_surface_supports_std_error_flows() {
    // Compile-time: the public failure types are std errors.
    assert_std_error(&VMResult::<()>::DivideByZero);
    assert_std_error(&TrapKind::IntegerOverflow);

    // Runtime: they box and render through `dyn Error` unchanged.
    let boxed: Box<dyn std::error::Error> = Box::new(VMResult::<()>::InvalidConversion);
    assert_eq!(boxed.to_string(), "invalid conversion to integer");
    let boxed: Box<dyn std::error::Error> = Box::new(TrapKind::DivideByZero);
    assert_eq!(boxed.to_string(), "divide by zero");

    // A captured TrapInfo is itself an error whose rendering names the
    // cause and the faulting function.
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(CAUSES_WAT, &store, &registry).await;
    let result =
        run_module_function(&instance, &store, "div_zero", &ResultValue::new(vec![])).await;
    assert!(matches!(result, VMResult::DivideByZero));
    let info = store.take_last_trap().expect("trap info must be captured");
    let rendered = info.to_string();
    assert!(
        rendered.starts_with("trap: divide by zero"),
        "TrapInfo Display must lead with the cause: {rendered}"
    );
    assert!(
        rendered.contains("div_zero (func 0)"),
        "TrapInfo Display must name the faulting function: {rendered}"
    );
    let boxed: Box<dyn std::error::Error> = Box::new(info);
    assert_eq!(boxed.to_string(), rendered);
}

fn divide_by_zero_host(_ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    VMResult::DivideByZero
}

#[tokio::test]
async fn host_function_trap_cause_survives_the_host_boundary() {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate_wat(
        r#"
        (module $hosty
          (func $host)
          (func $run (export "run") call $host))
        "#,
        &store,
        &registry,
    )
    .await;
    link_host_function_with_function_idx(&instance, 0, divide_by_zero_host, &store);

    let result = run_module_function(&instance, &store, "run", &ResultValue::new(vec![])).await;
    assert!(
        matches!(result, VMResult::DivideByZero),
        "the host trap cause must reach the embedder unchanged: {result:?}"
    );

    let info = store
        .take_last_trap()
        .expect("host-raised trap must publish trap information");
    assert_eq!(info.kind, TrapKind::DivideByZero);
    let frame = info
        .frames
        .first()
        .expect("host-raised trap must retain the host frame");
    assert_eq!(frame.kind, TrapFrameKind::Host);
    assert_eq!(frame.funcidx, Some(0));
}
