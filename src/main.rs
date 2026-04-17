mod baseline;
mod cli;
mod config;
mod diff;
mod output;
mod report;
mod scanner;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::baseline::BaselineKind;
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

    let live_root = Path::new("/");
    let (baseline_map, live_map) = match baseline.kind {
        BaselineKind::RpmOstree => scan_rpm_ostree_providers(&baseline, &config, &args)?,
        BaselineKind::Dir => {
            let baseline_map = scanner::scan_root(&baseline.root, &config)
            .with_context(|| format!("scan baseline at {}", baseline.root.display()))?;
            let live_map = scanner::scan_root_with(
                live_root,
                &config,
                &config.includes,
                &config.excludes,
                true,
                None,
            )
            .with_context(|| format!("scan live root at {}", live_root.display()))?;
            (baseline_map, live_map)
        }
    };

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

fn scan_rpm_ostree_providers(
    baseline: &baseline::Baseline,
    config: &config::ScanConfig,
    args: &cli::ScanArgs,
) -> Result<(HashMap<PathBuf, scanner::FileEntry>, HashMap<PathBuf, scanner::FileEntry>)> {
    let live_root = Path::new("/");
    let home = config::resolve_home_dir();

    let mut system_includes = Vec::new();
    let mut etc_includes = Vec::new();
    let mut usr_local_includes = Vec::new();
    let mut home_includes = Vec::new();

    for include in &config.includes {
        if include.starts_with(Path::new("/etc")) {
            etc_includes.push(include.clone());
        } else if include.starts_with(Path::new("/usr/local")) {
            usr_local_includes.push(include.clone());
        } else if home
            .as_ref()
            .map(|home| include.starts_with(home))
            .unwrap_or(false)
        {
            home_includes.push(include.clone());
        } else {
            system_includes.push(include.clone());
        }
    }

    if args.home_baseline.is_none() && !home_includes.is_empty() {
        eprintln!("warn: home paths ignored without --home-baseline");
        home_includes.clear();
    }

    if args.home_baseline.is_some() && home_includes.is_empty() {
        if let Some(home) = &home {
            home_includes.push(home.clone());
        }
    }

    let mut system_excludes = config.excludes.clone();
    system_excludes.push(PathBuf::from("/etc"));
    system_excludes.push(PathBuf::from("/usr/local"));
    system_excludes.push(PathBuf::from("/usr/etc"));

    let baseline_system = scan_if_includes(
        &baseline.root,
        config,
        &system_includes,
        &system_excludes,
        false,
        None,
    )
    .with_context(|| format!("scan baseline system at {}", baseline.root.display()))?;
    let live_system = scan_if_includes(
        live_root,
        config,
        &system_includes,
        &system_excludes,
        true,
        None,
    )
    .with_context(|| format!("scan live system at {}", live_root.display()))?;

    let etc_baseline_includes = map_etc_includes(&etc_includes);
    let baseline_etc = scan_if_includes(
        &baseline.root,
        config,
        &etc_baseline_includes,
        &config.excludes,
        false,
        Some((Path::new("/usr/etc"), Path::new("/etc"))),
    )
    .with_context(|| format!("scan baseline /usr/etc at {}", baseline.root.display()))?;
    let baseline_etc = scanner::remap_map_prefix(
        baseline_etc,
        Path::new("/usr/etc"),
        Path::new("/etc"),
    );
    let live_etc = scan_if_includes(
        live_root,
        config,
        &etc_includes,
        &config.excludes,
        true,
        None,
    )
    .with_context(|| format!("scan live /etc at {}", live_root.display()))?;

    let baseline_usr_local = HashMap::new();
    let live_usr_local = scan_if_includes(
        live_root,
        config,
        &usr_local_includes,
        &config.excludes,
        true,
        None,
    )
    .with_context(|| format!("scan live /usr/local at {}", live_root.display()))?;

    let (baseline_home, live_home) = if let Some(home_baseline) = args.home_baseline.as_deref() {
        let Some(home) = &home else {
            bail!("--home-baseline requires a resolvable HOME");
        };
        if !home_baseline.exists() {
            bail!("home baseline not found: {}", home_baseline.display());
        }
        if !home_baseline.is_dir() {
            bail!("home baseline must be a directory: {}", home_baseline.display());
        }

        let baseline_home_includes = map_home_includes(&home_includes, home);
        let baseline_home_excludes = map_home_excludes(&config.excludes, home);

        let mut baseline_home_config = config.clone();
        baseline_home_config.config_root = Some(home_baseline.join(".config"));

        let baseline_home = scan_if_includes(
            home_baseline,
            &baseline_home_config,
            &baseline_home_includes,
            &baseline_home_excludes,
            false,
            Some((Path::new("/"), home)),
        )
        .with_context(|| format!("scan home baseline at {}", home_baseline.display()))?;
        let baseline_home = scanner::remap_map_prefix(baseline_home, Path::new("/"), home);

        let live_home = scan_if_includes(
            live_root,
            config,
            &home_includes,
            &config.excludes,
            true,
            None,
        )
        .with_context(|| format!("scan live home at {}", live_root.display()))?;

        (baseline_home, live_home)
    } else {
        (HashMap::new(), HashMap::new())
    };

    let mut baseline_map = HashMap::new();
    let mut live_map = HashMap::new();

    merge_maps(&mut baseline_map, baseline_system, "system baseline");
    merge_maps(&mut baseline_map, baseline_etc, "etc baseline");
    merge_maps(&mut baseline_map, baseline_usr_local, "usr-local baseline");
    merge_maps(&mut baseline_map, baseline_home, "home baseline");

    merge_maps(&mut live_map, live_system, "system live");
    merge_maps(&mut live_map, live_etc, "etc live");
    merge_maps(&mut live_map, live_usr_local, "usr-local live");
    merge_maps(&mut live_map, live_home, "home live");

    Ok((baseline_map, live_map))
}

fn scan_if_includes(
    root: &Path,
    config: &config::ScanConfig,
    includes: &[PathBuf],
    excludes: &[PathBuf],
    same_file_system: bool,
    hash_path_remap: Option<(&Path, &Path)>,
) -> Result<HashMap<PathBuf, scanner::FileEntry>> {
    if includes.is_empty() {
        return Ok(HashMap::new());
    }

    scanner::scan_root_with(
        root,
        config,
        includes,
        excludes,
        same_file_system,
        hash_path_remap,
    )
}

fn map_etc_includes(includes: &[PathBuf]) -> Vec<PathBuf> {
    let etc_root = Path::new("/etc");
    let usr_etc_root = Path::new("/usr/etc");
    includes
        .iter()
        .map(|path| {
            let relative = path.strip_prefix(etc_root).unwrap_or(path);
            if relative.as_os_str().is_empty() {
                usr_etc_root.to_path_buf()
            } else {
                usr_etc_root.join(relative)
            }
        })
        .collect()
}

fn map_home_includes(includes: &[PathBuf], home: &Path) -> Vec<PathBuf> {
    includes
        .iter()
        .filter_map(|path| path.strip_prefix(home).ok())
        .map(|relative| {
            if relative.as_os_str().is_empty() {
                PathBuf::from("/")
            } else {
                Path::new("/").join(relative)
            }
        })
        .collect()
}

fn map_home_excludes(excludes: &[PathBuf], home: &Path) -> Vec<PathBuf> {
    excludes
        .iter()
        .filter_map(|path| path.strip_prefix(home).ok())
        .map(|relative| {
            if relative.as_os_str().is_empty() {
                PathBuf::from("/")
            } else {
                Path::new("/").join(relative)
            }
        })
        .collect()
}

fn merge_maps(
    target: &mut HashMap<PathBuf, scanner::FileEntry>,
    source: HashMap<PathBuf, scanner::FileEntry>,
    label: &str,
) {
    for (path, entry) in source {
        if target.insert(path.clone(), entry).is_some() {
            eprintln!("warn: duplicate path in {}: {}", label, path.display());
        }
    }
}
