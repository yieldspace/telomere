use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    pub name: PathBuf,
    pub func: String,
    pub args: Vec<i32>,
}
