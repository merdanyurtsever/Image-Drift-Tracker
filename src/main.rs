mod baseline;
mod cli;
mod config;
mod diff;
mod output;
mod report;
mod scanner;

use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;

use crate::cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan(args) => run_scan(args),
    }
}

fn run_scan(args: cli::ScanArgs) -> Result<()> {
    let config = config::ScanConfig::from_args(&args)?;
    let baseline = baseline::resolve_baseline(args.baseline, args.baseline_dir.as_deref())?;

    let baseline_map = scanner::scan_root(&baseline.root, &config)
        .with_context(|| format!("scan baseline at {}", baseline.root.display()))?;
    let live_root = Path::new("/");
    let live_map = scanner::scan_root(live_root, &config)
        .with_context(|| format!("scan live root at {}", live_root.display()))?;

    let mut diff_entries = diff::compute_diff(&baseline_map, &live_map);
    diff_entries.sort_by(|a, b| a.path.cmp(&b.path));

    let report = report::build_report(&baseline, &diff_entries);
    let use_color = output::use_color(args.no_color);

    if args.json {
        output::print_json_report(&report)?;
    } else {
        output::print_summary(&report, use_color, args.max_items);
        if args.diff {
            output::print_unified_diffs(&baseline.root, live_root, &diff_entries)?;
        }
    }

    if !args.no_report {
        if let Err(err) = report::write_report(&report, args.report_path.as_deref()) {
            eprintln!("warn: failed to write report: {err}");
        }
    }

    Ok(())
}
