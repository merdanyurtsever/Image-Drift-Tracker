use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::cli::ScanArgs;

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub includes: Vec<PathBuf>,
    pub excludes: Vec<PathBuf>,
}

impl ScanConfig {
    pub fn from_args(args: &ScanArgs) -> Result<Self> {
        let mut includes = if args.no_defaults {
            Vec::new()
        } else {
            default_includes()
        };

        let mut excludes = if args.no_defaults {
            Vec::new()
        } else {
            default_excludes()
        };

        includes.extend(args.include.iter().cloned());
        excludes.extend(args.exclude.iter().cloned());

        let includes = normalize_paths(includes);
        let excludes = normalize_paths(excludes);

        Ok(Self { includes, excludes })
    }
}

fn default_includes() -> Vec<PathBuf> {
    let mut includes = vec![
        PathBuf::from("/etc"),
        PathBuf::from("/usr"),
        PathBuf::from("/boot"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/opt"),
    ];

    if let Ok(home) = env::var("HOME") {
        let home = PathBuf::from(home);
        includes.push(home.join(".config"));
        includes.push(home.join(".local/bin"));
        includes.push(home.join(".local/share"));
        includes.push(home.join(".bashrc"));
        includes.push(home.join(".zshrc"));
        includes.push(home.join(".profile"));
        includes.push(home.join(".ssh/config"));
    }

    includes
}

fn default_excludes() -> Vec<PathBuf> {
    let mut excludes = vec![
        PathBuf::from("/proc"),
        PathBuf::from("/sys"),
        PathBuf::from("/dev"),
        PathBuf::from("/run"),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/log"),
        PathBuf::from("/var/cache"),
        PathBuf::from("/var/lib/containers"),
    ];

    if let Ok(home) = env::var("HOME") {
        excludes.push(PathBuf::from(home).join(".cache"));
    }

    excludes
}

fn normalize_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for path in paths {
        let normalized = normalize_path(&path);
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    out
}

fn normalize_path(path: &Path) -> PathBuf {
    let expanded = expand_tilde(path);
    absolutize(expanded)
}

fn expand_tilde(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if path_str == "~" || path_str.starts_with("~/") {
        if let Ok(home) = env::var("HOME") {
            let tail = path_str.strip_prefix("~/").unwrap_or("");
            if tail.is_empty() {
                return PathBuf::from(home);
            }
            return PathBuf::from(home).join(tail);
        }
    }

    path.to_path_buf()
}

fn absolutize(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        Path::new("/").join(path)
    }
}
