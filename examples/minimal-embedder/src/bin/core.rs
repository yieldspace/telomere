use std::{
    future::Future,
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
use telomere::{IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmValue};

#[cfg(feature = "jit")]
use telomere::{JitConfig, RuntimeConfig};

fn main() {
    if let Err(error) = run() {
        eprintln!("embed-core: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = input_path()?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
    let mut reader = IoReadBinaryReader::from(&bytes[..]);
    let mut parser = WasmParser::new(&mut reader);
    let module = parser
        .parse_module()
        .map_err(|error| format!("failed to parse '{}': {error}", path.display()))?;

    let store = make_store()?;
    let registry = Registry::new();
    block_on(async {
        let instance = match telomere::instantiate(module, &store, &registry).await {
            VMResult::Success(instance) => instance,
            failure => {
                return Err(format!(
                    "failed to instantiate '{}': {failure:?}",
                    path.display()
                ))
            }
        };
        let values = match telomere::run_module_function(
            &instance,
            &store,
            "main",
            &ResultValue::new(vec![WasmValue::I32(1), WasmValue::I32(2)]),
        )
        .await
        {
            VMResult::Success(values) => values,
            failure => return Err(format!("failed to call main: {failure:?}")),
        };
        let expected = ResultValue::new(vec![WasmValue::I32(3)]);
        if values != expected {
            return Err(format!(
                "main(1, 2) returned {values:?}, expected {expected:?}"
            ));
        }

        Ok(())
    })?;

    #[cfg(feature = "jit")]
    assert_jit_compiled(&store)?;

    println!("3");
    Ok(())
}

#[cfg(feature = "jit")]
fn assert_jit_compiled(store: &Store) -> Result<(), String> {
    let compiled_functions = store.jit_cache_stats().compiled_functions;
    if compiled_functions == 0 {
        return Err("JIT was enabled but did not compile the core workload".to_owned());
    }
    Ok(())
}

struct ThreadWaker(std::thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn make_store() -> Result<Store, String> {
    #[cfg(feature = "jit")]
    {
        if !telomere::jit_supported() {
            return Err(
                "the jit feature requires a supported target: macOS arm64, macOS/Linux x86_64, or Linux-gnu riscv64gc"
                    .to_owned(),
            );
        }
        let mut runtime_config = RuntimeConfig::default();
        runtime_config.jit = JitConfig {
            enabled: true,
            ..JitConfig::default()
        };
        return Ok(Store::new_with_runtime_config(runtime_config));
    }

    #[cfg(not(feature = "jit"))]
    Ok(Store::new())
}

fn input_path() -> Result<PathBuf, String> {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let path = args.next().ok_or_else(|| {
        format!(
            "usage: {} <core-wasm-file>",
            PathBuf::from(program).display()
        )
    })?;
    if args.next().is_some() {
        return Err("expected exactly one core Wasm file".to_owned());
    }
    Ok(PathBuf::from(path))
}
