#!/usr/bin/env bash
set -euo pipefail

repo="${GH_AI_CREDIT_PULSE_REPO:-probably-undefined/gh-ai-credit-pulse}"
ref="${GH_AI_CREDIT_PULSE_REF:-main}"
uuid="gh-ai-credit-pulse@probably-undefined"
data_home="${XDG_DATA_HOME:-${HOME}/.local/share}"
target_dir="${data_home}/gh-ai-credit-pulse"
extension_dir="${data_home}/gnome-shell/extensions/${uuid}"
bin_dir="${HOME}/.local/bin"
update=false
from_archive=false
enable_extension=true

while (($#)); do
    case "$1" in
        --update) update=true ;;
        --from-archive) from_archive=true ;;
        --no-extension) enable_extension=false ;;
        -h|--help)
            printf '%s\n' \
                'Usage: install.sh [--no-extension] [--update]' \
                '' \
                '  --no-extension  Install only the cross-platform Iced dashboard' \
                '  --update        Download and install the newest main branch'
            exit 0
            ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; exit 2 ;;
    esac
    shift
done

script_path="${BASH_SOURCE[0]:-}"
project_dir=""
if [[ -n "${script_path}" ]]; then
    project_dir="$(cd -- "$(dirname -- "${script_path}")" 2>/dev/null && pwd || true)"
fi

download_latest() {
    if ! command -v curl >/dev/null 2>&1; then
        printf 'Missing required command: curl\n' >&2
        exit 1
    fi
    download_dir="$(mktemp -d)"
    archive="${download_dir}/source.tar.gz"
    cleanup() { rm -rf -- "${download_dir}"; }
    trap cleanup EXIT
    printf 'Downloading %s@%s…\n' "${repo}" "${ref}"
    curl -fsSL "https://github.com/${repo}/archive/refs/heads/${ref}.tar.gz" -o "${archive}"
    tar -xzf "${archive}" -C "${download_dir}"
    source_dir="$(find "${download_dir}" -mindepth 1 -maxdepth 1 -type d -print -quit)"
    if [[ -z "${source_dir}" || ! -f "${source_dir}/install.sh" ]]; then
        printf 'Downloaded archive does not contain install.sh\n' >&2
        exit 1
    fi
    child_args=(--from-archive)
    [[ "${enable_extension}" == false ]] && child_args+=(--no-extension)
    bash "${source_dir}/install.sh" "${child_args[@]}"
    cleanup
    trap - EXIT
    exit 0
}

if [[ "${update}" == true && "${from_archive}" == false ]]; then
    download_latest
fi
if [[ ! -f "${project_dir}/extension/extension.js" || ! -f "${project_dir}/scripts/gh_ai_credits.py" ]]; then
    download_latest
fi

for command_name in curl gh /usr/bin/python3; do
    if ! command -v "${command_name}" >/dev/null 2>&1; then
        printf 'Missing required command: %s\n' "${command_name}" >&2
        exit 1
    fi
done

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) asset_name="gh-ai-credit-pulse-linux-x86_64" ;;
    *)
        printf 'No prebuilt binary for %s/%s yet.\n' "$(uname -s)" "$(uname -m)" >&2
        exit 1
        ;;
esac

binary_tmp="$(mktemp)"
trap 'rm -f -- "${binary_tmp}"' EXIT
binary_url="${GH_AI_CREDIT_PULSE_BINARY_URL:-https://github.com/${repo}/releases/download/latest/${asset_name}}"
printf 'Downloading Rust/Iced dashboard…\n'
if ! curl -fsSL "${binary_url}" -o "${binary_tmp}"; then
    if command -v cargo >/dev/null 2>&1; then
        printf 'No prebuilt release found; building it locally with Cargo…\n'
        cargo build --release --manifest-path "${project_dir}/Cargo.toml"
        install -m 0755 -- "${project_dir}/target/release/gh-ai-credit-pulse" "${binary_tmp}"
    else
        printf '%s\n' \
            'No prebuilt release is available yet and Cargo is not installed.' \
            'Please retry shortly after the GitHub build has completed.' >&2
        exit 1
    fi
fi
chmod 0755 "${binary_tmp}"

if [[ -e "${target_dir}" ]]; then
    backup_dir="${target_dir}.backup.$(date +%Y%m%d-%H%M%S)"
    cp -a -- "${target_dir}" "${backup_dir}"
    printf 'Existing installation backed up to %s\n' "${backup_dir}"
fi

install -d -- "${target_dir}/scripts" "${bin_dir}"
install -m 0755 -- "${binary_tmp}" "${target_dir}/gh-ai-credit-pulse-gui"
install -m 0755 -- "${project_dir}/install.sh" "${target_dir}/install.sh"
install -m 0755 -- "${project_dir}/gh-ai-credit-pulse" "${bin_dir}/gh-ai-credit-pulse"
install -m 0755 -- "${project_dir}/scripts/gh_ai_credits.py" "${target_dir}/scripts/"
install -m 0644 -- "${project_dir}/VERSION" "${target_dir}/VERSION"
install -m 0644 -- "${project_dir}/README.md" "${target_dir}/README.md"

if [[ "${enable_extension}" == true ]]; then
    if ! command -v gnome-extensions >/dev/null 2>&1; then
        printf 'GNOME Extensions CLI is missing; install package gnome-shell first.\n' >&2
        exit 1
    fi
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
