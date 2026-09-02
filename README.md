# GitHub AI Credit Pulse

[![build](https://github.com/probably-undefined/gh-ai-credit-pulse/actions/workflows/build.yml/badge.svg)](https://github.com/probably-undefined/gh-ai-credit-pulse/actions/workflows/build.yml)

A GitHub Copilot usage dashboard built with Rust and [Iced](https://iced.rs/),
with a GNOME Shell extension for Ubuntu Wayland.

The GNOME top bar shows the current cost and hourly rate, for example
`$30.27 · $0.41/h`. Hovering opens the compact dashboard.

The conversion is fixed at **100 AIC = $1.00**.

The desktop dashboard uses Iced's software `tiny-skia` renderer. It does not
build or require the WGPU/Vulkan renderer.

The billing-cycle projection uses local working time: Monday through Friday,
06:00–19:00. Weekends and time outside that window add no projected usage;
historical totals and recent-rate metrics still reflect all recorded usage.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/probably-undefined/gh-ai-credit-pulse/main/install.sh | bash
```

The installer downloads the latest provenance-attested release bundle, verifies
its SHA-256 checksum and GitHub build identity, installs the GNOME 42 extension,
enables it, creates `~/.local/bin/gh-ai-credit-pulse`, and adds an **AI Credit
Pulse** launcher with its own application icon. It also enables a systemd user
timer that samples every two minutes, including while the desktop is locked.
An up-to-date GitHub CLI with `gh attestation` support is required;
verification fails closed.

If GNOME has not discovered a newly installed extension in the running Wayland
session, log out and back in once, then run:

```bash
gnome-extensions enable gh-ai-credit-pulse@probably-undefined
```

Install only the cross-platform dashboard without GNOME integration:

```bash
curl -fsSL https://raw.githubusercontent.com/probably-undefined/gh-ai-credit-pulse/main/install.sh \
  | bash -s -- --no-extension
```

## Update

```bash
gh-ai-credit-pulse upgrade
```

Running the curl installer again performs the same update. Existing application
files are backed up before replacement. The SQLite history remains under
`~/.local/state/gh-ai-credits/history.sqlite3`.

## Requirements

- Linux x86-64 for the current prebuilt release
- GNOME Shell 42 for the included Ubuntu top-bar extension
- authenticated [GitHub CLI](https://cli.github.com/)

Verify the GitHub endpoint:

```bash
gh auth status
gh api /copilot_internal/user \
  --jq '.quota_snapshots.premium_interactions.credits_used'
```

## Commands

```bash
gh-ai-credit-pulse                 # open the Iced dashboard
gh-ai-credit-pulse --help
gh-ai-credit-pulse doctor          # headless installation diagnostics
gh-ai-credit-pulse --version
gh-ai-credit-pulse upgrade
gh-ai-credit-pulse sample --window 24h | jq
gh-ai-credit-pulse dashboard --window 7d | jq
gh-ai-credit-pulse export --output gh-ai-credit-history.csv
```

All CLI dispatch happens before the graphical backend is initialized. Help,
diagnostics, version output, collector commands, and invalid arguments therefore
work without a Vulkan, Wayland, or X11 presentation device.

Normal polling is coordinated through SQLite: multiple dashboards and GNOME
instances share a 25-second freshness window and one expiring fetch lease.
`sample --force` bypasses the freshness window for an explicit refresh while
still respecting the cross-process lease.

## Background sampling

The installer enables `gh-ai-credit-pulse-sample.timer` for the current user.
The user service manager remains active while the GNOME desktop is locked, so
usage history no longer depends on the top-bar extension's refresh loop. A
persistent timer runs promptly after resume when the machine was suspended.

```bash
systemctl --user status gh-ai-credit-pulse-sample.timer
systemctl --user list-timers gh-ai-credit-pulse-sample.timer
journalctl --user -u gh-ai-credit-pulse-sample.service
```

Disable or re-enable background collection with:

```bash
systemctl --user disable --now gh-ai-credit-pulse-sample.timer
systemctl --user enable --now gh-ai-credit-pulse-sample.timer
```

## Architecture

- `gh-ai-credit-pulse` is the Iced application.
- `gh-ai-credit-pulse-collector` is the small headless CLI used by the GNOME
  extension, systemd sampler, and shell wrapper.
- `src/collector/` contains the shared GitHub client, SQLite store, data model,
  and usage calculations.

The GUI calls the collector library directly. The extension starts the
headless collector binary. Python is not used at runtime or during installation.

## Build

```bash
cargo test --all-targets
cargo build --release
```

GitHub Actions builds the Linux bundle on every push to `main` and updates one
rolling `latest` pre-release consumed by the installer. Its asset name contains
the source commit, while the attestation binds the complete source digest.

`Cargo.toml` is the single source of truth for the application version. The
release workflow reads it through the built binary and generates the bundle's
`VERSION` file automatically.

## Supply-chain security

- Dependencies are locked in `Cargo.lock` and release builds use `--locked`.
- Every GitHub Action is pinned to an immutable full commit SHA.
- Pull requests and forks receive only `contents: read`; they cannot publish.
- The privileged publish job runs only for this canonical repository's `main`
  branch and never checks out or executes repository code.
- Releases reuse a single rolling `latest` tag; the workflow removes superseded
  `build-*` releases and tags.
- Release assets include the source commit in their names. The installer
  matches that commit against the rolling tag before downloading either asset,
  preventing mixed files during CDN cache propagation.
- The complete bundle is SHA-256 checked and carries GitHub/Sigstore build
  provenance. The installer verifies the exact signer workflow, canonical
  `main` ref, full source commit digest, and GitHub-hosted runner policy before
  extracting anything.
- Archive paths and entry types are validated before extraction.

A fork can build its own copy, but it cannot produce an attestation whose
repository identity is `probably-undefined/gh-ai-credit-pulse`. The canonical
installer does not accept a repository override.

## Data and privacy

The collector invokes the existing `gh` process and never reads or stores the
GitHub token itself. Samples are kept locally in SQLite for 180 days. Only the
latest raw API response is retained.

`/copilot_internal/user` is an internal GitHub endpoint. The collector supports
both `credits_used` and the older `entitlement - quota_remaining` form, but
GitHub may change this API without notice.
