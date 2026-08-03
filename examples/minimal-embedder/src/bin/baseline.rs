use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("embed-baseline: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = input_path()?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
    println!("{}", bytes.len());
    Ok(())
}

fn input_path() -> Result<PathBuf, String> {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let path = args
        .next()
        .ok_or_else(|| format!("usage: {} <wasm-file>", PathBuf::from(program).display()))?;
    if args.next().is_some() {
        return Err("expected exactly one Wasm file".to_owned());
    }
    Ok(PathBuf::from(path))
}
