use std::env;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use content_inspector::ContentType;
use owo_colors::OwoColorize;
use similar::TextDiff;

use crate::diff::{DiffEntry, DriftKind};
use crate::report::{Report, ReportItem};
use crate::scanner::{self, FileType};

pub fn use_color(no_color_flag: bool) -> bool {
    !no_color_flag && env::var_os("NO_COLOR").is_none()
}

pub fn print_summary(report: &Report, use_color: bool, max_items: usize) {
    let summary = &report.summary;
    println!(
        "Drift summary: {} added, {} removed, {} content-changed, {} metadata-changed, {} symlink-target-changed, {} type-changed",
        summary.added,
        summary.removed,
        summary.content_changed,
        summary.metadata_changed,
        summary.symlink_target_changed,
        summary.type_changed
    );

    if report.items.is_empty() {
        println!("No drift detected.");
        return;
    }

    println!("Top items:");
    for item in report.items.iter().take(max_items) {
        println!("{}", format_item(item, use_color));
    }
}

pub fn print_json_report(report: &Report) -> Result<()> {
    let json = serde_json::to_string_pretty(report).context("serialize report")?;
    println!("{json}");
    Ok(())
}

pub fn print_unified_diffs(
    baseline_root: &Path,
    live_root: &Path,
    diff_entries: &[DiffEntry],
) -> Result<()> {
    for entry in diff_entries {
        if entry.kind != DriftKind::ContentChanged {
            continue;
        }

        let (Some(before), Some(after)) = (&entry.before, &entry.after) else {
            continue;
        };

        if before.file_type != FileType::File || after.file_type != FileType::File {
            continue;
        }

        let baseline_path = scanner::join_root(baseline_root, &entry.path);
        let live_path = scanner::join_root(live_root, &entry.path);

        match unified_diff(&baseline_path, &live_path, &entry.path)? {
            Some(diff) => print!("{diff}"),
            None => println!("Binary diff skipped: {}", entry.path.display()),
        }
    }

    Ok(())
}

fn format_item(item: &ReportItem, use_color: bool) -> String {
    let prefix = if use_color {
        match item.kind {
            DriftKind::Added => "+".green().to_string(),
            DriftKind::Removed => "-".red().to_string(),
            DriftKind::ContentChanged => "~".yellow().to_string(),
            DriftKind::MetadataChanged => "!".blue().to_string(),
            DriftKind::SymlinkTargetChanged => "@".magenta().to_string(),
            DriftKind::TypeChanged => "#".cyan().to_string(),
        }
    } else {
        match item.kind {
            DriftKind::Added => "+".to_string(),
            DriftKind::Removed => "-".to_string(),
            DriftKind::ContentChanged => "~".to_string(),
            DriftKind::MetadataChanged => "!".to_string(),
            DriftKind::SymlinkTargetChanged => "@".to_string(),
            DriftKind::TypeChanged => "#".to_string(),
        }
    };

    format!("{} {}", prefix, item.path)
}

fn unified_diff(old_path: &Path, new_path: &Path, display_path: &Path) -> Result<Option<String>> {
    let old_bytes = fs::read(old_path)
        .with_context(|| format!("read {}", old_path.display()))?;
    let new_bytes = fs::read(new_path)
        .with_context(|| format!("read {}", new_path.display()))?;

    if is_binary(&old_bytes) || is_binary(&new_bytes) {
        return Ok(None);
    }

    let old_text = String::from_utf8_lossy(&old_bytes);
    let new_text = String::from_utf8_lossy(&new_bytes);

    let diff = TextDiff::from_lines(&old_text, &new_text);
    let mut output = Vec::new();

    let header_old = format!("baseline{}", display_path.display());
    let header_new = format!("live{}", display_path.display());

    diff.unified_diff()
        .context_radius(3)
        .header(&header_old, &header_new)
        .to_writer(&mut output)
        .map_err(|_| anyhow::Error::msg(format!("write diff for {}", display_path.display())))?;

    let output = String::from_utf8(output)
        .map_err(|_| anyhow::Error::msg(format!("diff output is not utf-8 for {}", display_path.display())))?;
    Ok(Some(output))
}

fn is_binary(bytes: &[u8]) -> bool {
    matches!(content_inspector::inspect(bytes), ContentType::BINARY)
}
