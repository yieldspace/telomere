use std::path::PathBuf;
mod common;
use common::run_wast;
use tokio::test;

fn resolve_test_file(name: &str) -> PathBuf {
    let suite_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wasm-testsuite");
    assert!(
        suite_dir.is_dir(),
        "missing wasm testsuite submodule at {}. Run `git submodule update --init --recursive` from the repository root.",
        suite_dir.display()
    );

    let path = suite_dir.join(format!("{name}.wast"));
    assert!(
        path.is_file(),
        "missing WAST fixture {} in {}. Verify the wasm testsuite submodule is checked out at the pinned commit.",
        name,
        suite_dir.display()
    );

    path
}

async fn run_test_file(name: &str) {
    let path = resolve_test_file(name);
    let wast = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    run_wast(&wast).await;
}

#[test]
async fn int_literals() {
    run_test_file("int_literals").await;
}
#[test]
async fn block() {
    run_test_file("block").await;
}
#[test]
async fn call() {
    run_test_file("call").await;
}

#[test]
async fn memory_grow() {
    run_test_file("memory_grow").await;
}

#[test]
async fn call_indirect() {
    run_test_file("call_indirect").await;
}
#[test]
async fn loop_() {
    run_test_file("loop").await;
}

#[test]
async fn br_if() {
    run_test_file("br_if").await;
}

#[test]
async fn const_() {
    run_test_file("const").await;
}

#[test]
async fn nop() {
    run_test_file("nop").await;
}

#[test]
async fn func() {
    run_test_file("func").await;
}

#[test]
async fn br_table() {
    run_test_file("br_table").await;
}

#[test]
async fn memory() {
    run_test_file("memory").await;
}

#[test]
async fn if_() {
    run_test_file("if").await;
}
#[test]
async fn address() {
    run_test_file("address").await;
}
#[test]
async fn align() {
    run_test_file("align").await;
}
#[test]
async fn memory_copy() {
    run_test_file("memory_copy").await;
}
#[test]
async fn memory_fill() {
    run_test_file("memory_fill").await;
}
#[test]
async fn memory_trap() {
    run_test_file("memory_trap").await;
}

#[test]
async fn memory_redundancy() {
    run_test_file("memory_redundancy").await;
}

#[test]
async fn memory_size() {
    run_test_file("memory_size").await;
}
#[test]
async fn memory_init() {
    run_test_file("memory_init").await;
}
#[test]
async fn imports() {
    run_test_file("imports").await;
}

#[test]
async fn comments() {
    run_test_file("comments").await;
}
#[test]
async fn conversions() {
    run_test_file("conversions").await;
}

#[test]
async fn custom() {
    run_test_file("custom").await;
}
#[test]
async fn data() {
    run_test_file("data").await;
}

#[test]
async fn bulk() {
    run_test_file("bulk").await;
}

#[test]
async fn elem() {
    run_test_file("elem").await;
}

#[test]
async fn endianness() {
    run_test_file("endianness").await;
}
#[test]
async fn exports() {
    run_test_file("exports").await;
}
#[test]
async fn f32() {
    run_test_file("f32").await;
}
#[test]
async fn f32_bitwise() {
    run_test_file("f32_bitwise").await;
}
#[test]
async fn f32_cmp() {
    run_test_file("f32_cmp").await;
}
#[test]
async fn f64() {
    run_test_file("f64").await;
}
#[test]
async fn f64_bitwise() {
    run_test_file("f64_bitwise").await;
}
#[test]
async fn f64_cmp() {
    run_test_file("f64_cmp").await;
}
#[test]
async fn fac() {
    run_test_file("fac").await;
}

#[test]
async fn float_exprs() {
    run_test_file("float_exprs").await;
}
#[test]
async fn float_literals() {
    run_test_file("float_literals").await;
}
#[test]
async fn float_memory() {
    run_test_file("float_memory").await;
}
#[test]
async fn float_misc() {
    run_test_file("float_misc").await;
}
#[test]
async fn forward() {
    run_test_file("forward").await;
}
#[test]
async fn func_ptrs() {
    run_test_file("func_ptrs").await;
}
#[test]
async fn global() {
    run_test_file("global").await;
}
#[test]
async fn i32() {
    run_test_file("i32").await;
}
#[test]
async fn i64() {
    run_test_file("i64").await;
}
#[test]
async fn inline_module() {
    run_test_file("inline-module").await;
}
#[test]
async fn int_exprs() {
    run_test_file("int_exprs").await;
}
/*
TODO: library bug?
#[test]
fn labels() {
    run_test_file("labels");
}*/
#[test]
async fn left_to_right() {
    run_test_file("left-to-right").await;
}

#[test]
async fn linking() {
    run_test_file("linking").await;
}

#[test]
async fn load() {
    run_test_file("load").await;
}
#[test]
async fn local_get() {
    run_test_file("local_get").await;
}
#[test]
async fn local_set() {
    run_test_file("local_set").await;
}
#[test]
async fn local_tee() {
    run_test_file("local_tee").await;
}
/*
library limitation
#[test]
fn names() {
    run_test_file("names");
}
*/
#[test]
async fn obsolete_keywords() {
    run_test_file("obsolete-keywords").await;
}
#[test]
async fn ref_func() {
    run_test_file("ref_func").await;
}
#[test]
async fn ref_is_null() {
    run_test_file("ref_is_null").await;
}
#[test]
async fn ref_null() {
    run_test_file("ref_null").await;
}
#[test]
async fn return_() {
    run_test_file("return").await;
}
#[test]
async fn select() {
    run_test_file("select").await;
}
#[test]
async fn skip_stack_guard_page() {
    run_test_file("skip-stack-guard-page").await;
}
#[test]
async fn stack() {
    run_test_file("stack").await;
}
#[test]
async fn start() {
    run_test_file("start").await;
}
#[test]
async fn store() {
    run_test_file("store").await;
}
#[test]
async fn switch() {
    run_test_file("switch").await;
}
#[test]
async fn token() {
    run_test_file("token").await;
}
#[test]
async fn traps() {
    run_test_file("traps").await;
}
#[test]
async fn type_() {
    run_test_file("type").await;
}
#[test]
async fn unreachable() {
    run_test_file("unreachable").await;
}

#[test]
async fn unreached_invalid() {
    run_test_file("unreached-invalid").await;
}

#[test]
async fn unreached_valid() {
    run_test_file("unreached-valid").await;
}
#[test]
async fn unwind() {
    run_test_file("unwind").await;
}
#[test]
async fn table() {
    run_test_file("table").await;
}
#[test]
async fn table_copy() {
    run_test_file("table_copy").await;
}
#[test]
async fn table_get() {
    run_test_file("table_get").await;
}
#[test]
async fn table_set() {
    run_test_file("table_set").await;
}

#[test]
async fn table_sub() {
    run_test_file("table-sub").await;
}

#[test]
async fn table_init() {
    run_test_file("table_init").await;
}
#[test]
async fn table_grow() {
    run_test_file("table_grow").await;
}
#[test]
async fn table_size() {
    run_test_file("table_size").await;
}

#[test]
async fn table_fill() {
    run_test_file("table_fill").await;
}
#[test]
async fn binary() {
    run_test_file("binary").await;
}
#[test]
async fn binary_leb128() {
    run_test_file("binary-leb128").await;
}

#[test]
async fn utf8_custom_section_id() {
    run_test_file("utf8-custom-section-id").await;
}

#[test]
async fn utf8_import_field() {
    run_test_file("utf8-import-field").await;
}

#[test]
async fn utf8_import_module() {
    run_test_file("utf8-import-module").await;
}

#[test]
async fn utf8_invalid_encoding() {
    run_test_file("utf8-invalid-encoding").await;
}

#[test]
async fn proposals_return_call() {
    run_test_file("proposals/tail-call/return_call").await;
}
#[test]
async fn proposals_return_call_indirect() {
    run_test_file("proposals/tail-call/return_call_indirect").await;
}

#[test]
async fn simd_load() {
    run_test_file("simd_load").await;
}
#[test]
async fn simd_const() {
    run_test_file("simd_const").await;
}
#[test]
async fn simd_address() {
    run_test_file("simd_address").await;
}

#[test]
async fn simd_align() {
    run_test_file("simd_align").await;
}
#[test]
async fn simd_bit_shift() {
    run_test_file("simd_bit_shift").await;
}
#[test]
async fn simd_bitwise() {
    run_test_file("simd_bitwise").await;
}
#[test]
async fn simd_boolean() {
    run_test_file("simd_boolean").await;
}
#[test]
async fn simd_conversions() {
    run_test_file("simd_conversions").await;
}
#[test]
async fn simd_f32x4_arith() {
    run_test_file("simd_f32x4_arith").await;
}
#[test]
async fn simd_f32x4_cmp() {
    run_test_file("simd_f32x4_cmp").await;
}

#[test]
async fn simd_f32x4_pmin_pmax() {
    run_test_file("simd_f32x4_pmin_pmax").await;
}

#[test]
async fn simd_f32x4_rounding() {
    run_test_file("simd_f32x4_rounding").await;
}
#[test]
async fn simd_f32x4() {
    run_test_file("simd_f32x4").await;
}
#[test]
async fn simd_f64x2_arith() {
    run_test_file("simd_f64x2_arith").await;
}
#[test]
async fn simd_f64x2_cmp() {
    run_test_file("simd_f64x2_cmp").await;
}
#[test]
async fn simd_f64x2_pmin_pmax() {
    run_test_file("simd_f64x2_pmin_pmax").await;
}
#[test]
async fn simd_f64x2_rounding() {
    run_test_file("simd_f64x2_rounding").await;
}
#[test]
async fn simd_f64x2() {
    run_test_file("simd_f64x2").await;
}
#[test]
async fn simd_i8x16_arith() {
    run_test_file("simd_i8x16_arith").await;
}
#[test]
async fn simd_i8x16_arith2() {
    run_test_file("simd_i8x16_arith2").await;
}
#[test]
async fn simd_i8x16_cmp() {
    run_test_file("simd_i8x16_cmp").await;
}
#[test]
async fn simd_i8x16_sat_arith() {
    run_test_file("simd_i8x16_sat_arith").await;
}
#[test]
async fn simd_i16x8_arith() {
    run_test_file("simd_i16x8_arith").await;
}
#[test]
async fn simd_i16x8_arith2() {
    run_test_file("simd_i16x8_arith2").await;
}
#[test]
async fn simd_i16x8_cmp() {
    run_test_file("simd_i16x8_cmp").await;
}
#[test]
async fn simd_i16x8_extadd_pairwise_i8x16() {
    run_test_file("simd_i16x8_extadd_pairwise_i8x16").await;
}
#[test]
async fn simd_i16x8_q15mulr_sat_s() {
    run_test_file("simd_i16x8_q15mulr_sat_s").await;
}
#[test]
async fn simd_i16x8_extmul_i8x16() {
    run_test_file("simd_i16x8_extmul_i8x16").await;
}
#[test]
async fn simd_i16x8_sat_arith() {
    run_test_file("simd_i16x8_sat_arith").await;
}
#[test]
async fn simd_i32x4_arith() {
    run_test_file("simd_i32x4_arith").await;
}
#[test]
async fn simd_i32x4_arith2() {
    run_test_file("simd_i32x4_arith2").await;
}
#[test]
async fn simd_i32x4_cmp() {
    run_test_file("simd_i32x4_cmp").await;
}
#[test]
async fn simd_i32x4_dot_i16x8() {
    run_test_file("simd_i32x4_dot_i16x8").await;
}
#[test]
async fn simd_i32x4_extadd_pairwise_i16x8() {
    run_test_file("simd_i32x4_extadd_pairwise_i16x8").await;
}

#[test]
async fn simd_32x4_extmul_i16x8() {
    run_test_file("simd_i32x4_extmul_i16x8").await;
}
#[test]
async fn simd_i32x4_trunc_sat_f32x4() {
    run_test_file("simd_i32x4_trunc_sat_f32x4").await;
}
#[test]
async fn simd_i32x4_trunc_sat_f64x2() {
    run_test_file("simd_i32x4_trunc_sat_f64x2").await;
}
#[test]
async fn simd_i64x2_arith() {
    run_test_file("simd_i64x2_arith").await;
}
#[test]
async fn simd_i64x2_arith2() {
    run_test_file("simd_i64x2_arith2").await;
}
#[test]
async fn simd_i64x2_cmp() {
    run_test_file("simd_i64x2_cmp").await;
}
#[test]
async fn simd_i64x2_extmul_i32x4() {
    run_test_file("simd_i64x2_extmul_i32x4").await;
}
#[test]
async fn simd_int_to_int_extend() {
    run_test_file("simd_int_to_int_extend").await;
}
#[test]
async fn simd_lane() {
    run_test_file("simd_lane").await;
}
#[test]
async fn simd_linking() {
    run_test_file("simd_linking").await;
}
#[test]
async fn simd_load8_lane() {
    run_test_file("simd_load8_lane").await;
}
#[test]
async fn simd_load16_lane() {
    run_test_file("simd_load16_lane").await;
}
#[test]
async fn simd_load32_lane() {
    run_test_file("simd_load32_lane").await;
}
#[test]
async fn simd_load64_lane() {
    run_test_file("simd_load64_lane").await;
}
#[test]
async fn simd_load_extend() {
    run_test_file("simd_load_extend").await;
}
#[test]
async fn simd_load_splat() {
    run_test_file("simd_load_splat").await;
}
#[test]
async fn simd_load_zero() {
    run_test_file("simd_load_zero").await;
}
#[test]
async fn simd_splat() {
    run_test_file("simd_splat").await;
}
#[test]
async fn simd_store() {
    run_test_file("simd_store").await;
}
#[test]
async fn simd_store8_lane() {
    run_test_file("simd_store8_lane").await;
}
#[test]
async fn simd_store16_lane() {
    run_test_file("simd_store16_lane").await;
}
#[test]
async fn simd_store32_lane() {
    run_test_file("simd_store32_lane").await;
}
#[test]
async fn simd_store64_lane() {
    run_test_file("simd_store64_lane").await;
}
