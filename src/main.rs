mod breakdown;
mod claude;
mod cli;
mod ingest;
mod latency;
mod pricing;
mod report;
mod workflow;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    cli::run(cli::Cli::parse())
}
