//! Stock `wasm32-wasip1` command module: the preview1 runner's
//! stock-toolchain fixture (see `examples/README.md` for the rebuild
//! command and `docs/core/preview1-scope.md` for the scope it proves).
//!
//! The program deliberately stays inside the runner's supported surface:
//! argv, the environment, stdin/stdout, and the `HashMap` random seed
//! (which is what makes a stock binary import `random_get`). It never
//! touches the filesystem.

use std::collections::HashMap;
use std::io::Read;

fn main() {
    println!("stock wasip1 hello");

    let args: Vec<String> = std::env::args().collect();
    println!("argc={}", args.len());
    for arg in &args {
        println!("arg={arg}");
    }

    match std::env::var("TELOMERE_PREVIEW1_FIXTURE") {
        Ok(value) => println!("fixture-env={value}"),
        Err(_) => println!("fixture-env=<unset>"),
    }

    let mut map = HashMap::new();
    map.insert("answer", 42);
    println!("hashmap={}", map["answer"]);

    let mut input = String::new();
    let read = std::io::stdin().read_to_string(&mut input).unwrap_or(0);
    println!("stdin-len={read}");
}
