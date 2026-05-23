use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

use crate::collector::Collector;

fn expand_path(s: &str) -> Result<PathBuf, shellexpand::LookupError<std::env::VarError>> {
    shellexpand::full(s).map(|cow| PathBuf::from(cow.as_ref()))
}

#[derive(Parser, Debug)]
#[command(version, about, author, long_about=None)]
pub struct CliArgs {
    #[arg(short, long, help = "Enable verbose output")]
    pub verbose: bool,

    #[arg(short, long, help = "Only output warnings and errors")]
    pub quiet: bool,

    #[arg(
        short = 'k',
        long,
        help = "Bunny.net API access key (Can also be set by environment variable BUNNYNET_ACCESS_KEY)"
    )]
    pub access_key: Option<String>,

    #[arg(
        short = 'f',
        long,
        value_parser = expand_path,
        help = "Path to a file containing a Bunny.net API access key"
    )]
    pub access_key_file: Option<PathBuf>,

    #[arg(
        short = 'r',
        long,
        default_value = "10",
        help = "Timeout in seconds for Bunny.net API requests"
    )]
    pub api_request_timeout: u64,

    #[arg(
        short = 'n',
        long,
        default_value = "5",
        help = "Maximum number of concurrent Bunny.net API requests"
    )]
    pub api_concurrency: usize,

    #[arg(
        short = 'i',
        long,
        default_value = "300",
        help = "Update interval in seconds"
    )]
    pub poll_interval: u64,

    #[arg(
        short = 's',
        long,
        default_value = "~/.local/share/bunnynet_prometheus/state",
        value_parser = expand_path,
        help = "Path to a directory to store persistent state files in"
    )]
    pub state_dir: PathBuf,

    #[arg(
        short = 'a',
        long,
        default_value = "0.0.0.0",
        help = "HTTP server bind address"
    )]
    pub bind_addr: IpAddr,

    #[arg(
        short = 'p',
        long,
        default_value = "9000",
        help = "HTTP server bind port"
    )]
    pub bind_port: u16,

    #[arg(short, long, value_enum, num_args = 1.., required = true, value_delimiter = ',', help="Comma-separated list of categories of statistics to poll")]
    pub collectors: Vec<Collector>,
}
