use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Run WebAssembly core modules and WASI 0.2 components on the telomere runtime.",
    long_about = "Run WebAssembly core modules and WASI 0.2 components on the telomere runtime.

Without a subcommand, MODULE is a core Wasm module. Pass an export name followed \
by i32 arguments to call that export, or pass `--` to run the module as a WASI \
preview1 command with the following arguments as guest argv.

Use the `component` subcommand for WASI 0.2 components that export \
`wasi:cli/run` at a version semver-compatible with the bundled WASI 0.2.6 \
bindings (for example `wasi:cli/run@0.2.0`).",
    args_conflicts_with_subcommands = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Execute the core module through the experimental baseline JIT.
    ///
    /// Requires a build with `--features jit` on a supported target.
    #[arg(long = "jit", default_value_t = false)]
    pub jit: bool,

    /// Upper bound, in MiB, on the JIT code cache.
    #[arg(long = "jit-code-cache-mib", value_name = "MIB", default_value_t = 4)]
    pub jit_code_cache_mib: u32,

    /// Verify a signed AoT artifact against the key in this file
    /// (HMAC-SHA256, #199). Required to run an artifact produced by
    /// `compile --aot --sign-key`; the artifact's key id must match
    /// the id derived from this file.
    #[arg(long = "verify-key", value_name = "PATH")]
    pub verify_key: Option<PathBuf>,

    /// Run an unsigned AoT artifact without any verification
    /// (#199). This is an explicit developer-mode opt-in: an
    /// unsigned artifact is arbitrary native code supplied by
    /// whoever wrote the file, so never pass this for artifacts
    /// from an untrusted source.
    #[arg(long = "trust-unverified-artifact", default_value_t = false)]
    pub trust_unverified_artifact: bool,

    /// Reconnect the quarantined benchmark-shaped recognizers (#188).
    ///
    /// Measurement/evaluation only: the instantiate-time rewrites shaped
    /// after specific CoreMark functions stay disconnected unless this flag
    /// is passed. See `docs/core/superinstruction-selection.md`.
    #[arg(long = "benchmark-recognizers", default_value_t = false)]
    pub benchmark_recognizers: bool,

    /// Set an environment variable for a preview1 command guest.
    /// Repeatable; only used when the module runs as a preview1 command.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Do not pass the host environment through to a preview1 command guest.
    #[arg(long = "no-inherit-env", default_value_t = false)]
    pub no_inherit_env: bool,

    /// Path to the core Wasm module to run.
    #[arg(value_name = "MODULE")]
    pub name: Option<PathBuf>,

    /// Export name and i32 arguments, or guest argv when placed after `--`.
    #[arg(value_name = "ARG")]
    pub args: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a WASI 0.2 component that exports `wasi:cli/run` (semver-compatible versions accepted).
    Component(ComponentCommand),
    /// Compile a core Wasm module into a precompiled artifact (#196).
    ///
    /// The artifact skips parsing and optimization when this exact build
    /// loads it later (any command that takes a core module path accepts
    /// the artifact transparently). Artifacts are pinned to the exact
    /// build that produced them: a different telomere build rejects
    /// them with a fingerprint error instead of loading them.
    ///
    /// With `--bundle` the input is a Wasm component instead, packaged
    /// into a component bundle (#201) that the `component` subcommand
    /// loads transparently.
    Compile(CompileCommand),
    /// Print the resolved measurement-only optimizer switch state as JSON.
    #[cfg(feature = "measure-switches")]
    MeasureSwitchesProbe,
}

#[derive(Args, Debug, Clone)]
pub struct CompileCommand {
    /// Path to the core Wasm module to compile, the component to
    /// package when `--bundle` is passed, or the artifact to
    /// validate when `--check` is passed.
    #[arg(value_name = "MODULE")]
    pub name: PathBuf,

    /// Where to write the artifact.
    ///
    /// Defaults to the module path with `.tpre` appended (`.taot`
    /// for `--aot`, `.tbnd` for `--bundle`).
    #[arg(long = "output", short = 'o', value_name = "OUTPUT")]
    pub output: Option<PathBuf>,

    /// Emit an AoT artifact (signed native-code bundle, #198)
    /// instead of a precompiled module (#196).
    ///
    /// Requires a build with `--features jit`. The artifact's
    /// native code is pinned to the exact build that produced it.
    #[arg(long = "aot", default_value_t = false)]
    pub aot: bool,

    /// Package a Wasm component into a component bundle (#201)
    /// instead of compiling a core module: the bundle carries the
    /// component binary plus an AoT artifact for each embedded core
    /// module, keyed by section hash, and the `component` subcommand
    /// loads it transparently.
    ///
    /// Requires a build with `--features jit`. `--sign-key` signs
    /// the bundle as a whole; without a key the bundle is unsigned,
    /// which loaders refuse unless explicitly trusted.
    #[arg(long = "bundle", default_value_t = false, conflicts_with = "aot")]
    pub bundle: bool,

    /// Sign the AoT artifact (or, with `--bundle`, the component
    /// bundle) with the key in this file (HMAC-SHA256, #199). The
    /// environment variable `TELOMERE_SIGN_KEY` names the same kind
    /// of file when the flag is absent. Without a key the artifact
    /// is unsigned, which loaders refuse unless explicitly trusted.
    #[arg(long = "sign-key", value_name = "PATH")]
    pub sign_key: Option<PathBuf>,

    /// Verify a signed artifact being `--check`ed against the key
    /// in this file (HMAC-SHA256, #199).
    #[arg(long = "verify-key", value_name = "PATH")]
    pub verify_key: Option<PathBuf>,

    /// Validate an existing artifact against this build instead of
    /// compiling: exit status is success when the artifact matches
    /// this build, failure with the typed rejection reason
    /// otherwise. Nothing is executed or installed.
    #[arg(
        long = "check",
        default_value_t = false,
        conflicts_with_all = ["output", "aot", "bundle", "sign_key"]
    )]
    pub check: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ComponentCommand {
    /// Path to the component to run, or to a component bundle
    /// produced by `compile --bundle` (detected by its magic and
    /// loaded through the bundle path).
    #[arg(value_name = "COMPONENT")]
    pub name: PathBuf,

    /// Verify a signed component bundle against the key in this
    /// file (HMAC-SHA256, #199). Required to run a bundle produced
    /// by `compile --bundle --sign-key`; the bundle's key id must
    /// match the id derived from this file. Plain components do
    /// not consult it.
    #[arg(long = "verify-key", value_name = "PATH")]
    pub verify_key: Option<PathBuf>,

    /// Run an unsigned component bundle without any verification
    /// (#201). This is an explicit developer-mode opt-in: an
    /// unsigned bundle is arbitrary native code supplied by
    /// whoever wrote the file, so never pass this for bundles
    /// from an untrusted source. Plain components do not consult
    /// it.
    #[arg(long = "trust-unverified-artifact", default_value_t = false)]
    pub trust_unverified_artifact: bool,

    /// Grant the guest a host directory, optionally under a different guest path.
    ///
    /// Repeatable. Without `:GUEST` the directory is preopened as `.`.
    #[arg(long = "dir", value_name = "HOST[:GUEST]")]
    pub preopens: Vec<String>,

    /// Set an environment variable for the guest. Repeatable.
    ///
    /// Overrides an inherited variable with the same key.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    /// Do not pass the host environment through to the guest.
    #[arg(long = "no-inherit-env", default_value_t = false)]
    pub no_inherit_env: bool,

    /// Guest argv, placed after `--`. `argv[0]` is the component file name.
    #[arg(value_name = "ARG")]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreCommand {
    pub name: PathBuf,
    pub tail_args: Vec<String>,
    pub args_after_separator: bool,
    pub jit: bool,
    pub jit_code_cache_mib: u32,
    pub benchmark_recognizers: bool,
    pub env: Vec<String>,
    pub no_inherit_env: bool,
    pub verify_key: Option<PathBuf>,
    pub trust_unverified_artifact: bool,
}

impl Cli {
    pub fn core_command(&self, raw_args: &[OsString]) -> Result<CoreCommand> {
        let name = self
            .name
            .clone()
            .ok_or_else(|| anyhow!("module path is required"))?;

        Ok(CoreCommand {
            name,
            tail_args: self.args.clone(),
            args_after_separator: has_separator(raw_args),
            jit: self.jit,
            jit_code_cache_mib: self.jit_code_cache_mib,
            benchmark_recognizers: self.benchmark_recognizers,
            env: self.env.clone(),
            no_inherit_env: self.no_inherit_env,
            verify_key: self.verify_key.clone(),
            trust_unverified_artifact: self.trust_unverified_artifact,
        })
    }
}

fn has_separator(raw_args: &[OsString]) -> bool {
    raw_args.iter().skip(1).any(|arg| arg == "--")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn bare_invocation_shows_usage_instead_of_a_missing_module_error() {
        let error = Cli::try_parse_from(["telomere-cli"])
            .expect_err("a bare invocation must not parse into a runnable command");

        // `arg_required_else_help` makes clap render the help text itself, so
        // the CLI never reaches `core_command` and never reports
        // `module path is required`.
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert_eq!(error.exit_code(), 2);
        assert!(
            error.use_stderr(),
            "the usage shown for a bare invocation goes to stderr, not stdout"
        );

        let rendered = error.render().to_string();
        assert!(
            rendered.contains("Usage: telomere-cli"),
            "usage line missing from:\n{rendered}"
        );
        assert!(
            !rendered.contains("module path is required"),
            "the old missing-module error must not resurface:\n{rendered}"
        );
    }

    #[test]
    fn core_command_still_reports_a_missing_module_path() {
        let cli = Cli::try_parse_from(["telomere-cli", "--jit"])
            .expect("a flag-only invocation parses; the module check happens later");
        let raw = vec![OsString::from("telomere-cli"), OsString::from("--jit")];

        let error = cli
            .core_command(&raw)
            .expect_err("a core command without a module path must fail");
        assert_eq!(error.to_string(), "module path is required");
    }

    #[test]
    fn parses_legacy_core_module_invocation() {
        let raw = vec![
            OsString::from("telomere-cli"),
            OsString::from("examples/add.wasm"),
            OsString::from("main"),
            OsString::from("1"),
            OsString::from("2"),
        ];
        let cli = Cli::try_parse_from(raw.clone()).expect("legacy invocation should parse");
        let core = cli
            .core_command(&raw)
            .expect("legacy core command should build");

        assert!(cli.command.is_none());
        assert!(!cli.jit);
        assert_eq!(cli.name, Some(PathBuf::from("examples/add.wasm")));
        assert_eq!(
            core,
            CoreCommand {
                name: PathBuf::from("examples/add.wasm"),
                tail_args: vec!["main".to_owned(), "1".to_owned(), "2".to_owned()],
                args_after_separator: false,
                jit: false,
                jit_code_cache_mib: 4,
                benchmark_recognizers: false,
                env: Vec::new(),
                no_inherit_env: false,
                verify_key: None,
                trust_unverified_artifact: false,
            }
        );
    }

    #[test]
    fn parses_core_jit_flag() {
        let raw = vec![
            OsString::from("telomere-cli"),
            OsString::from("--jit"),
            OsString::from("examples/add.wasm"),
            OsString::from("main"),
            OsString::from("1"),
            OsString::from("2"),
        ];
        let cli = Cli::try_parse_from(raw.clone()).expect("jit flag should parse");
        let core = cli.core_command(&raw).expect("core command should build");

        assert!(cli.jit);
        assert!(core.jit);
        assert_eq!(core.jit_code_cache_mib, 4);
        assert_eq!(core.name, PathBuf::from("examples/add.wasm"));
    }

    #[test]
    fn parses_core_jit_code_cache_limit() {
        let raw = vec![
            OsString::from("telomere-cli"),
            OsString::from("--jit"),
            OsString::from("--jit-code-cache-mib"),
            OsString::from("8"),
            OsString::from("examples/add.wasm"),
            OsString::from("main"),
        ];
        let cli = Cli::try_parse_from(raw.clone()).expect("jit cache flag should parse");
        let core = cli.core_command(&raw).expect("core command should build");

        assert!(core.jit);
        assert_eq!(core.jit_code_cache_mib, 8);
    }

    #[test]
    fn parses_core_wasi_invocation_with_guest_args_after_separator() {
        let raw = vec![
            OsString::from("telomere-cli"),
            OsString::from("guest.wasm"),
            OsString::from("--"),
            OsString::from("one"),
            OsString::from("-flag"),
        ];
        let cli = Cli::try_parse_from(raw.clone()).expect("wasi invocation should parse");
        let core = cli.core_command(&raw).expect("core command should build");

        assert_eq!(
            core,
            CoreCommand {
                name: PathBuf::from("guest.wasm"),
                tail_args: vec!["one".to_owned(), "-flag".to_owned()],
                args_after_separator: true,
                jit: false,
                jit_code_cache_mib: 4,
                benchmark_recognizers: false,
                env: Vec::new(),
                no_inherit_env: false,
                verify_key: None,
                trust_unverified_artifact: false,
            }
        );
    }

    #[test]
    fn parses_component_subcommand() {
        let cli = Cli::try_parse_from([
            "telomere-cli",
            "component",
            "guest.wasm",
            "--env",
            "FOO=BAR",
            "--dir",
            ".:sandbox",
            "--",
            "one",
            "-flag",
        ])
        .expect("component invocation should parse");

        let Some(Command::Component(component)) = cli.command else {
            panic!("component subcommand should be selected");
        };
        assert_eq!(component.name, PathBuf::from("guest.wasm"));
        assert_eq!(component.env, vec!["FOO=BAR"]);
        assert_eq!(component.preopens, vec![".:sandbox"]);
        assert_eq!(component.args, vec!["one", "-flag"]);
    }

    #[test]
    fn parses_compile_aot_with_sign_key() {
        let cli = Cli::try_parse_from([
            "telomere-cli",
            "compile",
            "module.wasm",
            "--aot",
            "--sign-key",
            "key.bin",
            "-o",
            "module.taot",
        ])
        .expect("compile --aot invocation should parse");

        let Some(Command::Compile(compile)) = cli.command else {
            panic!("compile subcommand should be selected");
        };
        assert_eq!(compile.name, PathBuf::from("module.wasm"));
        assert!(compile.aot);
        assert!(!compile.check);
        assert_eq!(compile.sign_key, Some(PathBuf::from("key.bin")));
        assert_eq!(compile.output, Some(PathBuf::from("module.taot")));
    }

    #[test]
    fn parses_compile_check_with_verify_key() {
        let cli = Cli::try_parse_from([
            "telomere-cli",
            "compile",
            "--check",
            "module.taot",
            "--verify-key",
            "key.bin",
        ])
        .expect("compile --check invocation should parse");

        let Some(Command::Compile(compile)) = cli.command else {
            panic!("compile subcommand should be selected");
        };
        assert!(compile.check);
        assert_eq!(compile.name, PathBuf::from("module.taot"));
        assert_eq!(compile.verify_key, Some(PathBuf::from("key.bin")));
    }

    #[test]
    fn parses_compile_bundle_with_sign_key() {
        let cli = Cli::try_parse_from([
            "telomere-cli",
            "compile",
            "guest.wasm",
            "--bundle",
            "--sign-key",
            "key.bin",
            "-o",
            "guest.tbnd",
        ])
        .expect("compile --bundle invocation should parse");

        let Some(Command::Compile(compile)) = cli.command else {
            panic!("compile subcommand should be selected");
        };
        assert_eq!(compile.name, PathBuf::from("guest.wasm"));
        assert!(compile.bundle);
        assert!(!compile.aot);
        assert!(!compile.check);
        assert_eq!(compile.sign_key, Some(PathBuf::from("key.bin")));
        assert_eq!(compile.output, Some(PathBuf::from("guest.tbnd")));
    }

    #[test]
    fn compile_bundle_conflicts_with_aot_and_check() {
        let error =
            Cli::try_parse_from(["telomere-cli", "compile", "guest.wasm", "--bundle", "--aot"])
                .expect_err("--bundle must not combine with --aot");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);

        let error = Cli::try_parse_from([
            "telomere-cli",
            "compile",
            "guest.wasm",
            "--bundle",
            "--check",
        ])
        .expect_err("--bundle must not combine with --check");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn parses_component_bundle_load_policy_flags() {
        let cli = Cli::try_parse_from([
            "telomere-cli",
            "component",
            "guest.tbnd",
            "--verify-key",
            "key.bin",
            "--trust-unverified-artifact",
        ])
        .expect("component bundle invocation should parse");

        let Some(Command::Component(component)) = cli.command else {
            panic!("component subcommand should be selected");
        };
        assert_eq!(component.name, PathBuf::from("guest.tbnd"));
        assert_eq!(component.verify_key, Some(PathBuf::from("key.bin")));
        assert!(component.trust_unverified_artifact);
    }

    #[test]
    fn compile_check_conflicts_with_output() {
        let error = Cli::try_parse_from([
            "telomere-cli",
            "compile",
            "--check",
            "module.taot",
            "-o",
            "elsewhere.taot",
        ])
        .expect_err("--check must not accept an output path");
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn parses_core_artifact_load_policy_flags() {
        let raw = vec![
            OsString::from("telomere-cli"),
            OsString::from("--verify-key"),
            OsString::from("key.bin"),
            OsString::from("--trust-unverified-artifact"),
            OsString::from("module.taot"),
            OsString::from("main"),
            OsString::from("1"),
        ];
        let cli = Cli::try_parse_from(raw.clone()).expect("artifact invocation should parse");
        let core = cli.core_command(&raw).expect("core command should build");

        assert_eq!(core.verify_key, Some(PathBuf::from("key.bin")));
        assert!(core.trust_unverified_artifact);
        assert_eq!(core.name, PathBuf::from("module.taot"));
    }
}
