use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};

use crate::cli::ScanArgs;

#[derive(Debug, Clone)]
pub struct ScanConfig {
    pub includes: Vec<PathBuf>,
    pub excludes: Vec<PathBuf>,
    pub metadata_only_paths: Vec<PathBuf>,
    pub metadata_only_excludes: Vec<PathBuf>,
    pub config_root: Option<PathBuf>,
    pub ignore_matcher: Option<Arc<IgnoreMatcher>>,
}

impl ScanConfig {
    pub fn from_args(args: &ScanArgs) -> Result<Self> {
        let home = resolve_home_dir();
        let mut includes = if args.no_defaults {
            Vec::new()
        } else {
            default_includes(home.as_deref())
        };

        let mut excludes = if args.no_defaults {
            Vec::new()
        } else {
            default_excludes(home.as_deref())
        };

        includes.extend(args.include.iter().cloned());
        excludes.extend(args.exclude.iter().cloned());

        let includes = normalize_paths(includes);
        let excludes = normalize_paths(excludes);

        let mut metadata_only_paths = Vec::new();
        if args.usr_metadata_only {
            metadata_only_paths.push(PathBuf::from("/usr"));
        }
        metadata_only_paths.extend(args.metadata_only.iter().cloned());

        let metadata_only_paths = normalize_paths(metadata_only_paths);
        let metadata_only_excludes = metadata_only_excludes(&metadata_only_paths);

        let ignore_matcher = load_driftignore(home.as_deref())?;

        Ok(Self {
            includes,
            excludes,
            metadata_only_paths,
            metadata_only_excludes,
            config_root: home.map(|home| home.join(".config")),
            ignore_matcher,
        })
    }

    pub fn is_metadata_only(&self, path: &Path) -> bool {
        let matches = self
            .metadata_only_paths
            .iter()
            .any(|prefix| path.starts_with(prefix));
        if !matches {
            return false;
        }

        !self
            .metadata_only_excludes
            .iter()
            .any(|exclude| path.starts_with(exclude))
    }
}

pub fn is_config_backup_path(config_root: Option<&Path>, path: &Path) -> bool {
    let Some(config_root) = config_root else {
        return false;
    };

    let relative = match path.strip_prefix(config_root) {
        Ok(relative) => relative,
        Err(_) => return false,
    };

    relative.components().any(|component| match component {
        Component::Normal(name) => name.to_string_lossy().ends_with("-backup"),
        _ => false,
    })
}

#[derive(Debug)]
pub struct IgnoreMatcher {
    base: PathBuf,
    matcher: Gitignore,
}

impl IgnoreMatcher {
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let relative = path.strip_prefix(&self.base).unwrap_or(path);
        self.matcher
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
    }
}

fn load_driftignore(home: Option<&Path>) -> Result<Option<Arc<IgnoreMatcher>>> {
    let mut builder = GitignoreBuilder::new(Path::new("/"));
    let mut loaded = false;

    for path in driftignore_paths(home) {
        if !path.exists() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("read driftignore {}", path.display()))?;
        for line in contents.lines() {
            builder
                .add_line(Some(path.clone()), line)
                .with_context(|| format!("parse driftignore line in {}", path.display()))?;
        }
        loaded = true;
    }

    if !loaded {
        return Ok(None);
    }

    let matcher = builder.build().context("build driftignore matcher")?;
    Ok(Some(Arc::new(IgnoreMatcher {
        base: PathBuf::from("/"),
        matcher,
    })))
}

fn driftignore_paths(home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        paths.push(cwd.join(".driftignore"));
    }

    let config_home = env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".config")));

    if let Some(config_home) = config_home {
        paths.push(config_home.join("image-drift-tracker/.driftignore"));
    }

    paths
}

fn default_includes(_home: Option<&Path>) -> Vec<PathBuf> {
    vec![
        PathBuf::from("/etc"),
        PathBuf::from("/usr"),
        PathBuf::from("/usr/local"),
        PathBuf::from("/opt"),
    ]
}

fn default_excludes(home: Option<&Path>) -> Vec<PathBuf> {
    let mut excludes = vec![
        PathBuf::from("/proc"),
        PathBuf::from("/sys"),
        PathBuf::from("/sysroot"),
        PathBuf::from("/boot"),
        PathBuf::from("/dev"),
        PathBuf::from("/run"),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/log"),
        PathBuf::from("/var/cache"),
        PathBuf::from("/var/lib/containers"),
    ];

    if let Some(home) = home {
        excludes.push(home.join(".cache"));
        excludes.push(home.join(".local/share"));
        excludes.push(home.join(".local/share/containers"));
        excludes.push(home.join(".local/share/flatpak"));
        excludes.push(home.join(".config/Google"));
        excludes.push(home.join(".config/GNS3"));
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
        if let Some(home) = resolve_home_dir() {
            let tail = path_str.strip_prefix("~/").unwrap_or("");
            if tail.is_empty() {
                return home;
            }
            return home.join(tail);
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

fn metadata_only_excludes(metadata_only_paths: &[PathBuf]) -> Vec<PathBuf> {
    let usr_path = Path::new("/usr");
    let usr_local_path = Path::new("/usr/local");

    let usr_enabled = metadata_only_paths.iter().any(|path| path == usr_path);
    let usr_local_enabled = metadata_only_paths
        .iter()
        .any(|path| path == usr_local_path);

    if usr_enabled && !usr_local_enabled {
        return vec![usr_local_path.to_path_buf()];
    }

    Vec::new()
}

pub fn resolve_home_dir() -> Option<PathBuf> {
    if let Ok(sudo_user) = env::var("SUDO_USER") {
        let sudo_user = sudo_user.trim();
        if !sudo_user.is_empty() {
            if let Some(home) = home_from_passwd(sudo_user) {
                return Some(home);
            }
        }
    }

    env::var("HOME").ok().map(PathBuf::from)
}

fn home_from_passwd(user: &str) -> Option<PathBuf> {
    let contents = fs::read_to_string("/etc/passwd").ok()?;

    for line in contents.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        if name != user {
            continue;
        }

        let _password = parts.next()?;
        let _uid = parts.next()?;
        let _gid = parts.next()?;
        let _gecos = parts.next()?;
        let home = parts.next()?;
        if home.is_empty() {
            return None;
        }

        return Some(PathBuf::from(home));
    }

    None
}
