#!/usr/bin/env bash
set -euo pipefail

# This identity is intentionally not configurable. Updates must always verify
# against the canonical repository, never against a similarly named fork.
readonly repo="probably-undefined/gh-ai-credit-pulse"
readonly uuid="gh-ai-credit-pulse@probably-undefined"
readonly bundle_name="gh-ai-credit-pulse-linux-x86_64.tar.gz"
readonly release_base="https://github.com/${repo}/releases/latest/download"

data_home="${XDG_DATA_HOME:-${HOME}/.local/share}"
target_dir="${data_home}/gh-ai-credit-pulse"
extension_dir="${data_home}/gnome-shell/extensions/${uuid}"
bin_dir="${HOME}/.local/bin"
from_bundle=false
enable_extension=true

while (($#)); do
    case "$1" in
        --update) ;;
        --from-bundle) from_bundle=true ;;
        --no-extension) enable_extension=false ;;
        -h|--help)
            printf '%s\n' \
                'Usage: install.sh [--no-extension] [--update]' \
                '' \
                '  --no-extension  Install only the cross-platform Iced dashboard' \
                '  --update        Download and install the newest verified release'
            exit 0
            ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
    esac
    shift
done

# BASH_SOURCE can be an empty special array when this script arrives via a
# pipe. Expanding the array itself is safe on the older Bash in Ubuntu 22.04.
script_path="${BASH_SOURCE:-}"
project_dir=""
if [[ -n "${script_path}" ]]; then
    project_dir="$(cd -- "$(dirname -- "${script_path}")" 2>/dev/null && pwd || true)"
fi

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'Missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

download_verified_bundle() {
    for command_name in curl gh sha256sum tar /usr/bin/python3; do
        require_command "${command_name}"
    done
    if ! gh attestation --help >/dev/null 2>&1; then
        printf '%s\n' \
            'Your GitHub CLI is too old to verify release provenance.' \
            'Update gh, then run this installer again. Installation stopped safely.' >&2
        exit 1
    fi

    download_dir="$(mktemp -d)"
    archive="${download_dir}/${bundle_name}"
    checksum="${archive}.sha256"
    cleanup() { rm -rf -- "${download_dir}"; }
    trap cleanup EXIT

    printf 'Downloading verified release from %s…\n' "${repo}"
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
        "${release_base}/${bundle_name}" -o "${archive}"
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
        "${release_base}/${bundle_name}.sha256" -o "${checksum}"

    (cd "${download_dir}" && sha256sum --check --strict "${bundle_name}.sha256")
    gh attestation verify "${archive}" --repo "${repo}" >/dev/null
    printf 'Checksum and GitHub build provenance verified.\n'

    /usr/bin/python3 - "${archive}" "${download_dir}" <<'PY'
import pathlib
import sys
import tarfile

archive, destination = sys.argv[1:]
with tarfile.open(archive, "r:gz") as bundle:
    for member in bundle.getmembers():
        path = pathlib.PurePosixPath(member.name)
        if path.is_absolute() or ".." in path.parts or not path.parts:
            raise SystemExit(f"Unsafe archive path: {member.name}")
        if path.parts[0] != "gh-ai-credit-pulse":
            raise SystemExit(f"Unexpected archive root: {member.name}")
        if not (member.isfile() or member.isdir()):
            raise SystemExit(f"Unsupported archive entry: {member.name}")
    bundle.extractall(destination)
PY

    child_args=(--from-bundle)
    [[ "${enable_extension}" == false ]] && child_args+=(--no-extension)
    bash "${download_dir}/gh-ai-credit-pulse/install.sh" "${child_args[@]}"
    cleanup
    trap - EXIT
    exit 0
}

if [[ "${from_bundle}" == false ]]; then
    download_verified_bundle
fi

if [[ ! -x "${project_dir}/gh-ai-credit-pulse-gui" ||
      ! -f "${project_dir}/extension/extension.js" ||
      ! -f "${project_dir}/scripts/gh_ai_credits.py" ]]; then
    printf 'Verified release bundle is incomplete; refusing to install.\n' >&2
    exit 1
fi

for command_name in gh /usr/bin/python3; do
    require_command "${command_name}"
done

if [[ -e "${target_dir}" ]]; then
    backup_dir="${target_dir}.backup.$(date +%Y%m%d-%H%M%S)"
    cp -a -- "${target_dir}" "${backup_dir}"
    printf 'Existing installation backed up to %s\n' "${backup_dir}"
fi

install -d -- "${target_dir}/scripts" "${bin_dir}"
install -m 0755 -- "${project_dir}/gh-ai-credit-pulse-gui" "${target_dir}/gh-ai-credit-pulse-gui"
install -m 0755 -- "${project_dir}/install.sh" "${target_dir}/install.sh"
install -m 0755 -- "${project_dir}/gh-ai-credit-pulse" "${bin_dir}/gh-ai-credit-pulse"
install -m 0755 -- "${project_dir}/scripts/gh_ai_credits.py" "${target_dir}/scripts/"
install -m 0644 -- "${project_dir}/VERSION" "${target_dir}/VERSION"
install -m 0644 -- "${project_dir}/README.md" "${target_dir}/README.md"

if [[ "${enable_extension}" == true ]]; then
    require_command gnome-extensions
    install -d -- "${extension_dir}/scripts"
    install -m 0644 -- "${project_dir}/extension/extension.js" "${extension_dir}/extension.js"
    install -m 0644 -- "${project_dir}/extension/metadata.json" "${extension_dir}/metadata.json"
    install -m 0644 -- "${project_dir}/extension/stylesheet.css" "${extension_dir}/stylesheet.css"
    install -m 0755 -- "${project_dir}/scripts/gh_ai_credits.py" "${extension_dir}/scripts/"

    gnome-extensions disable "${uuid}" >/dev/null 2>&1 || true
    if gnome-extensions enable "${uuid}"; then
        printf 'Enabled GNOME extension %s\n' "${uuid}"
    else
        printf '%s\n' \
            'The extension is installed but GNOME Shell has not discovered it yet.' \
            'Log out and back in once, then run:' \
            "  gnome-extensions enable ${uuid}"
    fi
fi

version="$(tr -d '[:space:]' <"${project_dir}/VERSION")"
printf '\nInstalled gh-ai-credit-pulse %s\n' "${version}"
printf 'Open dashboard: %s/gh-ai-credit-pulse\n' "${bin_dir}"
printf 'Update later:   %s/gh-ai-credit-pulse --self-update\n' "${bin_dir}"
