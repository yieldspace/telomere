use std::{
    future::Future,
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn main() {
    if let Err(error) = run() {
        eprintln!("embed-component: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let path = input_path()?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&bytes)
        .map_err(|error| format!("failed to compile '{}': {error}", path.display()))?;
    let linker = ComponentLinker::new();
    let store = telomere::Store::new();
    let result: Result<(), String> = block_on(async {
        let instance = engine
            .instantiate(&program, &store, &linker)
            .await
            .map_err(|error| format!("failed to instantiate '{}': {error}", path.display()))?;
        let values = instance
            .call(
                &store,
                "add",
                &[ComponentValue::S32(20), ComponentValue::S32(22)],
            )
            .await
            .map_err(|error| format!("failed to call add: {error}"))?;
        let expected = vec![ComponentValue::S32(42)];
        if values != expected {
            return Err(format!(
                "add(20, 22) returned {values:?}, expected {expected:?}"
            ));
        }
        Ok(())
    });
    result?;

    println!("42");
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

fn input_path() -> Result<PathBuf, String> {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let path = args.next().ok_or_else(|| {
        format!(
            "usage: {} <component-file>",
            PathBuf::from(program).display()
        )
    })?;
    if args.next().is_some() {
        return Err("expected exactly one component file".to_owned());
    }
    Ok(PathBuf::from(path))
}
