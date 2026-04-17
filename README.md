# Image-Drift-Tracker

Track configuration drift on rpm-ostree systems by comparing your live OS to the base deployment. The CLI scans config-heavy paths, hashes file content, records metadata, and reports what changed with optional unified diffs.

## Status
Early implementation. The rpm-ostree baseline detector is in place, with more baseline sources planned.

## Features
- rpm-ostree baseline resolution from the booted deployment
- Parallel filesystem scan with include/exclude rules
- Drift categories: added, removed, content-changed, metadata-changed, symlink-target-changed, type-changed
- Metadata-only paths to skip hashing immutable trees (e.g., /usr)
- Colorized summary output and optional unified diffs
- JSON report persisted for search

## Install
Requires Rust (cargo).

```
cargo build --release
```

## Usage
```
cargo run -- scan
cargo run -- scan --diff
cargo run -- scan --no-defaults --include /etc --include ~/.config
cargo run -- scan --usr-metadata-only
cargo run -- scan --metadata-only /usr --metadata-only /var/lib/NetworkManager
cargo run -- scan --baseline dir --baseline-dir /path/to/baseline
```

## Defaults
Unless `--no-defaults` is set, these are included:

- /etc
- /usr
- /usr/local
- /opt
- ~/.config
- ~/.local/bin
- ~/.bashrc
- ~/.zshrc
- ~/.profile
- ~/.ssh/config

Always excluded:

- /proc
- /sys
- /sysroot
- /boot
- /dev
- /run
- /tmp
- ~/.cache
- ~/.local/share
- ~/.local/share/containers
- ~/.local/share/flatpak
- ~/.config/Google
- ~/.config/GNS3
- /var/log
- /var/cache
- /var/lib/containers

These defaults avoid container and Flatpak storage churn, skip /sysroot to prevent recursive ostree deployment scans, and ignore noisy app state under ~/.config.

Directories under ~/.config ending with `-backup` are also excluded by default (for example, Android Studio timestamped backups).

## Metadata-Only Paths
Use metadata-only mode to avoid hashing large immutable trees while still tracking ownership, permissions, and size changes.

- `--usr-metadata-only` marks /usr as metadata-only, while /usr/local stays fully hashed
- `--metadata-only <path>` can be repeated to mark additional paths

## Driftignore
Create a `.driftignore` file to exclude paths with gitignore-style patterns. The tool loads:

- `./.driftignore` (current working directory)
- `$XDG_CONFIG_HOME/image-drift-tracker/.driftignore` (or `~/.config/image-drift-tracker/.driftignore`)

Patterns are matched against absolute paths rooted at `/`.

Example:
```
# Ignore Android Studio backups anywhere
**/*-backup/

# Ignore logs and databases
**/*.log
**/*.db

# Ignore specific apps
/home/you/.config/Google/
/home/you/.config/GNS3/
```

## Performance
Scanning and hashing run across all available cores. Limit CPU usage with:

```
RAYON_NUM_THREADS=4 cargo run -- scan
```

## Report Path
The latest drift report is written to:

- $XDG_STATE_HOME/image-drift-tracker/drift.json
- or ~/.local/state/image-drift-tracker/drift.json