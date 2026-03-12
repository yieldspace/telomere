use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

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

    pub name: Option<PathBuf>,

    pub func: Option<String>,

    pub args: Vec<i32>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_core_module_invocation() {
        let cli = Cli::try_parse_from(["telomere-cli", "examples/add.wasm", "main", "1", "2"])
            .expect("legacy invocation should parse");

        assert!(cli.command.is_none());
        assert_eq!(cli.name, Some(PathBuf::from("examples/add.wasm")));
        assert_eq!(cli.func.as_deref(), Some("main"));
        assert_eq!(cli.args, vec![1, 2]);
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
