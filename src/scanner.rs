use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blake3::Hasher;
use serde::Serialize;
use walkdir::WalkDir;

use crate::config::ScanConfig;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    File,
    Symlink,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub file_type: FileType,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub hash: Option<String>,
    pub symlink_target: Option<String>,
}

pub fn scan_root(root: &Path, config: &ScanConfig) -> Result<HashMap<PathBuf, FileEntry>> {
    let exclude_roots = build_exclude_roots(root, &config.excludes);
    let mut map = HashMap::new();

    for include in &config.includes {
        if let Err(err) = scan_include(root, include, &exclude_roots, &mut map) {
            eprintln!("warn: failed to scan {}: {}", include.display(), err);
        }
    }

    Ok(map)
}

fn scan_include(
    root: &Path,
    include: &Path,
    exclude_roots: &[PathBuf],
    map: &mut HashMap<PathBuf, FileEntry>,
) -> Result<()> {
    let include_root = join_root(root, include);

    let metadata = match fs::symlink_metadata(&include_root) {
        Ok(meta) => meta,
        Err(err) => {
            eprintln!(
                "warn: include missing {}: {}",
                include_root.display(),
                err
            );
            return Ok(());
        }
    };

    if is_excluded(&include_root, exclude_roots) {
        return Ok(());
    }

    if metadata.is_dir() {
        for entry in WalkDir::new(&include_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_excluded(e.path(), exclude_roots))
        {
            match entry {
                Ok(entry) => {
                    if entry.file_type().is_dir() {
                        continue;
                    }
                    if let Err(err) = insert_entry(root, entry.path(), map) {
                        eprintln!("warn: {}: {}", entry.path().display(), err);
                    }
                }
                Err(err) => {
                    eprintln!(
                        "warn: walk error under {}: {}",
                        include_root.display(),
                        err
                    );
                }
            }
        }
    } else {
        insert_entry(root, &include_root, map)?;
    }

    Ok(())
}

fn insert_entry(
    root: &Path,
    path: &Path,
    map: &mut HashMap<PathBuf, FileEntry>,
) -> Result<()> {
    if let Some(entry) = FileEntry::from_path(path)? {
        let relative = to_root_relative(root, path)?;
        map.insert(relative, entry);
    }
    Ok(())
}

fn build_exclude_roots(root: &Path, excludes: &[PathBuf]) -> Vec<PathBuf> {
    excludes.iter().map(|path| join_root(root, path)).collect()
}

fn is_excluded(path: &Path, excludes: &[PathBuf]) -> bool {
    excludes.iter().any(|exclude| path.starts_with(exclude))
}

impl FileEntry {
    fn from_path(path: &Path) -> Result<Option<Self>> {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("metadata {}", path.display()))?;
        let file_type = metadata.file_type();

        if file_type.is_dir() {
            return Ok(None);
        }

        let mode = metadata.mode();
        let uid = metadata.uid();
        let gid = metadata.gid();
        let size = metadata.len();

        if file_type.is_symlink() {
            let target = fs::read_link(path)
                .with_context(|| format!("read link {}", path.display()))?;
            return Ok(Some(FileEntry {
                file_type: FileType::Symlink,
                mode,
                uid,
                gid,
                size,
                hash: None,
                symlink_target: Some(target.to_string_lossy().to_string()),
            }));
        }

        if metadata.is_file() {
            let hash = hash_file(path)?;
            return Ok(Some(FileEntry {
                file_type: FileType::File,
                mode,
                uid,
                gid,
                size,
                hash: Some(hash),
                symlink_target: None,
            }));
        }

        Ok(None)
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("open file {}", path.display()))?;
    let mut hasher = Hasher::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("read file {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

pub(crate) fn join_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        let stripped = path.strip_prefix("/").unwrap_or(path);
        root.join(stripped)
    } else {
        root.join(path)
    }
}

fn to_root_relative(root: &Path, path: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("strip {} from {}", root.display(), path.display()))?;
    Ok(Path::new("/").join(relative))
}
