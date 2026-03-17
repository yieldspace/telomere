use crate::cli::ComponentCommand;
use anyhow::{anyhow, Context};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use telomere_component::{ComponentEngine, ComponentLinker};
use telomere_component_wasi::{add_to_linker_sync, bindings, WasiState};

pub async fn run(command: ComponentCommand) -> anyhow::Result<ExitCode> {
    let path = command.name.clone();
    let bytes = std::fs::read(&path)
        .with_context(|| format!("failed to read component `{}`", path.display()))?;
    run_component_bytes(&bytes, &path, &command).await
}

async fn run_component_bytes(
    bytes: &[u8],
    path: &Path,
    command: &ComponentCommand,
) -> anyhow::Result<ExitCode> {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(bytes)
        .with_context(|| format!("failed to compile component `{}`", path.display()))?;

    let state = build_wasi_state(path, command)?;
    let mut linker = ComponentLinker::new();
    add_to_linker_sync(&mut linker, state.clone())
        .context("failed to register WASI imports into component linker")?;

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .with_context(|| format!("failed to instantiate component `{}`", path.display()))?;

    let exports = bindings::Exports::new(instance);
    let result = exports.wasi_cli_run().run(&store).await;
    if let Some(code) = state.exit_code() {
        return Ok(ExitCode::from(code));
    }
    let result = result.context("failed to invoke `wasi:cli/run.run`")?;

    Ok(match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => ExitCode::from(1),
    })
}

fn build_wasi_state(path: &Path, command: &ComponentCommand) -> anyhow::Result<WasiState> {
    let env = collect_env(command)?;
    let guest_args = guest_args(path, &command.args);

    let mut builder = WasiState::builder()
        .args(guest_args)
        .env(env)
        .inherit_stdio()
        .inherit_stdin();
    for spec in &command.preopens {
        let (host, guest) = parse_preopen(spec)?;
        builder = builder.preopen_dir(host, guest);
    }
    Ok(builder.build())
}

fn guest_args(path: &Path, args: &[String]) -> Vec<String> {
    let arg0 = path
        .file_name()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string());
    std::iter::once(arg0).chain(args.iter().cloned()).collect()
}

fn collect_env(command: &ComponentCommand) -> anyhow::Result<Vec<(String, String)>> {
    let mut values = Vec::new();
    if !command.no_inherit_env {
        values.extend(std::env::vars());
    }
    for spec in &command.env {
        let (key, value) = parse_env(spec)?;
        if let Some((_, existing)) = values.iter_mut().find(|(existing, _)| existing == &key) {
            *existing = value;
        } else {
            values.push((key, value));
        }
    }
    Ok(values)
}

fn parse_env(spec: &str) -> anyhow::Result<(String, String)> {
    let (key, value) = spec
        .split_once('=')
        .ok_or_else(|| anyhow!("invalid --env `{spec}`: expected KEY=VALUE"))?;
    if key.is_empty() {
        return Err(anyhow!("invalid --env `{spec}`: key must not be empty"));
    }
    Ok((key.to_owned(), value.to_owned()))
}

fn parse_preopen(spec: &str) -> anyhow::Result<(PathBuf, String)> {
    if spec.is_empty() {
        return Err(anyhow!("invalid --dir: value must not be empty"));
    }
    if let Some(separator) = preopen_guest_separator(spec) {
        let (host, guest) = spec.split_at(separator);
        let guest = &guest[1..];
        if host.is_empty() || guest.is_empty() {
            return Err(anyhow!(
                "invalid --dir `{spec}`: expected HOST[:GUEST] with non-empty parts"
            ));
        }
        return Ok((PathBuf::from(host), guest.to_owned()));
    }
    Ok((PathBuf::from(spec), ".".to_owned()))
}

fn preopen_guest_separator(spec: &str) -> Option<usize> {
    let start = windows_drive_prefix_len(spec);
    spec[start..].rfind(':').map(|offset| start + offset)
}

fn windows_drive_prefix_len(spec: &str) -> usize {
    let bytes = spec.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> ComponentCommand {
        ComponentCommand {
            name: PathBuf::from("guest.wasm"),
            preopens: Vec::new(),
            env: Vec::new(),
            no_inherit_env: true,
            args: vec!["one".to_owned(), "two".to_owned()],
        }
    }

    #[test]
    fn guest_args_use_component_filename_as_argv0() {
        assert_eq!(
            guest_args(
                Path::new("/tmp/guest.wasm"),
                &["one".to_owned(), "two".to_owned()]
            ),
            vec!["guest.wasm".to_owned(), "one".to_owned(), "two".to_owned()]
        );
    }

    #[test]
    fn parse_env_requires_key_value_pair() {
        let error = parse_env("FOO").expect_err("invalid env must fail");
        assert!(error.to_string().contains("KEY=VALUE"));
    }

    #[test]
    fn parse_preopen_defaults_guest_path_to_dot() {
        let (host, guest) = parse_preopen("/tmp/data").expect("preopen should parse");
        assert_eq!(host, PathBuf::from("/tmp/data"));
        assert_eq!(guest, ".");
    }

    #[test]
    fn parse_preopen_preserves_windows_drive_path_without_guest_path() {
        let (host, guest) = parse_preopen(r"C:\tmp").expect("preopen should parse");
        assert_eq!(host, PathBuf::from(r"C:\tmp"));
        assert_eq!(guest, ".");
    }

    #[test]
    fn parse_preopen_supports_windows_drive_path_with_guest_path() {
        let (host, guest) =
            parse_preopen(r"C:\tmp:sandbox").expect("preopen should parse with guest path");
        assert_eq!(host, PathBuf::from(r"C:\tmp"));
        assert_eq!(guest, "sandbox");
    }

    #[tokio::test]
    async fn component_runner_maps_payloadless_result_to_exit_code() {
        let bytes = wat::parse_str(command_component_wat(0)).expect("component wat must parse");
        let exit = run_component_bytes(&bytes, Path::new("guest.component.wasm"), &command())
            .await
            .expect("component run should succeed");
        assert_eq!(exit, ExitCode::SUCCESS);

        let bytes = wat::parse_str(command_component_wat(1)).expect("component wat must parse");
        let exit = run_component_bytes(&bytes, Path::new("guest.component.wasm"), &command())
            .await
            .expect("component run should return guest failure");
        assert_eq!(exit, ExitCode::from(1));
    }

    fn command_component_wat(discriminant: i32) -> String {
        format!(
            r#"
(component
  (type $run-result (result))
  (type $run-func (func (result $run-result)))
  (type $run-instance (instance (export "run" (func (type $run-func)))))

  (core module $run-core
    (func (export "run") (result i32)
      i32.const {discriminant})
  )
  (core instance $run-core-inst (instantiate $run-core))
  (func $run (type $run-func)
    (canon lift (core func $run-core-inst "run")))

  (instance $run-export
    (export "run" (func $run))
  )
  (export "wasi:cli/run@0.2.6" (instance $run-export))
)
"#
        )
    }
}
