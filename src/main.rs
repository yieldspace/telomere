use clap::Parser;
use cli::{Cli, Command};
use std::process::ExitCode;
use telomere::WasmValue;

mod cli;
mod component_cli;

/// The main entry point of the application.
///
/// # Returns
///
/// * `anyhow::Result<()>` - Returns `Ok(())` if the program executes successfully, or an error otherwise.
#[tokio::main]
async fn main() -> ExitCode {
    match try_main().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::FAILURE
        }
    }
}

async fn try_main() -> anyhow::Result<ExitCode> {
    let args = Cli::parse();

    if let Some(Command::Component(command)) = args.command {
        return component_cli::run(command).await;
    }

    run_core_module(args).await?;
    Ok(ExitCode::SUCCESS)
}

async fn run_core_module(args: Cli) -> anyhow::Result<()> {
    let name = args
        .name
        .ok_or_else(|| anyhow::anyhow!("module path is required"))?;
    let func = args
        .func
        .ok_or_else(|| anyhow::anyhow!("function name is required"))?;

    let module = {
        let bytes = std::fs::read(&name)?;

        let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);

        let mut parser = telomere::WasmParser::new(&mut reader);
        parser.parse_module()?
    };

    let mut store = telomere::Store::new();

    let registry = telomere::Registry::new();

    let instance = telomere::instantiate(module, &mut store, &registry)
        .await
        .unwrap();

    let wasm_args = telomere::ResultValue::new(
        args.args
            .iter()
            .map(|x| telomere::WasmValue::I32(*x))
            .collect(),
    );

    let ret = telomere::run_module_function(&instance, &mut store, &func, &wasm_args)
        .await
        .unwrap();

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

    println!("{ret}");
    Ok(())
}
