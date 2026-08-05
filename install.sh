#!/usr/bin/env bash
set -euo pipefail

# This identity is intentionally not configurable. Updates must always verify
# against the canonical repository, never against a similarly named fork.
readonly repo="probably-undefined/gh-ai-credit-pulse"
readonly uuid="gh-ai-credit-pulse@probably-undefined"
readonly bundle_pattern='^gh-ai-credit-pulse-linux-x86_64-([0-9a-f]{12})\.tar\.gz$'
readonly release_tag="latest"

data_home="${XDG_DATA_HOME:-${HOME}/.local/share}"
target_dir="${data_home}/gh-ai-credit-pulse"
extension_dir="${data_home}/gnome-shell/extensions/${uuid}"
applications_dir="${data_home}/applications"
icons_dir="${data_home}/icons/hicolor/256x256/apps"
bin_dir="${HOME}/.local/bin"
systemd_user_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
readonly sampler_service="gh-ai-credit-pulse-sample.service"
readonly sampler_timer="gh-ai-credit-pulse-sample.timer"
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
    if ! gh attestation verify --help 2>/dev/null | grep -q -- '--source-digest'; then
        printf '%s\n' \
            'Your GitHub CLI is too old to verify release provenance.' \
            'Update gh, then run this installer again. Installation stopped safely.' >&2
        exit 1
    fi

    # Resolve the rolling release once, then bind its SHA-named asset to the
    # commit currently referenced by the tag. Provenance verification below
    # independently enforces the same full source digest.
    release_info=""
    if ! release_info="$(
        gh release view "${release_tag}" --repo "${repo}" \
            --json tagName,isDraft,assets \
            --jq '.tagName, (.isDraft | tostring), (.assets[].name)' 2>/dev/null
    )"; then
        printf '%s\n' \
            'No published gh-ai-credit-pulse release is currently available.' \
            'Installation stopped safely; try again after the release workflow finishes.' >&2
        exit 1
    fi

    mapfile -t release_fields <<<"${release_info}"
    if [[ "${release_fields[0]:-}" != "${release_tag}" ||
          "${release_fields[1]:-}" != 'false' ]]; then
        printf 'GitHub returned an invalid rolling release.\n' >&2
        exit 1
    fi

    source_sha="$(
        gh api "repos/${repo}/commits/${release_tag}" --jq '.sha' 2>/dev/null || true
    )"
    if [[ ! "${source_sha}" =~ ^[0-9a-f]{40}$ ]]; then
        printf 'GitHub returned an invalid release commit.\n' >&2
        exit 1
    fi
    source_short_sha="${source_sha:0:12}"
    bundle_name="gh-ai-credit-pulse-linux-x86_64-${source_short_sha}.tar.gz"
    assets="$(printf '%s\n' "${release_fields[@]:2}")"

    if [[ "$(grep -Fxc -- "${bundle_name}" <<<"${assets}" || true)" != 1 ||
          "$(grep -Fxc -- "${bundle_name}.sha256" <<<"${assets}" || true)" != 1 ]]; then
        printf 'GitHub returned an unexpected release asset set.\n' >&2
        exit 1
    fi

    if [[ ! "${bundle_name}" =~ ${bundle_pattern} ||
          "${BASH_REMATCH[1]}" != "${source_short_sha}" ]]; then
        printf 'Release commit and asset name do not match.\n' >&2
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
        --source-digest "${source_sha}" \
        --source-ref "refs/heads/main" \
        --deny-self-hosted-runners >/dev/null
    printf 'Checksum and GitHub build provenance verified.\n'

    while IFS= read -r entry; do
        if [[ -z "${entry}" || "${entry}" == /* || "${entry}" == *'/../'* ||
              "${entry}" == *'/..' || "${entry}" == '../'* ||
              "${entry}" != gh-ai-credit-pulse/* ]]; then
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
      ! -f "${project_dir}/systemd/${sampler_service}.in" ||
      ! -f "${project_dir}/systemd/${sampler_timer}" ||
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

install -d -- "${target_dir}/assets" "${target_dir}/systemd" \
    "${applications_dir}" "${icons_dir}" "${bin_dir}" "${systemd_user_dir}"
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
install -m 0644 -- "${project_dir}/systemd/${sampler_service}.in" \
    "${target_dir}/systemd/${sampler_service}.in"
install -m 0644 -- "${project_dir}/systemd/${sampler_timer}" \
    "${target_dir}/systemd/${sampler_timer}"
install -m 0644 -- "${project_dir}/assets/gh-ai-credit-pulse.png" \
    "${icons_dir}/gh-ai-credit-pulse.png"
sed "s|@EXEC@|${bin_dir}/gh-ai-credit-pulse|g" \
    "${project_dir}/assets/io.github.probably_undefined.GhAiCreditPulse.desktop" > \
    "${applications_dir}/io.github.probably_undefined.GhAiCreditPulse.desktop"
chmod 0644 "${applications_dir}/io.github.probably_undefined.GhAiCreditPulse.desktop"

collector_path="${target_dir}/gh-ai-credit-pulse-collector"
escaped_collector_path="$(printf '%s' "${collector_path}" | sed 's/[\\&|]/\\&/g')"
sed "s|@COLLECTOR@|${escaped_collector_path}|g" \
    "${project_dir}/systemd/${sampler_service}.in" > \
    "${systemd_user_dir}/${sampler_service}"
install -m 0644 -- "${project_dir}/systemd/${sampler_timer}" \
    "${systemd_user_dir}/${sampler_timer}"

if command -v systemctl >/dev/null 2>&1 &&
   systemctl --user daemon-reload >/dev/null 2>&1; then
    systemctl --user enable "${sampler_timer}" >/dev/null
    systemctl --user restart "${sampler_timer}"
    printf 'Enabled background sampling every two minutes.\n'
else
    printf '%s\n' \
        'Installed the systemd user timer, but the user service manager is unavailable.' \
        'Enable it after logging into the desktop:' \
        "  systemctl --user enable --now ${sampler_timer}"
fi

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
