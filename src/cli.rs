use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand};
use std::{ffi::OsString, path::PathBuf};

#[derive(Parser, Debug)]
#[command(
    version,
    about,
    long_about = None,
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(long = "jit", default_value_t = false)]
    pub jit: bool,

    #[arg(long = "jit-code-cache-mib", default_value_t = 4)]
    pub jit_code_cache_mib: u32,

    pub name: Option<PathBuf>,

    #[arg(value_name = "ARG")]
    pub args: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Component(ComponentCommand),
}

#[derive(Args, Debug, Clone)]
pub struct ComponentCommand {
    pub name: PathBuf,

    #[arg(long = "dir", value_name = "HOST[:GUEST]")]
    pub preopens: Vec<String>,

    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub env: Vec<String>,

    #[arg(long = "no-inherit-env", default_value_t = false)]
    pub no_inherit_env: bool,

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
