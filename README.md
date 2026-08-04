# GitHub AI Credit Pulse

[![build](https://github.com/probably-undefined/gh-ai-credit-pulse/actions/workflows/build.yml/badge.svg)](https://github.com/probably-undefined/gh-ai-credit-pulse/actions/workflows/build.yml)

A modern GitHub Copilot AI-credit dashboard built with Rust and
[Iced](https://iced.rs/), with native GNOME Shell integration for Ubuntu
Wayland.

The GNOME top bar shows only the current cost, for example `$30.27`. Hovering
opens a compact dashboard. Clicking **Open full dashboard** launches the
cross-platform Iced application.

The conversion is fixed at **100 AIC = $1.00**.

Version 1.1 adds a high-contrast violet/cyan visual system, a cycle hero,
velocity and projection signals, a 14-day pulse chart, and a redesigned GNOME
hover dashboard while keeping the idle top-bar footprint to the dollar value.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/probably-undefined/gh-ai-credit-pulse/main/install.sh | bash
```

The installer downloads the latest provenance-attested release bundle, verifies
its SHA-256 checksum and GitHub build identity, installs the GNOME 42 extension,
enables it, and creates `~/.local/bin/gh-ai-credit-pulse`. An up-to-date GitHub
CLI with `gh attestation` support is required; verification fails closed.

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
gh-ai-credit-pulse --self-update
```

Running the curl installer again performs the same update. Existing application
files are backed up before replacement. The SQLite history remains under
`~/.local/state/gh-ai-credits/history.sqlite3`.

## Requirements

- Linux x86-64 for the current prebuilt release
- GNOME Shell 42 for the included Ubuntu top-bar extension
- authenticated [GitHub CLI](https://cli.github.com/)
- Python 3.10 or newer for the dependency-free collector

Verify the GitHub endpoint:

```bash
gh auth status
gh api /copilot_internal/user \
  --jq '.quota_snapshots.premium_interactions.credits_used'
```

## Commands

```bash
gh-ai-credit-pulse                 # open the Iced dashboard
gh-ai-credit-pulse --version
gh-ai-credit-pulse --self-update
```

Collector commands remain available from a checkout:

```bash
python3 scripts/gh_ai_credits.py sample --window 24h | jq
python3 scripts/gh_ai_credits.py dashboard --window 7d | jq
python3 scripts/gh_ai_credits.py export --output gh-ai-credit-history.csv
```

## Build

```bash
cargo build --release
```

GitHub Actions builds the Linux bundle on every push to `main` and publishes it
as the rolling `latest` release consumed by the installer.

## Supply-chain security

- Dependencies are locked in `Cargo.lock` and release builds use `--locked`.
- Every GitHub Action is pinned to an immutable full commit SHA.
- Pull requests and forks receive only `contents: read`; they cannot publish.
- The privileged publish job runs only for this canonical repository's `main`
  branch and never checks out or executes repository code.
- Releases use unique immutable tags instead of replacing a shared tag.
- The installer resolves `latest` once and pins both downloads to that exact
  immutable tag, preventing mixed assets during CDN cache propagation.
- The complete bundle is SHA-256 checked and carries GitHub/Sigstore build
  provenance. The installer verifies the exact signer workflow, canonical
  `main` ref, and GitHub-hosted runner policy before extracting anything.
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
