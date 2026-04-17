use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use serde::Serialize;

use crate::scanner::{FileEntry, FileType};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    Added,
    Removed,
    ContentChanged,
    MetadataChanged,
    SymlinkTargetChanged,
    TypeChanged,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffEntry {
    pub path: PathBuf,
    pub kind: DriftKind,
    pub before: Option<FileEntry>,
    pub after: Option<FileEntry>,
}

pub fn compute_diff(
    baseline: &HashMap<PathBuf, FileEntry>,
    live: &HashMap<PathBuf, FileEntry>,
) -> Vec<DiffEntry> {
    let mut paths = BTreeSet::new();
    paths.extend(baseline.keys().cloned());
    paths.extend(live.keys().cloned());

    let mut entries = Vec::new();

    for path in paths {
        let before = baseline.get(&path);
        let after = live.get(&path);

        let kind = match (before, after) {
            (None, Some(_)) => DriftKind::Added,
            (Some(_), None) => DriftKind::Removed,
            (Some(before), Some(after)) => classify_change(before, after),
            (None, None) => continue,
        };

        if kind == DriftKind::MetadataChanged && metadata_equal(before.unwrap(), after.unwrap()) {
            continue;
        }

        entries.push(DiffEntry {
            path,
            kind,
            before: before.cloned(),
            after: after.cloned(),
        });
    }

    entries
}

fn classify_change(before: &FileEntry, after: &FileEntry) -> DriftKind {
    if before.file_type != after.file_type {
        return DriftKind::TypeChanged;
    }

    match before.file_type {
        FileType::File => {
            if before.hash != after.hash {
                DriftKind::ContentChanged
            } else if !metadata_equal(before, after) {
                DriftKind::MetadataChanged
            } else {
                DriftKind::MetadataChanged
            }
        }
        FileType::Symlink => {
            if before.symlink_target != after.symlink_target {
                DriftKind::SymlinkTargetChanged
            } else if !metadata_equal(before, after) {
                DriftKind::MetadataChanged
            } else {
                DriftKind::MetadataChanged
            }
        }
    }
}

fn metadata_equal(before: &FileEntry, after: &FileEntry) -> bool {
    before.mode == after.mode && before.uid == after.uid && before.gid == after.gid
}
