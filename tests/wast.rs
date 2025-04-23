use std::path::PathBuf;
mod common;
use common::run_wast;

fn run_test_file(name: &str) {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    d.push("tests/wasm-testsuite");
    d.push(format!("{name}.wast"));
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}

#[test]
fn int_literals() {
    run_test_file("int_literals");
}
#[test]
fn block() {
    run_test_file("block");
}
#[test]
fn call() {
    run_test_file("call");
}

#[test]
fn memory_grow() {
    run_test_file("memory_grow");
}

#[test]
fn call_indirect() {
    run_test_file("call_indirect");
}
#[test]
fn loop_() {
    run_test_file("loop");
}

#[test]
fn br_if() {
    run_test_file("br_if");
}

#[test]
fn const_() {
    run_test_file("const");
}

#[test]
fn nop() {
    run_test_file("nop");
}

#[test]
fn func() {
    run_test_file("func");
}

#[test]
fn br_table() {
    run_test_file("br_table");
}

#[test]
fn memory() {
    run_test_file("memory");
}

#[test]
fn if_() {
    run_test_file("if");
}
#[test]
fn address() {
    run_test_file("address");
}
#[test]
fn align() {
    run_test_file("align");
}
#[test]
fn memory_copy() {
    run_test_file("memory_copy");
}
#[test]
fn memory_fill() {
    run_test_file("memory_fill");
}
#[test]
fn memory_trap() {
    run_test_file("memory_trap");
}

#[test]
fn memory_redundancy() {
    run_test_file("memory_redundancy");
}

#[test]
fn memory_size() {
    run_test_file("memory_size");
}
#[test]
fn memory_init() {
    run_test_file("memory_init");
}
#[test]
fn imports() {
    run_test_file("imports");
}

#[test]
fn comments() {
    run_test_file("comments");
}
#[test]
fn conversions() {
    run_test_file("conversions");
}

#[test]
fn custom() {
    run_test_file("custom");
}
#[test]
fn data() {
    run_test_file("data");
}

#[test]
fn bulk() {
    run_test_file("bulk");
}

#[test]
fn elem() {
    run_test_file("elem");
}

#[test]
fn endianness() {
    run_test_file("endianness");
}
#[test]
fn exports() {
    run_test_file("exports");
}
#[test]
fn f32() {
    run_test_file("f32");
}
#[test]
fn f32_bitwise() {
    run_test_file("f32_bitwise");
}
#[test]
fn f32_cmp() {
    run_test_file("f32_cmp");
}
#[test]
fn f64() {
    run_test_file("f64");
}
#[test]
fn f64_bitwise() {
    run_test_file("f64_bitwise");
}
#[test]
fn f64_cmp() {
    run_test_file("f64_cmp");
}
#[test]
fn fac() {
    run_test_file("fac");
}

#[test]
fn float_exprs() {
    run_test_file("float_exprs");
}
#[test]
fn float_literals() {
    run_test_file("float_literals");
}
#[test]
fn float_memory() {
    run_test_file("float_memory");
}
#[test]
fn float_misc() {
    run_test_file("float_misc");
}
#[test]
fn forward() {
    run_test_file("forward");
}
#[test]
fn func_ptrs() {
    run_test_file("func_ptrs");
}
#[test]
fn global() {
    run_test_file("global");
}
#[test]
fn i32() {
    run_test_file("i32");
}
#[test]
fn i64() {
    run_test_file("i64");
}
#[test]
fn inline_module() {
    run_test_file("inline-module");
}
#[test]
fn int_exprs() {
    run_test_file("int_exprs");
}
/*
TODO: library bug?
#[test]
fn labels() {
    run_test_file("labels");
}*/
#[test]
fn left_to_right() {
    run_test_file("left-to-right");
}

#[test]
fn linking() {
    run_test_file("linking");
}

#[test]
fn load() {
    run_test_file("load");
}
#[test]
fn local_get() {
    tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).init();
    run_test_file("local_get");
}
#[test]
fn local_set() {
    run_test_file("local_set");
}
#[test]
fn local_tee() {
    run_test_file("local_tee");
}
/*
library limitation
#[test]
fn names() {
    run_test_file("names");
}
*/
#[test]
fn obsolete_keywords() {
    run_test_file("obsolete-keywords");
}
#[test]
fn ref_func() {
    run_test_file("ref_func");
}
#[test]
fn ref_is_null() {
    run_test_file("ref_is_null");
}
#[test]
fn ref_null() {
    run_test_file("ref_null");
}
#[test]
fn return_() {
    run_test_file("return");
}
#[test]
fn select() {
    run_test_file("select");
}
#[test]
fn skip_stack_guard_page() {
    run_test_file("skip-stack-guard-page");
}
#[test]
fn stack() {
    run_test_file("stack");
}
#[test]
fn start() {
    run_test_file("start");
}
#[test]
fn store() {
    run_test_file("store");
}
#[test]
fn switch() {
    run_test_file("switch");
}
#[test]
fn token() {
    run_test_file("token");
}
#[test]
fn traps() {
    run_test_file("traps");
}
#[test]
fn type_() {
    run_test_file("type");
}
#[test]
fn unreachable() {
    run_test_file("unreachable");
}

#[test]
fn unreached_invalid() {
    run_test_file("unreached-invalid");
}

#[test]
fn unreached_valid() {
    run_test_file("unreached-valid");
}
#[test]
fn unwind() {
    run_test_file("unwind");
}
#[test]
fn table() {
    run_test_file("table");
}
#[test]
fn table_copy() {
    run_test_file("table_copy");
}
#[test]
fn table_get() {
    run_test_file("table_get");
}
#[test]
fn table_set() {
    run_test_file("table_set");
}

#[test]
fn table_sub() {
    run_test_file("table-sub");
}

#[test]
fn table_init() {
    run_test_file("table_init");
}
#[test]
fn table_grow() {
    run_test_file("table_grow");
}
#[test]
fn table_size() {
    run_test_file("table_size");
}

#[test]
fn table_fill() {
    run_test_file("table_fill");
}
#[test]
fn binary() {
    run_test_file("binary");
}
#[test]
fn binary_leb128() {
    run_test_file("binary-leb128");
}

#[test]
fn utf8_custom_section_id() {
    run_test_file("utf8-custom-section-id");
}

#[test]
fn utf8_import_field() {
    run_test_file("utf8-import-field");
}

#[test]
fn utf8_import_module() {
    run_test_file("utf8-import-module");
}

#[test]
fn utf8_invalid_encoding() {
    run_test_file("utf8-invalid-encoding");
}
