# GitHub AI Credit Pulse

A modern GitHub Copilot AI-credit dashboard built with Rust and
[Iced](https://iced.rs/), with native GNOME Shell integration for Ubuntu
Wayland.

The GNOME top bar shows only the current cost, for example `$30.27`. Hovering
opens a compact dashboard. Clicking **Open full dashboard** launches the
cross-platform Iced application.

The conversion is fixed at **100 AIC = $1.00**.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/probably-undefined/gh-ai-credit-pulse/main/install.sh | bash
```

The installer downloads the latest prebuilt Rust binary, installs the GNOME 42
extension, enables it, and creates `~/.local/bin/gh-ai-credit-pulse`.

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

GitHub Actions builds the Linux binary on every push to `main` and publishes it
as the rolling `latest` release consumed by the installer.

## Data and privacy

The collector invokes the existing `gh` process and never reads or stores the
GitHub token itself. Samples are kept locally in SQLite for 180 days. Only the
latest raw API response is retained.

`/copilot_internal/user` is an internal GitHub endpoint. The collector supports
both `credits_used` and the older `entitlement - quota_remaining` form, but
GitHub may change this API without notice.
