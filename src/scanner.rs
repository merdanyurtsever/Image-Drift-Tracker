use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blake3::Hasher;
use rayon::prelude::*;
use serde::Serialize;

use crate::config::{self, ScanConfig};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    File,
    Symlink,
}

#[derive(Clone, Copy, Debug)]
enum HashPolicy {
    Full,
    MetadataOnly,
}

impl HashPolicy {
    fn should_hash(self) -> bool {
        matches!(self, HashPolicy::Full)
    }
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
    scan_root_with(
        root,
        config,
        &config.includes,
        &config.excludes,
        false,
        None,
    )
}

pub fn scan_root_with(
    root: &Path,
    config: &ScanConfig,
    includes: &[PathBuf],
    excludes: &[PathBuf],
    same_file_system: bool,
    hash_path_remap: Option<(&Path, &Path)>,
) -> Result<HashMap<PathBuf, FileEntry>> {
    let exclude_roots = build_exclude_roots(root, excludes);
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    for include in includes {
        if let Err(err) =
            collect_paths(
                root,
                include,
                config,
                &exclude_roots,
                same_file_system,
                &mut paths,
                &mut seen,
            )
        {
            eprintln!("warn: failed to scan {}: {}", include.display(), err);
        }
    }

    let entries: Vec<(PathBuf, FileEntry)> = paths
        .par_iter()
        .filter_map(|path| {
            let relative = match to_root_relative(root, path) {
                Ok(relative) => relative,
                Err(err) => {
                    eprintln!("warn: {}: {}", path.display(), err);
                    return None;
                }
            };

            let metadata_path = match hash_path_remap {
                Some((from, to)) => remap_prefix(&relative, from, to),
                None => relative.clone(),
            };

            let hash_policy = if config.is_metadata_only(&metadata_path) {
                HashPolicy::MetadataOnly
            } else {
                HashPolicy::Full
            };

            match FileEntry::from_path(path, hash_policy) {
                Ok(Some(entry)) => Some((relative, entry)),
                Ok(None) => None,
                Err(err) => {
                    eprintln!("warn: {}: {}", path.display(), err);
                    None
                }
            }
        })
        .collect();

    Ok(entries.into_iter().collect())
}

fn collect_paths(
    root: &Path,
    include: &Path,
    config: &ScanConfig,
    exclude_roots: &[PathBuf],
    same_file_system: bool,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
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

    if is_excluded_path(&include_root, metadata.is_dir(), exclude_roots, config) {
        return Ok(());
    }

    if metadata.is_dir() {
        let root_dev = metadata.dev();
        walk_directory(
            &include_root,
            root_dev,
            same_file_system,
            exclude_roots,
            config,
            paths,
            seen,
        )?;
    } else if seen.insert(include_root.clone()) {
        paths.push(include_root);
    }

    Ok(())
}

fn walk_directory(
    dir: &Path,
    root_dev: u64,
    same_file_system: bool,
    exclude_roots: &[PathBuf],
    config: &ScanConfig,
    paths: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) -> Result<()> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if is_permission_denied_io(&err) => return Ok(()),
        Err(err) => return Err(err).with_context(|| format!("read dir {}", dir.display())),
    };

    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) if is_permission_denied_io(&err) => continue,
            Err(err) => return Err(err).with_context(|| format!("read entry under {}", dir.display())),
        };

        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) if is_permission_denied_io(&err) => continue,
            Err(err) => return Err(err).with_context(|| format!("metadata {}", path.display())),
        };

        let is_dir = metadata.is_dir();
        if is_excluded_path(&path, is_dir, exclude_roots, config) {
            continue;
        }

        if same_file_system && metadata.dev() != root_dev {
            continue;
        }

        if is_dir {
            walk_directory(
                &path,
                root_dev,
                same_file_system,
                exclude_roots,
                config,
                paths,
                seen,
            )?;
            continue;
        }

        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }

    Ok(())
}

fn build_exclude_roots(root: &Path, excludes: &[PathBuf]) -> Vec<PathBuf> {
    excludes.iter().map(|path| join_root(root, path)).collect()
}

fn is_excluded(path: &Path, excludes: &[PathBuf]) -> bool {
    excludes.iter().any(|exclude| path.starts_with(exclude))
}

fn is_excluded_path(path: &Path, is_dir: bool, excludes: &[PathBuf], config: &ScanConfig) -> bool {
    is_excluded_path_with_root(
        path,
        is_dir,
        excludes,
        config.config_root.as_deref(),
        config.ignore_matcher.as_deref(),
    )
}

fn is_excluded_path_with_root(
    path: &Path,
    is_dir: bool,
    excludes: &[PathBuf],
    config_root: Option<&Path>,
    ignore_matcher: Option<&config::IgnoreMatcher>,
) -> bool {
    if is_excluded(path, excludes) || config::is_config_backup_path(config_root, path) {
        return true;
    }

    ignore_matcher
        .map(|matcher| matcher.is_ignored(path, is_dir))
        .unwrap_or(false)
}

impl FileEntry {
    fn from_path(path: &Path, hash_policy: HashPolicy) -> Result<Option<Self>> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(meta) => meta,
            Err(err) if is_permission_denied_io(&err) => return Ok(None),
            Err(err) => return Err(err).with_context(|| format!("metadata {}", path.display())),
        };
        let file_type = metadata.file_type();

        if file_type.is_dir() {
            return Ok(None);
        }

        let mode = metadata.mode();
        let uid = metadata.uid();
        let gid = metadata.gid();
        let size = metadata.len();

        if file_type.is_symlink() {
            let target = match fs::read_link(path) {
                Ok(target) => target,
                Err(err) if is_permission_denied_io(&err) => return Ok(None),
                Err(err) => return Err(err).with_context(|| format!("read link {}", path.display())),
            };
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
            let hash = if hash_policy.should_hash() {
                match hash_file(path)? {
                    Some(hash) => Some(hash),
                    None => return Ok(None),
                }
            } else {
                None
            };
            return Ok(Some(FileEntry {
                file_type: FileType::File,
                mode,
                uid,
                gid,
                size,
                hash,
                symlink_target: None,
            }));
        }

        Ok(None)
    }
}

fn hash_file(path: &Path) -> Result<Option<String>> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if is_permission_denied_io(&err) => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("open file {}", path.display())),
    };
    let mut hasher = Hasher::new();
    let mut buffer = vec![0u8; 131072];

    loop {
        let read = match file.read(&mut buffer) {
            Ok(read) => read,
            Err(err) if is_permission_denied_io(&err) => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| format!("read file {}", path.display()))
            }
        };
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(Some(hasher.finalize().to_hex().to_string()))
}

pub(crate) fn join_root(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        let stripped = path.strip_prefix("/").unwrap_or(path);
        root.join(stripped)
    } else {
        root.join(path)
    }
}

pub fn remap_map_prefix(
    entries: HashMap<PathBuf, FileEntry>,
    from: &Path,
    to: &Path,
) -> HashMap<PathBuf, FileEntry> {
    let mut remapped = HashMap::with_capacity(entries.len());
    for (path, entry) in entries {
        let mapped = remap_prefix(&path, from, to);
        remapped.insert(mapped, entry);
    }
    remapped
}

fn remap_prefix(path: &Path, from: &Path, to: &Path) -> PathBuf {
    match path.strip_prefix(from) {
        Ok(suffix) if suffix.as_os_str().is_empty() => to.to_path_buf(),
        Ok(suffix) => to.join(suffix),
        Err(_) => path.to_path_buf(),
    }
}

fn to_root_relative(root: &Path, path: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(root)
        .with_context(|| format!("strip {} from {}", root.display(), path.display()))?;
    Ok(Path::new("/").join(relative))
}

fn is_permission_denied_io(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied
}
