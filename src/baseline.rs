use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum BaselineKind {
    RpmOstree,
    Dir,
}

impl BaselineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BaselineKind::RpmOstree => "rpm-ostree",
            BaselineKind::Dir => "dir",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Baseline {
    pub kind: BaselineKind,
    pub root: PathBuf,
}

pub fn resolve_baseline(kind: BaselineKind, baseline_dir: Option<&Path>) -> Result<Baseline> {
    let root = match kind {
        BaselineKind::RpmOstree => rpm_ostree_root()?,
        BaselineKind::Dir => {
            let dir = baseline_dir.context("--baseline dir requires --baseline-dir <path>")?;
            if !dir.exists() {
                bail!("baseline dir not found: {}", dir.display());
            }
            dir.to_path_buf()
        }
    };

    Ok(Baseline { kind, root })
}

fn rpm_ostree_root() -> Result<PathBuf> {
    let output = Command::new("rpm-ostree")
        .args(["status", "--json"])
        .output()
        .context("run rpm-ostree status --json")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("rpm-ostree status --json failed: {}", stderr.trim());
    }

    let status: Status =
        serde_json::from_slice(&output.stdout).context("parse rpm-ostree status JSON")?;

    let deployment = status
        .deployments
        .iter()
        .find(|d| d.booted.unwrap_or(false))
        .or_else(|| status.deployments.first())
        .context("no deployments reported by rpm-ostree")?;

    let osname = deployment
        .osname
        .as_deref()
        .context("deployment missing osname")?;
    let deployment_checksum = deployment.checksum.as_deref();
    let base_checksum = deployment.base_checksum.as_deref();

    if deployment_checksum.is_none() && base_checksum.is_none() {
        bail!("deployment missing checksum");
    }

    if let Some(checksum) = deployment_checksum {
        match find_deployment_dir(osname, checksum) {
            Ok(root) => return Ok(root),
            Err(err) => {
                if let Some(base) = base_checksum {
                    if base != checksum {
                        if let Ok(root) = find_deployment_dir(osname, base) {
                            eprintln!(
                                "warn: deployment checksum not found; using base checksum {}",
                                base
                            );
                            return Ok(root);
                        }
                    }
                }
                return Err(err);
            }
        }
    }

    let checksum = base_checksum.context("deployment missing checksum")?;
    find_deployment_dir(osname, checksum)
}

fn find_deployment_dir(osname: &str, checksum: &str) -> Result<PathBuf> {
    let deploy_root = Path::new("/sysroot/ostree/deploy")
        .join(osname)
        .join("deploy");

    let mut matches = Vec::new();
    for entry in fs::read_dir(&deploy_root)
        .with_context(|| format!("read {}", deploy_root.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(checksum) {
            matches.push(entry.path());
        }
    }

    if matches.is_empty() {
        bail!(
            "no deployment dir matching {} under {}",
            checksum,
            deploy_root.display()
        );
    }

    matches.sort();
    Ok(matches.remove(0))
}

#[derive(Deserialize)]
struct Status {
    deployments: Vec<Deployment>,
}

#[derive(Deserialize)]
struct Deployment {
    booted: Option<bool>,
    osname: Option<String>,
    checksum: Option<String>,
    #[serde(rename = "base-checksum")]
    base_checksum: Option<String>,
}
