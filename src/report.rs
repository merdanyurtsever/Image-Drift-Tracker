use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::baseline::Baseline;
use crate::diff::{DiffEntry, DriftKind};
use crate::scanner::FileEntry;

#[derive(Debug, Serialize)]
pub struct Report {
    pub generated_at: u64,
    pub baseline: BaselineInfo,
    pub summary: Summary,
    pub items: Vec<ReportItem>,
}

#[derive(Debug, Serialize)]
pub struct BaselineInfo {
    pub kind: String,
    pub root: String,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub added: usize,
    pub removed: usize,
    pub content_changed: usize,
    pub metadata_changed: usize,
    pub symlink_target_changed: usize,
    pub type_changed: usize,
}

#[derive(Debug, Serialize)]
pub struct ReportItem {
    pub path: String,
    pub kind: DriftKind,
    pub before: Option<FileEntry>,
    pub after: Option<FileEntry>,
}

pub fn build_report(baseline: &Baseline, diff_entries: &[DiffEntry]) -> Report {
    let items: Vec<ReportItem> = diff_entries
        .iter()
        .map(|entry| ReportItem {
            path: entry.path.to_string_lossy().to_string(),
            kind: entry.kind.clone(),
            before: entry.before.clone(),
            after: entry.after.clone(),
        })
        .collect();

    let summary = Summary::from_items(&items);
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Report {
        generated_at,
        baseline: BaselineInfo {
            kind: baseline.kind.as_str().to_string(),
            root: baseline.root.to_string_lossy().to_string(),
        },
        summary,
        items,
    }
}

pub fn write_report(report: &Report, path: Option<&Path>) -> Result<PathBuf> {
    let path = path
        .map(PathBuf::from)
        .unwrap_or_else(default_report_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }

    let json = serde_json::to_vec_pretty(report).context("serialize report")?;
    fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;

    Ok(path)
}

fn default_report_path() -> PathBuf {
    if let Ok(state_home) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(state_home).join("image-drift-tracker/drift.json");
    }

    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".local/state/image-drift-tracker/drift.json");
    }

    PathBuf::from("drift.json")
}

impl Summary {
    fn from_items(items: &[ReportItem]) -> Self {
        let mut summary = Summary {
            added: 0,
            removed: 0,
            content_changed: 0,
            metadata_changed: 0,
            symlink_target_changed: 0,
            type_changed: 0,
        };

        for item in items {
            match item.kind {
                DriftKind::Added => summary.added += 1,
                DriftKind::Removed => summary.removed += 1,
                DriftKind::ContentChanged => summary.content_changed += 1,
                DriftKind::MetadataChanged => summary.metadata_changed += 1,
                DriftKind::SymlinkTargetChanged => summary.symlink_target_changed += 1,
                DriftKind::TypeChanged => summary.type_changed += 1,
            }
        }

        summary
    }
}
