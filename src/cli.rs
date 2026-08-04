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
`wasi:cli/run@0.2.6`.",
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

    /// Path to the core Wasm module to run.
    #[arg(value_name = "MODULE")]
    pub name: Option<PathBuf>,

    /// Export name and i32 arguments, or guest argv when placed after `--`.
    #[arg(value_name = "ARG")]
    pub args: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run a WASI 0.2 component that exports `wasi:cli/run@0.2.6`.
    Component(ComponentCommand),
    /// Print the resolved measurement-only optimizer switch state as JSON.
    #[cfg(feature = "measure-switches")]
    MeasureSwitchesProbe,
}

#[derive(Args, Debug, Clone)]
pub struct ComponentCommand {
    /// Path to the component to run.
    #[arg(value_name = "COMPONENT")]
    pub name: PathBuf,

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
}
