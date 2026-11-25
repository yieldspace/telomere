//! Hold N instances of one core module in a single process and report the
//! process memory high-water mark.
//!
//! This is the harness for the static-memory-ceiling measurement in
//! `docs/benchmarks/footprint.md` (#140): how many instances of a
//! representative module one embedder process can hold under a stated RAM
//! budget. It instantiates the module `count` times into one `Store`, keeps
//! every handle alive, then prints the instance count together with the
//! process `VmHWM`/`VmRSS` read from `/proc/self/status` (Linux). The
//! externally measured peak RSS (`/usr/bin/time -v` around the process) is
//! the primary figure; the in-process values are a cross-check.
//!
//! With the optional `call` argument each instance's exported `main(1, 2)`
//! is invoked once (and checked to return `3`) before the report, so the
//! measurement covers instances that have actually executed, not only
//! freshly instantiated ones.

use std::{
    future::Future,
    path::PathBuf,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};
use telomere::{IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmValue};

fn main() {
    if let Err(error) = run() {
        eprintln!("embed-instances: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let (path, count, call) = input_args()?;
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("failed to read '{}': {error}", path.display()))?;
    let mut reader = IoReadBinaryReader::from(&bytes[..]);
    let mut parser = WasmParser::new(&mut reader);
    let module = parser
        .parse_module()
        .map_err(|error| format!("failed to parse '{}': {error}", path.display()))?;

    let store = Store::new();
    let registry = Registry::new();
    block_on(async {
        let mut instances = Vec::new();
        for index in 0..count {
            match telomere::instantiate(module.clone(), &store, &registry).await {
                VMResult::Success(instance) => instances.push(instance),
                failure => {
                    return Err(format!(
                        "failed to instantiate '{}' (instance {} of {count}): {failure:?}",
                        path.display(),
                        index + 1
                    ))
                }
            }
        }
        if call {
            for (index, instance) in instances.iter().enumerate() {
                let values = match telomere::run_module_function(
                    instance,
                    &store,
                    "main",
                    &ResultValue::new(vec![WasmValue::I32(1), WasmValue::I32(2)]),
                )
                .await
                {
                    VMResult::Success(values) => values,
                    failure => {
                        return Err(format!(
                            "failed to call main on instance {} of {count}: {failure:?}",
                            index + 1
                        ))
                    }
                };
                if values != ResultValue::new(vec![WasmValue::I32(3)]) {
                    return Err(format!(
                        "main(1, 2) on instance {} returned {values:?}, expected 3",
                        index + 1
                    ));
                }
            }
        }
        let held = instances.len();
        let (vmhwm_kib, vmrss_kib) = proc_self_memory_kib();
        println!(
            "instances={held} vmhwm_kib={} vmrss_kib={}",
            vmhwm_kib
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_owned()),
            vmrss_kib
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unavailable".to_owned())
        );
        Ok(())
    })
}

fn proc_self_memory_kib() -> (Option<u64>, Option<u64>) {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return (None, None);
    };
    let mut vmhwm = None;
    let mut vmrss = None;
    for line in status.lines() {
        if let Some((key, rest)) = line.split_once(':') {
            let value = rest
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse::<u64>()
                .ok();
            match key {
                "VmHWM" => vmhwm = value,
                "VmRSS" => vmrss = value,
                _ => {}
            }
        }
    }
    (vmhwm, vmrss)
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

fn input_args() -> Result<(PathBuf, usize, bool), String> {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let usage = || {
        format!(
            "usage: {} <core-wasm-file> <instance-count> [call]",
            PathBuf::from(&program).display()
        )
    };
    let path = args.next().ok_or_else(usage)?;
    let count_arg = args.next().ok_or_else(usage)?;
    let call = match args.next() {
        None => false,
        Some(flag) if flag == "call" => true,
        Some(_) => return Err(usage()),
    };
    if args.next().is_some() {
        return Err(usage());
    }
    let count = count_arg
        .to_string_lossy()
        .parse::<usize>()
        .map_err(|_| usage())?;
    Ok((PathBuf::from(path), count, call))
}
