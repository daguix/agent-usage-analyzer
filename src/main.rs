mod activity;
mod breakdown;
mod claude;
mod cli;
mod ingest;
mod latency;
mod pricing;
mod report;
mod tools;
mod workflow;

use anyhow::Result;
use clap::Parser;

fn main() -> Result<()> {
    cli::run(cli::Cli::parse())
}
