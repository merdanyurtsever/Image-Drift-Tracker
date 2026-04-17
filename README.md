# Image-Drift-Tracker

Track configuration drift on rpm-ostree systems by comparing your live OS to the base deployment. The CLI scans config-heavy paths, hashes file content, records metadata, and reports what changed with optional unified diffs.

## Status
Early implementation. The rpm-ostree baseline detector is in place, with more baseline sources planned.

## Features
- rpm-ostree baseline resolution from the booted deployment
- Fast filesystem scan with include/exclude rules
- Drift categories: added, removed, content-changed, metadata-changed, symlink-target-changed, type-changed
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
cargo run -- scan --baseline dir --baseline-dir /path/to/baseline
```

## Defaults
Unless `--no-defaults` is set, these are included:

- /etc
- /usr
- /boot
- /usr/local
- /opt
- ~/.config
- ~/.local/bin
- ~/.local/share
- ~/.bashrc
- ~/.zshrc
- ~/.profile
- ~/.ssh/config

Always excluded:

- /proc
- /sys
- /dev
- /run
- /tmp
- ~/.cache
- /var/log
- /var/cache
- /var/lib/containers

## Report Path
The latest drift report is written to:

- $XDG_STATE_HOME/image-drift-tracker/drift.json
- or ~/.local/state/image-drift-tracker/drift.json