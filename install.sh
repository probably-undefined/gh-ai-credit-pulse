#!/usr/bin/env bash
set -euo pipefail

# This identity is intentionally not configurable. Updates must always verify
# against the canonical repository, never against a similarly named fork.
readonly repo="probably-undefined/gh-ai-credit-pulse"
readonly uuid="gh-ai-credit-pulse@probably-undefined"
readonly bundle_pattern='^gh-ai-credit-pulse-linux-x86_64-([0-9a-f]{12})\.tar\.gz$'

data_home="${XDG_DATA_HOME:-${HOME}/.local/share}"
target_dir="${data_home}/gh-ai-credit-pulse"
extension_dir="${data_home}/gnome-shell/extensions/${uuid}"
applications_dir="${data_home}/applications"
icons_dir="${data_home}/icons/hicolor/256x256/apps"
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
    for command_name in gh grep sha256sum tar; do
        require_command "${command_name}"
    done
    if ! gh attestation --help >/dev/null 2>&1; then
        printf '%s\n' \
            'Your GitHub CLI is too old to verify release provenance.' \
            'Update gh, then run this installer again. Installation stopped safely.' >&2
        exit 1
    fi

    # GitHub's /releases/latest endpoint intentionally excludes pre-releases.
    # Let gh perform the JSON parsing, then validate the tag and exact asset pair
    # in Bash. This keeps the bootstrap path independent of Python.
    release_tags=""
    if ! release_tags="$(
        gh release list --repo "${repo}" --limit 30 \
            --json tagName,isDraft,publishedAt \
            --jq 'sort_by(.publishedAt) | reverse | .[] | select(.isDraft == false) | .tagName' \
            2>/dev/null
    )"; then
        printf '%s\n' \
            'No published gh-ai-credit-pulse release is currently available.' \
            'Installation stopped safely; try again after the release workflow finishes.' >&2
        exit 1
    fi

    release_tag=""
    bundle_name=""
    while IFS= read -r candidate_tag; do
        [[ "${candidate_tag}" =~ ^build-([0-9a-f]{12})$ ]] || continue
        candidate_sha="${BASH_REMATCH[1]}"
        candidate_bundle="gh-ai-credit-pulse-linux-x86_64-${candidate_sha}.tar.gz"
        assets="$(
            gh release view "${candidate_tag}" --repo "${repo}" \
                --json assets --jq '.assets[].name' 2>/dev/null || true
        )"
        [[ "$(grep -Fxc -- "${candidate_bundle}" <<<"${assets}" || true)" == 1 ]] || continue
        [[ "$(grep -Fxc -- "${candidate_bundle}.sha256" <<<"${assets}" || true)" == 1 ]] || continue
        release_tag="${candidate_tag}"
        bundle_name="${candidate_bundle}"
        break
    done <<<"${release_tags}"

    if [[ -z "${release_tag}" || -z "${bundle_name}" ]]; then
        printf '%s\n' \
            'GitHub returned no valid published build release.' \
            'Installation stopped safely; no files were changed.' >&2
        exit 1
    fi

    if [[ ! "${release_tag}" =~ ^build-([0-9a-f]{12})$ ]]; then
        printf 'GitHub returned an unexpected release tag: %s\n' "${release_tag}" >&2
        exit 1
    fi
    tag_sha="${BASH_REMATCH[1]}"

    if [[ ! "${bundle_name}" =~ ${bundle_pattern} ]]; then
        printf 'GitHub returned an unexpected release asset: %s\n' "${bundle_name}" >&2
        exit 1
    fi
    asset_sha="${BASH_REMATCH[1]}"

    if [[ "${tag_sha}" != "${asset_sha}" ]]; then
        printf 'Release tag and asset commit do not match.\n' >&2
        exit 1
    fi

    download_dir="$(mktemp -d)"
    archive="${download_dir}/${bundle_name}"
    checksum="${archive}.sha256"
    cleanup() { rm -rf -- "${download_dir}"; }
    trap cleanup EXIT

    printf 'Downloading verified release %s from %s…\n' "${release_tag}" "${repo}"
    gh release download "${release_tag}" --repo "${repo}" --dir "${download_dir}" \
        --pattern "${bundle_name}" --pattern "${bundle_name}.sha256"

    (cd "${download_dir}" && sha256sum --check --strict "${bundle_name}.sha256")
    gh attestation verify "${archive}" \
        --repo "${repo}" \
        --signer-workflow "${repo}/.github/workflows/build.yml" \
        --source-ref "refs/heads/main" \
        --deny-self-hosted-runners >/dev/null
    printf 'Checksum and GitHub build provenance verified.\n'

    while IFS= read -r entry; do
        if [[ -z "${entry}" || "${entry}" == /* || "${entry}" == *'/../'* ||
              "${entry}" == '../'* || "${entry}" != gh-ai-credit-pulse/* ]]; then
            printf 'Unsafe archive path: %s\n' "${entry}" >&2
            exit 1
        fi
    done < <(tar -tzf "${archive}")
    while IFS= read -r listing; do
        entry_type="${listing:0:1}"
        if [[ "${entry_type}" != '-' && "${entry_type}" != 'd' ]]; then
            printf 'Unsupported archive entry type: %s\n' "${entry_type}" >&2
            exit 1
        fi
    done < <(tar -tvzf "${archive}")
    tar -xzf "${archive}" --no-same-owner --no-same-permissions -C "${download_dir}"

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
      ! -x "${project_dir}/gh-ai-credit-pulse-collector" ||
      ! -f "${project_dir}/assets/io.github.probably_undefined.GhAiCreditPulse.desktop" ||
      ! -f "${project_dir}/assets/gh-ai-credit-pulse.png" ||
      ! -f "${project_dir}/extension/extension.js" ]]; then
    printf 'Verified release bundle is incomplete; refusing to install.\n' >&2
    exit 1
fi

require_command gh

if [[ -e "${target_dir}" ]]; then
    backup_dir="${target_dir}.backup.$(date +%Y%m%d-%H%M%S)"
    cp -a -- "${target_dir}" "${backup_dir}"
    printf 'Existing installation backed up to %s\n' "${backup_dir}"
fi

install -d -- "${target_dir}/assets" "${applications_dir}" "${icons_dir}" "${bin_dir}"
install -m 0755 -- "${project_dir}/gh-ai-credit-pulse-gui" "${target_dir}/gh-ai-credit-pulse-gui"
install -m 0755 -- "${project_dir}/gh-ai-credit-pulse-collector" \
    "${target_dir}/gh-ai-credit-pulse-collector"
install -m 0755 -- "${project_dir}/install.sh" "${target_dir}/install.sh"
install -m 0755 -- "${project_dir}/gh-ai-credit-pulse" "${bin_dir}/gh-ai-credit-pulse"
rm -f -- "${target_dir}/scripts/gh_ai_credits.py"
rmdir -- "${target_dir}/scripts" 2>/dev/null || true
install -m 0644 -- "${project_dir}/assets/gh-ai-credit-pulse.png" \
    "${target_dir}/assets/gh-ai-credit-pulse.png"
install -m 0644 -- "${project_dir}/VERSION" "${target_dir}/VERSION"
install -m 0644 -- "${project_dir}/README.md" "${target_dir}/README.md"
install -m 0644 -- "${project_dir}/assets/gh-ai-credit-pulse.png" \
    "${icons_dir}/gh-ai-credit-pulse.png"
sed "s|@EXEC@|${bin_dir}/gh-ai-credit-pulse|g" \
    "${project_dir}/assets/io.github.probably_undefined.GhAiCreditPulse.desktop" > \
    "${applications_dir}/io.github.probably_undefined.GhAiCreditPulse.desktop"
chmod 0644 "${applications_dir}/io.github.probably_undefined.GhAiCreditPulse.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${applications_dir}" >/dev/null 2>&1 || true
fi

if [[ "${enable_extension}" == true ]]; then
    require_command gnome-extensions
    extension_was_installed=false
    [[ -f "${extension_dir}/extension.js" ]] && extension_was_installed=true

    install -d -- "${extension_dir}"
    install -m 0644 -- "${project_dir}/extension/extension.js" "${extension_dir}/extension.js"
    install -m 0644 -- "${project_dir}/extension/metadata.json" "${extension_dir}/metadata.json"
    install -m 0644 -- "${project_dir}/extension/stylesheet.css" "${extension_dir}/stylesheet.css"
    rm -f -- "${extension_dir}/scripts/gh_ai_credits.py"
    rmdir -- "${extension_dir}/scripts" 2>/dev/null || true

    if [[ "${extension_was_installed}" == true ]]; then
        # GNOME 42 caches extension modules for the lifetime of the Wayland
        # session. Disable/enable only calls the old module again.
        gnome-extensions enable "${uuid}" >/dev/null 2>&1 || true
        printf '%s\n' \
            "Updated GNOME extension ${uuid}." \
            'GNOME 42 requires one logout/login to load changed extension code.'
    elif gnome-extensions enable "${uuid}"; then
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
