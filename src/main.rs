//! `telomere-cli`, the command-line runner for the telomere runtime.
//!
//! It has three entry points: calling an exported function of a core Wasm
//! module, running a core module as a WASI preview1 command, and running a WASI
//! 0.2 component that exports `wasi:cli/run@0.2.6`. Argument parsing lives in
//! [`cli`], the preview1 host functions in [`core_wasi_preview1`], and the
//! component host setup in [`component_cli`].

use anyhow::Context;
use clap::Parser;
use cli::{Cli, Command, CompileCommand, CoreCommand};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};
use telomere::{JitConfig, ResultValue, RuntimeConfig, WasmValue};

#[cfg(feature = "jit")]
mod aot_key;
mod cli;
mod component_cli;
mod core_wasi_preview1;

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
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let args = Cli::parse_from(raw_args.clone());

    #[cfg(feature = "measure-switches")]
    let optimizer_switch = telomere::measure_switches::resolve()?;

    match args.command {
        Some(Command::Component(command)) => return component_cli::run(command).await,
        Some(Command::Compile(command)) => return compile_module(command),
        #[cfg(feature = "measure-switches")]
        Some(Command::MeasureSwitchesProbe) => {
            println!(r#"{{"state":"{}"}}"#, optimizer_switch.as_str());
            return Ok(ExitCode::SUCCESS);
        }
        None => {}
    }

    let command = args.core_command(&raw_args)?;
    run_core_module(command).await
}

async fn run_core_module(command: CoreCommand) -> anyhow::Result<ExitCode> {
    run_core_module_with_stdio(command, core_wasi_preview1::StdioMode::Inherit).await
}

async fn run_core_module_with_stdio(
    command: CoreCommand,
    stdio: core_wasi_preview1::StdioMode,
) -> anyhow::Result<ExitCode> {
    let load_options = AotLoadOptions {
        verify_key: command.verify_key.clone(),
        trust_unverified_artifact: command.trust_unverified_artifact,
    };
    let (module, aot) = load_module_from_path(&command.name, &load_options)?;
    if should_run_wasi(&command, &module) {
        let environ =
            core_wasi_preview1::collect_guest_environ(!command.no_inherit_env, &command.env)?;
        return core_wasi_preview1::run(
            module,
            &command.name,
            &command.tail_args,
            environ,
            stdio,
            command.jit,
            command.jit_code_cache_mib,
            command.benchmark_recognizers,
            aot,
        )
        .await;
    }

    if command.args_after_separator {
        anyhow::bail!(
            "`{}` uses guest argv syntax (`-- ...`) but does not import `{}` with an exported `_start`",
            command.name.display(),
            core_wasi_preview1::PREVIEW1_MODULE
        );
    }
    let (func, wasm_args) = legacy_core_invocation(&command, &module)?;
    let ret = run_exported_core_function(
        module,
        &func,
        &wasm_args,
        command.jit,
        command.jit_code_cache_mib,
        command.benchmark_recognizers,
        aot,
    )
    .await?;
    println!("{}", format_result_values(&ret));
    Ok(ExitCode::SUCCESS)
}

/// Magic prefix of an AoT artifact (#198). Duplicated here so that
/// a build without the `jit` feature can still name what it was
/// given instead of failing with a wasm parse error.
const AOT_ARTIFACT_MAGIC: &[u8; 8] = b"TELOAOT1";

/// Magic prefix of a component bundle (#201). Duplicated here for
/// the same reason as [`AOT_ARTIFACT_MAGIC`]: the routing code in
/// `main` and `component_cli` must recognise a bundle in builds
/// without the `jit` feature (which gates the bundle API) so it can
/// name what it was given.
pub(crate) const COMPONENT_BUNDLE_MAGIC: &[u8; 8] = b"TELOBND1";

/// How the load path treats an AoT artifact (#199): the key to
/// verify a signature against, and whether unsigned artifacts are
/// trusted without verification.
#[derive(Default)]
struct AotLoadOptions {
    // Consumed by the `jit`-gated artifact loader; in other builds
    // the loader rejects artifacts before reading them.
    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    verify_key: Option<PathBuf>,
    #[cfg_attr(not(feature = "jit"), allow(dead_code))]
    trust_unverified_artifact: bool,
}

/// An opened AoT artifact carried alongside its module so the run
/// path can install it after instantiation. Infallible in builds
/// without the `jit` feature, where no artifact can be opened.
#[cfg(feature = "jit")]
type LoadedAotArtifact = telomere::AotArtifact;
#[cfg(not(feature = "jit"))]
type LoadedAotArtifact = std::convert::Infallible;

/// Loads a core module from `path`. AoT artifacts (#198) are opened
/// through the fail-closed loader under `options`; precompiled
/// artifacts (#196) are detected by magic and restored directly;
/// anything else goes through the wasm parser. Returns the module
/// plus the opened AoT artifact, if any, for installation after
/// instantiation.
fn load_module_from_path(
    path: &Path,
    options: &AotLoadOptions,
) -> anyhow::Result<(telomere::Module, Option<LoadedAotArtifact>)> {
    let bytes = fs::read(path)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to read module `{}`", path.display()))?;
    if bytes.starts_with(AOT_ARTIFACT_MAGIC) {
        return load_aot_artifact(path, &bytes, options);
    }
    if telomere::is_precompiled_module(&bytes) {
        let module = telomere::deserialize_module(&bytes)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("failed to load precompiled module `{}`", path.display()))?;
        return Ok((module, None));
    }
    let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);
    let mut parser = telomere::WasmParser::new(&mut reader);
    let module = parser
        .parse_module()
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to parse module `{}`", path.display()))?;
    Ok((module, None))
}

#[cfg(feature = "jit")]
fn load_aot_artifact(
    path: &Path,
    bytes: &[u8],
    options: &AotLoadOptions,
) -> anyhow::Result<(telomere::Module, Option<LoadedAotArtifact>)> {
    let verifier = match &options.verify_key {
        Some(key_path) => Some(aot_key::HmacSha256Key::load(key_path)?),
        None => None,
    };
    let policy = telomere::AotLoadPolicy {
        allow_unsigned: options.trust_unverified_artifact,
    };
    let opened = telomere::open_aot_artifact(
        bytes,
        verifier
            .as_ref()
            .map(|key| key as &dyn telomere::AotVerifier),
        policy,
    )
    .map_err(anyhow::Error::from)
    .with_context(|| format!("failed to load AoT artifact `{}`", path.display()))?;
    let module = opened.module().clone();
    Ok((module, Some(opened)))
}

#[cfg(not(feature = "jit"))]
fn load_aot_artifact(
    path: &Path,
    _bytes: &[u8],
    _options: &AotLoadOptions,
) -> anyhow::Result<(telomere::Module, Option<LoadedAotArtifact>)> {
    anyhow::bail!(
        "`{}` is a telomere AoT artifact; running it requires a telomere-cli build with `--features jit`",
        path.display()
    )
}

/// Installs an opened AoT artifact's native code for `instance`.
/// Runs after instantiation: installation binds the blobs to the
/// instance's actual functions.
#[cfg(feature = "jit")]
fn install_loaded_aot_artifact(
    store: &telomere::Store,
    instance: &telomere::InstanceHandle,
    artifact: &LoadedAotArtifact,
) -> anyhow::Result<()> {
    telomere::install_aot_artifact(store, instance, artifact)
        .map_err(anyhow::Error::from)
        .context("failed to install AoT artifact")?;
    Ok(())
}

#[cfg(not(feature = "jit"))]
fn install_loaded_aot_artifact(
    _store: &telomere::Store,
    _instance: &telomere::InstanceHandle,
    artifact: &LoadedAotArtifact,
) -> anyhow::Result<()> {
    match *artifact {}
}

/// Implements `telomere-cli compile MODULE [-o OUTPUT]`: parse a core
/// module and write its artifact, printing the per-phase serialize
/// timings. The default artifact is the #196 precompiled module;
/// `--aot` writes the #198 AoT artifact instead, and `--bundle`
/// packages a component into the #201 component bundle (see
/// [`compile_component_bundle`]). Compilation only ever parses: it
/// never instantiates the module, so guest code (including the
/// start function) never runs at compile time (#199).
///
/// With `--check`, nothing is compiled: the named artifact is
/// validated against this build instead (see [`check_artifact`]).
fn compile_module(command: CompileCommand) -> anyhow::Result<ExitCode> {
    if command.check {
        return check_artifact(&command);
    }
    let bytes = fs::read(&command.name)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to read module `{}`", command.name.display()))?;
    if telomere::is_precompiled_module(&bytes) {
        anyhow::bail!(
            "`{}` is already a precompiled artifact",
            command.name.display()
        );
    }
    if bytes.starts_with(AOT_ARTIFACT_MAGIC) {
        anyhow::bail!("`{}` is already an AoT artifact", command.name.display());
    }
    if bytes.starts_with(COMPONENT_BUNDLE_MAGIC) {
        anyhow::bail!("`{}` is already a component bundle", command.name.display());
    }
    if command.bundle {
        if !is_component_binary(&bytes) {
            anyhow::bail!(
                "`{}` is not a Wasm component; `--bundle` packages components — \
                 compile core modules without `--bundle`",
                command.name.display()
            );
        }
        return compile_component_bundle(&command, &bytes);
    }
    if is_component_binary(&bytes) {
        anyhow::bail!(
            "`{}` is a Wasm component; `compile` handles core modules only — \
             run components with `telomere component`, or package one into \
             an AoT bundle with `compile --bundle`",
            command.name.display()
        );
    }
    let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);
    let mut parser = telomere::WasmParser::new(&mut reader);
    let module = parser
        .parse_module()
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to parse module `{}`", command.name.display()))?;

    if command.aot {
        return compile_aot_artifact(&command, &module);
    }
    if command.sign_key.is_some() {
        anyhow::bail!(
            "`--sign-key` only applies with `--aot` or `--bundle` (precompiled artifacts are not signed)"
        );
    }

    let mut timings = telomere::PrecompiledTimings::default();
    let artifact = telomere::serialize_module_timed(&module, Some(&mut timings))
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to serialize module `{}`", command.name.display()))?;

    let output = command.output.clone().unwrap_or_else(|| {
        let mut path = command.name.clone().into_os_string();
        path.push(".tpre");
        PathBuf::from(path)
    });
    fs::write(&output, &artifact)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to write artifact `{}`", output.display()))?;
    println!(
        "compiled {} ({} bytes) -> {} ({} bytes) in {:.2} ms (sections {:.2} ms, functions {:.2} ms)",
        command.name.display(),
        bytes.len(),
        output.display(),
        artifact.len(),
        timings.total.as_secs_f64() * 1e3,
        timings.sections.as_secs_f64() * 1e3,
        timings.functions.as_secs_f64() * 1e3,
    );
    Ok(ExitCode::SUCCESS)
}

/// Whether `bytes` carry the component-model version marker
/// (version 0x0d) instead of the core module version (0x01).
fn is_component_binary(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[0..4] == b"\0asm" && bytes[4] == 0x0d && bytes[5] == 0x00
}

/// Resolves the signing key path for `compile --aot`: the flag
/// wins, and the `TELOMERE_SIGN_KEY` environment variable (a key
/// file path) is the fallback.
#[cfg(feature = "jit")]
fn sign_key_path(command: &CompileCommand) -> Option<PathBuf> {
    command
        .sign_key
        .clone()
        .or_else(|| std::env::var_os("TELOMERE_SIGN_KEY").map(PathBuf::from))
}

/// Implements `telomere-cli compile --aot MODULE [-o OUTPUT]`:
/// build the #198 AoT artifact from the parsed module, signed when
/// a key is configured and unsigned (developer mode) otherwise.
#[cfg(feature = "jit")]
fn compile_aot_artifact(
    command: &CompileCommand,
    module: &telomere::Module,
) -> anyhow::Result<ExitCode> {
    let key = match sign_key_path(command) {
        Some(path) => Some(aot_key::HmacSha256Key::load(&path)?),
        None => None,
    };
    let artifact = telomere::build_aot_artifact(
        module,
        key.as_ref().map(|key| key as &dyn telomere::AotSigner),
    )
    .map_err(anyhow::Error::from)
    .with_context(|| {
        format!(
            "failed to build AoT artifact for `{}`",
            command.name.display()
        )
    })?;
    let output = command.output.clone().unwrap_or_else(|| {
        let mut path = command.name.clone().into_os_string();
        path.push(".taot");
        PathBuf::from(path)
    });
    fs::write(&output, &artifact)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to write artifact `{}`", output.display()))?;
    match &key {
        Some(key) => println!(
            "compiled {} -> {} ({} bytes, AoT, signed with {})",
            command.name.display(),
            output.display(),
            artifact.len(),
            key.key_id(),
        ),
        None => println!(
            "compiled {} -> {} ({} bytes, AoT, UNSIGNED: developer mode only — \
             loaders refuse this artifact unless explicitly trusted)",
            command.name.display(),
            output.display(),
            artifact.len(),
        ),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "jit"))]
fn compile_aot_artifact(
    _command: &CompileCommand,
    _module: &telomere::Module,
) -> anyhow::Result<ExitCode> {
    anyhow::bail!("`compile --aot` requires a telomere-cli build with `--features jit`")
}

/// Implements `telomere-cli compile --bundle COMPONENT [-o OUTPUT]`:
/// package the component into the #201 bundle through the library
/// producer, signed when a key is configured and unsigned
/// (developer mode) otherwise. Like the other producers, this only
/// ever parses: the embedded core modules are never instantiated,
/// so guest code never runs at compile time.
#[cfg(feature = "jit")]
fn compile_component_bundle(command: &CompileCommand, bytes: &[u8]) -> anyhow::Result<ExitCode> {
    let key = match sign_key_path(command) {
        Some(path) => Some(aot_key::HmacSha256Key::load(&path)?),
        None => None,
    };
    let sections = telomere_component::bundle::enumerate_core_module_sections(bytes)
        .map_err(anyhow::Error::from)
        .with_context(|| {
            format!(
                "failed to enumerate core modules of `{}`",
                command.name.display()
            )
        })?;
    let bundle = telomere_component::build_component_bundle(
        bytes,
        key.as_ref().map(|key| key as &dyn telomere::AotSigner),
    )
    .map_err(anyhow::Error::from)
    .with_context(|| {
        format!(
            "failed to build component bundle for `{}`",
            command.name.display()
        )
    })?;
    let output = command.output.clone().unwrap_or_else(|| {
        let mut path = command.name.clone().into_os_string();
        path.push(".tbnd");
        PathBuf::from(path)
    });
    fs::write(&output, &bundle)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to write bundle `{}`", output.display()))?;
    match &key {
        Some(key) => println!(
            "compiled {} -> {} ({} bytes, component bundle, {} core module sections, signed with {})",
            command.name.display(),
            output.display(),
            bundle.len(),
            sections.len(),
            key.key_id(),
        ),
        None => println!(
            "compiled {} -> {} ({} bytes, component bundle, {} core module sections, UNSIGNED: developer mode only — \
             loaders refuse this bundle unless explicitly trusted)",
            command.name.display(),
            output.display(),
            bundle.len(),
            sections.len(),
        ),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(not(feature = "jit"))]
fn compile_component_bundle(_command: &CompileCommand, _bytes: &[u8]) -> anyhow::Result<ExitCode> {
    anyhow::bail!("`compile --bundle` requires a telomere-cli build with `--features jit`")
}

/// Implements `telomere-cli compile --check ARTIFACT`: validates an
/// existing artifact against this build without executing or
/// installing anything. A matching artifact prints `ok:` and exits
/// successfully; a stale or corrupt one prints the typed rejection
/// reason and exits with a failure status.
fn check_artifact(command: &CompileCommand) -> anyhow::Result<ExitCode> {
    let bytes = fs::read(&command.name)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to read artifact `{}`", command.name.display()))?;
    if bytes.starts_with(AOT_ARTIFACT_MAGIC) {
        return check_aot_artifact(command, &bytes);
    }
    if bytes.starts_with(COMPONENT_BUNDLE_MAGIC) {
        return check_component_bundle(command, &bytes);
    }
    if telomere::is_precompiled_module(&bytes) {
        return match telomere::deserialize_module(&bytes) {
            Ok(_) => {
                println!(
                    "ok: `{}` is a precompiled artifact matching this build",
                    command.name.display()
                );
                Ok(ExitCode::SUCCESS)
            }
            Err(error) => {
                eprintln!("rejected: `{}`: {error}", command.name.display());
                Ok(ExitCode::FAILURE)
            }
        };
    }
    eprintln!(
        "rejected: `{}` is not a telomere artifact (neither precompiled, AoT, nor a component bundle)",
        command.name.display()
    );
    Ok(ExitCode::FAILURE)
}

#[cfg(feature = "jit")]
fn check_aot_artifact(command: &CompileCommand, bytes: &[u8]) -> anyhow::Result<ExitCode> {
    let verifier = match &command.verify_key {
        Some(path) => Some(aot_key::HmacSha256Key::load(path)?),
        None => None,
    };
    // Checking never installs or executes, so an unsigned artifact
    // is validated on fingerprint grounds and reported as unsigned;
    // the trust decision belongs to the load path. A signed
    // artifact still requires its key: a signature that cannot be
    // verified is a rejection (typed `AotError::NoVerifier`).
    let policy = telomere::AotLoadPolicy {
        allow_unsigned: true,
    };
    match telomere::open_aot_artifact(
        bytes,
        verifier
            .as_ref()
            .map(|key| key as &dyn telomere::AotVerifier),
        policy,
    ) {
        Ok(opened) => {
            let signature = if opened.is_unsigned() {
                "unsigned".to_owned()
            } else {
                format!("signed with {}", opened.key_id())
            };
            println!(
                "ok: `{}` is an AoT artifact matching this build ({signature}; fingerprint {})",
                command.name.display(),
                opened.fingerprint(),
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("rejected: `{}`: {error}", command.name.display());
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(not(feature = "jit"))]
fn check_aot_artifact(_command: &CompileCommand, _bytes: &[u8]) -> anyhow::Result<ExitCode> {
    anyhow::bail!("checking an AoT artifact requires a telomere-cli build with `--features jit`")
}

/// Implements `telomere-cli compile --check BUNDLE` for a component
/// bundle (#201): runs the fail-closed bundle loader (signature
/// posture, keyed-set pre-check, per-artifact fingerprint checks,
/// substitution decode) without instantiating or installing
/// anything. As with `--check` for AoT artifacts, an unsigned
/// bundle is validated structurally and reported as unsigned — the
/// trust decision belongs to the load path — while a signed bundle
/// still requires its key: a signature that cannot be verified is
/// a rejection (typed `BundleError::NoVerifier`).
#[cfg(feature = "jit")]
fn check_component_bundle(command: &CompileCommand, bytes: &[u8]) -> anyhow::Result<ExitCode> {
    let verifier = match &command.verify_key {
        Some(path) => Some(aot_key::HmacSha256Key::load(path)?),
        None => None,
    };
    let policy = telomere::AotLoadPolicy {
        allow_unsigned: true,
    };
    let engine = telomere_component::ComponentEngine::new();
    match engine.compile_bundle(
        bytes,
        verifier
            .as_ref()
            .map(|key| key as &dyn telomere::AotVerifier),
        policy,
    ) {
        Ok(loaded) => {
            // The reported signature state comes from the bundle
            // itself, as in `check_aot_artifact`: supplying a key
            // does not make an unsigned bundle signed.
            let signature = if loaded.is_unsigned() {
                "unsigned".to_owned()
            } else {
                format!("signed with {}", loaded.key_id())
            };
            let report = loaded.report();
            println!(
                "ok: `{}` is a component bundle matching this build ({}; {} core module sections, {} substituted)",
                command.name.display(),
                signature,
                report.core_module_sections,
                report.substituted_ordinals.len(),
            );
            Ok(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprintln!("rejected: `{}`: {error}", command.name.display());
            Ok(ExitCode::FAILURE)
        }
    }
}

#[cfg(not(feature = "jit"))]
fn check_component_bundle(_command: &CompileCommand, _bytes: &[u8]) -> anyhow::Result<ExitCode> {
    anyhow::bail!("checking a component bundle requires a telomere-cli build with `--features jit`")
}

fn should_run_wasi(command: &CoreCommand, module: &telomere::Module) -> bool {
    core_wasi_preview1::is_preview1_command_module(module)
        && (command.args_after_separator || command.tail_args.is_empty())
}

fn legacy_core_invocation(
    command: &CoreCommand,
    module: &telomere::Module,
) -> anyhow::Result<(String, ResultValue)> {
    let Some((func, arg_values)) = command.tail_args.split_first() else {
        if core_wasi_preview1::is_preview1_command_module(module) {
            anyhow::bail!(
                "use `-- ...` to pass guest argv to the preview1 module `{}`",
                command.name.display()
            );
        }
        anyhow::bail!(
            "function name is required unless `{}` imports `{}` and exports `_start`",
            command.name.display(),
            core_wasi_preview1::PREVIEW1_MODULE
        );
    };

    let wasm_args = arg_values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.parse::<i32>().map(WasmValue::I32).map_err(|error| {
                anyhow::anyhow!(
                    "legacy core argument {} (`{}`) must be an i32: {error}",
                    index + 1,
                    value
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((func.clone(), ResultValue::new(wasm_args)))
}

async fn run_exported_core_function(
    module: telomere::Module,
    func: &str,
    wasm_args: &ResultValue,
    jit: bool,
    jit_code_cache_mib: u32,
    benchmark_recognizers: bool,
    aot: Option<LoadedAotArtifact>,
) -> anyhow::Result<ResultValue> {
    if jit && !telomere::jit_supported() {
        anyhow::bail!(
            "`--jit` requires the `jit` feature on a supported target: macOS arm64, macOS/Linux x86_64, or Linux-gnu riscv64gc"
        );
    }
    if aot.is_some() && !telomere::jit_supported() {
        anyhow::bail!(
            "running an AoT artifact requires native execution on a supported target: macOS arm64, macOS/Linux x86_64, or Linux-gnu riscv64gc"
        );
    }
    // An AoT artifact requests native execution by itself: its blobs
    // are only entered when the store enables it (the same posture
    // as the wast AoT lane).
    let native = jit || aot.is_some();
    let store = if native || benchmark_recognizers {
        let mut runtime_config = RuntimeConfig::default();
        runtime_config.jit = JitConfig { enabled: native };
        runtime_config.limits.max_code_cache_bytes = jit_code_cache_mib.saturating_mul(1024 * 1024);
        runtime_config.recognizers.benchmark_shaped = benchmark_recognizers;
        telomere::Store::new_with_runtime_config(runtime_config)
    } else {
        telomere::Store::new()
    };
    let registry = telomere::Registry::new();
    let instance = vm_result_to_anyhow(
        telomere::instantiate(module, &store, &registry).await,
        "failed to instantiate core module",
    )?;
    if let Some(artifact) = &aot {
        install_loaded_aot_artifact(&store, &instance, artifact)?;
    }
    vm_result_to_anyhow(
        telomere::run_module_function(&instance, &store, func, wasm_args).await,
        format!("failed to invoke exported function `{func}`"),
    )
}

fn format_result_values(ret: &ResultValue) -> String {
    ret.iter()
        .map(|value| match value {
            WasmValue::I32(value) => value.to_string(),
            WasmValue::I64(value) => value.to_string(),
            WasmValue::F32(value) => value.to_string(),
            WasmValue::F64(value) => value.to_string(),
            WasmValue::V128(value) => value.to_string(),
            WasmValue::FuncRef(value) => value.to_string(),
            WasmValue::ExternRef(value) => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn vm_result_to_anyhow<T>(
    result: telomere::VMResult<T>,
    context: impl Into<String>,
) -> anyhow::Result<T> {
    let context = context.into();
    match result {
        telomere::VMResult::Success(value) => Ok(value),
        telomere::VMResult::Unreachable => Err(anyhow::anyhow!("{context}: unreachable")),
        telomere::VMResult::StackOverflow => Err(anyhow::anyhow!("{context}: stack overflow")),
        telomere::VMResult::MemoryIndexOutOfRange => {
            Err(anyhow::anyhow!("{context}: memory index out of range"))
        }
        telomere::VMResult::TableIndexOutOfRange => {
            Err(anyhow::anyhow!("{context}: table index out of range"))
        }
        telomere::VMResult::CallIndirectInvalidType => {
            Err(anyhow::anyhow!("{context}: call_indirect invalid type"))
        }
        telomere::VMResult::TableUninitialized => {
            Err(anyhow::anyhow!("{context}: table uninitialized"))
        }
        telomere::VMResult::Unlinkable => Err(anyhow::anyhow!("{context}: unlinkable")),
        telomere::VMResult::MemoryAllocationFailed => {
            Err(anyhow::anyhow!("{context}: memory allocation failed"))
        }
        telomere::VMResult::InvalidOperand => Err(anyhow::anyhow!("{context}: invalid operand")),
        telomere::VMResult::DivideByZero => {
            Err(anyhow::anyhow!("{context}: integer divide by zero"))
        }
        telomere::VMResult::IntegerOverflow => Err(anyhow::anyhow!("{context}: integer overflow")),
        telomere::VMResult::InvalidConversion => {
            Err(anyhow::anyhow!("{context}: invalid conversion to integer"))
        }
        telomere::VMResult::UnalignedAtomic => Err(anyhow::anyhow!("{context}: unaligned atomic")),
        telomere::VMResult::Unimplemented => Err(anyhow::anyhow!("{context}: unimplemented")),
        telomere::VMResult::FuelExhausted => {
            Err(anyhow::anyhow!("{context}: interrupted: fuel exhausted"))
        }
        telomere::VMResult::Cancelled => Err(anyhow::anyhow!("{context}: interrupted: cancelled")),
        telomere::VMResult::ResourceLimitExceeded => {
            Err(anyhow::anyhow!("{context}: resource limit exceeded"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(stem: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("telomere-cli-{stem}-{nonce}.wasm"))
    }

    fn example_path(file: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(file)
    }

    /// Run the committed preview1 example with `args` and return its stdout.
    async fn run_preview1_hello(args: Vec<String>) -> String {
        let path = example_path("wasi-preview1-hello.wasm");
        let command = CoreCommand {
            name: path,
            tail_args: args,
            args_after_separator: true,
            jit: false,
            jit_code_cache_mib: 4,
            benchmark_recognizers: false,
            env: Vec::new(),
            no_inherit_env: false,
            verify_key: None,
            trust_unverified_artifact: false,
        };
        let captured = core_wasi_preview1::CapturedStdio::default();

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("the preview1 example should run");
        assert_eq!(exit, ExitCode::SUCCESS);

        String::from_utf8(captured.stdout()).expect("the example only writes utf-8")
    }

    /// Run the committed stock `wasm32-wasip1` fixture with captured stdio
    /// and return its stdout. The fixture's environment comes from this
    /// process's environment, like a real CLI invocation.
    async fn run_preview1_stock(args: Vec<String>, stdin: Vec<u8>) -> String {
        let path = example_path("wasi-preview1-stock-hello.wasm");
        let command = CoreCommand {
            name: path,
            tail_args: args,
            args_after_separator: true,
            jit: false,
            jit_code_cache_mib: 4,
            benchmark_recognizers: false,
            env: Vec::new(),
            no_inherit_env: false,
            verify_key: None,
            trust_unverified_artifact: false,
        };
        let captured = core_wasi_preview1::CapturedStdio::with_stdin(stdin);

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("the stock preview1 fixture should run");
        assert_eq!(exit, ExitCode::SUCCESS);

        String::from_utf8(captured.stdout()).expect("the fixture only writes utf-8")
    }

    fn write_wasm(stem: &str, wat: &str) -> std::path::PathBuf {
        let path = unique_temp_path(stem);
        let bytes = wat::parse_str(wat).expect("wat must parse");
        fs::write(&path, bytes).expect("wasm must be writable");
        path
    }

    const PREVIEW1_GREETING: &str = "hello from telomere (wasi preview1)";
    const PREVIEW1_ARGV0: &str = "wasi-preview1-hello.wasm";

    #[tokio::test]
    async fn preview1_example_echoes_the_documented_argv() {
        let stdout = run_preview1_hello(vec!["one".to_owned(), "two".to_owned()]).await;

        assert_eq!(
            stdout,
            format!("{PREVIEW1_GREETING}\n{PREVIEW1_ARGV0}\none\ntwo\n")
        );
    }

    /// The stock-toolchain fixture (a plain Rust `wasm32-wasip1` binary, not
    /// hand-written WAT) runs end to end: it reads argv and the environment,
    /// seeds a `HashMap` (which imports `random_get`), echoes stdin's length
    /// through `fd_read`, and exits 0. See docs/core/preview1-scope.md.
    #[tokio::test]
    async fn preview1_stock_wasip1_fixture_runs_end_to_end() {
        std::env::set_var("TELOMERE_PREVIEW1_FIXTURE", "present");
        let stdout = run_preview1_stock(vec!["one".to_owned()], b"abc".to_vec()).await;
        std::env::remove_var("TELOMERE_PREVIEW1_FIXTURE");

        assert_eq!(
            stdout,
            "stock wasip1 hello\nargc=2\narg=wasi-preview1-stock-hello.wasm\narg=one\nfixture-env=present\nhashmap=42\nstdin-len=3\n"
        );
    }

    /// The example used to place the argv pointer array at a fixed 64 and the
    /// string buffer at a fixed 256, so 49 arguments made the pointer array run
    /// into the strings. It now sizes both regions from `args_sizes_get`.
    #[tokio::test]
    async fn preview1_example_survives_a_pointer_array_longer_than_the_old_fixed_slot() {
        let args: Vec<String> = (1..=64).map(|index| format!("a{index}")).collect();
        let stdout = run_preview1_hello(args.clone()).await;

        let mut expected = vec![PREVIEW1_GREETING.to_owned(), PREVIEW1_ARGV0.to_owned()];
        expected.extend(args);
        assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
    }

    /// The example used to hold argv strings in a fixed 768-byte window that sat
    /// below the static newline at 1088, so ~832 bytes of arguments overwrote
    /// the separator. The buffer is now sized by `argv_buf_size`, and the static
    /// data all lives below the argv region.
    #[tokio::test]
    async fn preview1_example_survives_argv_longer_than_the_old_fixed_buffer() {
        let args: Vec<String> = (0..11).map(|_| "x".repeat(80)).collect();
        let stdout = run_preview1_hello(args.clone()).await;

        let mut expected = vec![PREVIEW1_GREETING.to_owned(), PREVIEW1_ARGV0.to_owned()];
        expected.extend(args);
        assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
    }

    /// argv larger than the example's one declared page has to make the guest
    /// grow linear memory rather than write out of bounds.
    #[tokio::test]
    async fn preview1_example_grows_memory_for_argv_larger_than_one_page() {
        let args: Vec<String> = (0..900).map(|_| "y".repeat(100)).collect();
        let stdout = run_preview1_hello(args.clone()).await;

        let mut expected = vec![PREVIEW1_GREETING.to_owned(), PREVIEW1_ARGV0.to_owned()];
        expected.extend(args);
        assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
    }

    #[tokio::test]
    async fn core_runner_errors_when_function_is_missing_for_non_wasi_module() {
        let path = write_wasm(
            "non-wasi",
            r#"
            (module
              (func (export "main") (result i32) (i32.const 42)))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
            benchmark_recognizers: false,
            env: Vec::new(),
            no_inherit_env: false,
            verify_key: None,
            trust_unverified_artifact: false,
        };

        let error = run_core_module(command)
            .await
            .expect_err("missing function name must fail");
        assert!(error.to_string().contains("function name is required"));

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_runner_auto_detects_preview1_start() {
        let path = write_wasm(
            "preview1",
            r#"
            (module
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (func (export "_start")
                i32.const 3
                call $proc_exit))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
            benchmark_recognizers: false,
            env: Vec::new(),
            no_inherit_env: false,
            verify_key: None,
            trust_unverified_artifact: false,
        };

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(core_wasi_preview1::CapturedStdio::default()),
        )
        .await
        .expect("preview1 auto detection should succeed");
        assert_eq!(exit, ExitCode::from(3));

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_wasi_preview1_call_indirect_stdout_survives_optimizer() {
        let path = write_wasm(
            "preview1-indirect-stdout",
            r#"
            (module
              (type $fdwrite (func (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (type $fdwrite)))
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (table 1 funcref)
              (elem (i32.const 0) $write_via_import)
              (memory 1)
              (data (i32.const 0) "\08\00\00\00\0f\00\00\00")
              (data (i32.const 8) "hello indirect\n")
              (func $write_via_import (type $fdwrite)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                call $fd_write)
              (func (export "_start")
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 32
                i32.const 0
                call_indirect (type $fdwrite)
                drop
                i32.const 0
                call $proc_exit))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
            benchmark_recognizers: false,
            env: Vec::new(),
            no_inherit_env: false,
            verify_key: None,
            trust_unverified_artifact: false,
        };
        let captured = core_wasi_preview1::CapturedStdio::default();

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("preview1 indirect stdout path should succeed");

        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(captured.stdout(), b"hello indirect\n");

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_wasi_preview1_call_indirect_index_loaded_from_memory_survives_optimizer() {
        let path = write_wasm(
            "preview1-indirect-memory-index",
            r#"
            (module
              (type $fdwrite (func (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (type $fdwrite)))
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (table 1 funcref)
              (elem (i32.const 0) $write_via_import)
              (memory 1)
              (data (i32.const 0) "\08\00\00\00\0f\00\00\00")
              (data (i32.const 8) "hello indirect\n")
              (func $write_via_import (type $fdwrite)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                call $fd_write)
              (func (export "_start")
                i32.const 64
                i32.const 0
                i32.store
                i32.const 64
                i32.const 1
                i32.store
                i32.const 64
                i32.const 0
                i32.store
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 32
                i32.const 64
                i32.load
                call_indirect (type $fdwrite)
                drop
                i32.const 0
                call $proc_exit))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
            benchmark_recognizers: false,
            env: Vec::new(),
            no_inherit_env: false,
            verify_key: None,
            trust_unverified_artifact: false,
        };
        let captured = core_wasi_preview1::CapturedStdio::default();

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("preview1 indirect memory index path should succeed");

        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(captured.stdout(), b"hello indirect\n");

        let _ = fs::remove_file(path);
    }
}
