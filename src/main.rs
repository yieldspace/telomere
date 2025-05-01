use clap::Parser;
use cli::Cli;
use std::env::current_dir;
use telomere::WasmValue;

mod cli;

/// The main entry point of the application.
///
/// # Returns
///
/// * `anyhow::Result<()>` - Returns `Ok(())` if the program executes successfully, or an error otherwise.
fn main() -> anyhow::Result<()> {
    // Parse command-line arguments using the `Cli` struct.
    let args = Cli::parse();

    // Get the current working directory and append the provided module name to the path.
    let path = current_dir()?.join(args.name);

    // Load and parse the WebAssembly module.
    let module = {
        // Read the module file into a byte array.
        let bytes = std::fs::read(path)?;

        // Create a binary reader for the module bytes.
        let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);

        // Parse the WebAssembly module using the `WasmParser`.
        let mut parser = telomere::WasmParser::new(&mut reader);
        parser.parse_module()?
    };

    // Create a new WebAssembly store to manage module state.
    let mut store = telomere::Store::new();

    // Create a new registry for managing imports and exports.
    let registry = telomere::Registry::new();

    // Instantiate the WebAssembly module with the store and registry.
    let instance = telomere::instantiate(module, &mut store, &registry).unwrap();

    // Prepare the arguments for the WebAssembly function as `WasmValue`s.
    let wasm_args = telomere::ResultValue::new(
        args.args
            .iter()
            .map(|x| telomere::WasmValue::I32(*x))
            .collect(),
    );

    // Run the specified WebAssembly function with the provided arguments.
    let ret = telomere::run_module_function(&instance, &mut store, &args.func, &wasm_args).unwrap();

    // Convert the return values from the WebAssembly function to strings.
    let ret = ret
        .iter()
        .map(|x| match x {
            WasmValue::I32(value) => value.to_string(),
            WasmValue::I64(value) => value.to_string(),
            WasmValue::F32(value) => value.to_string(),
            WasmValue::F64(value) => value.to_string(),
            WasmValue::V128(value) => value.to_string(),
            WasmValue::FuncRef(value) => value.to_string(),
            WasmValue::ExternRef(value) => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ");

    // Print the return values to the console.
    println!("{ret}");

    // Indicate successful execution.
    Ok(())
}
