use telomere::{IoReadBinaryReader, WasmParser};

/// Fuzz-found crash input for #344: the optimizer's GVN caching pass
/// rewrote a block after the analysis had recorded a local set/get
/// coalescing pair, and the transform then applied the stale pair cursor
/// to the rewritten block, panicking with "analysis emitted an invalid
/// coalescing pair" in `make_coalesced_tee`.
const COALESCING_PAIR_STALE_CURSOR: &[u8] =
    include_bytes!("../../../fuzz/regressions/parse_core_module/coalescing-pair-stale-cursor.bin");

#[test]
fn parse_core_module_coalescing_pair_stale_cursor() {
    let input = COALESCING_PAIR_STALE_CURSOR.to_vec();
    // Mirror the parse_core_module fuzz target's entry point and stack size.
    let joined = std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let mut reader = IoReadBinaryReader::from(&input[..]);
            let mut parser = WasmParser::new(&mut reader);
            parser.parse_module()
        })
        .expect("spawning parser thread")
        .join();
    let result = joined.expect("the parser must not panic on the #344 crash input");
    match result {
        Ok(_) => {}
        Err(err) => panic!("the #344 crash input parses cleanly after the fix: {err}"),
    }
}
