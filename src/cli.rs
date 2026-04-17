use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::baseline::BaselineKind;

#[derive(Parser, Debug)]
#[command(
    name = "image-drift-tracker",
    version,
    about = "OS drift tracker for rpm-ostree systems"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Scan(ScanArgs),
}

#[derive(Args, Debug)]
pub struct ScanArgs {
    #[arg(long, value_enum, default_value_t = BaselineKind::RpmOstree)]
    pub baseline: BaselineKind,

    #[arg(long)]
    pub baseline_dir: Option<PathBuf>,

    #[arg(long, action = clap::ArgAction::Append)]
    pub include: Vec<PathBuf>,

    #[arg(long, action = clap::ArgAction::Append)]
    pub exclude: Vec<PathBuf>,

    #[arg(long)]
    pub no_defaults: bool,

    #[arg(long)]
    pub report_path: Option<PathBuf>,

    #[arg(long)]
    pub no_report: bool,

    #[arg(long)]
    pub diff: bool,

    #[arg(long)]
    pub json: bool,

    #[arg(long, default_value_t = 50)]
    pub max_items: usize,

    #[arg(long)]
    pub no_color: bool,
}
