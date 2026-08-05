#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf -- "${test_root}"' EXIT

test_home="${test_root}/home"
data_home="${test_root}/data"
install_dir="${data_home}/gh-ai-credit-pulse"
mkdir -p "${test_home}/.local/bin" "${install_dir}"
cp "${project_dir}/VERSION" "${install_dir}/VERSION"

cat >"${install_dir}/gh-ai-credit-pulse-gui" <<'EOF'
#!/usr/bin/env bash
printf 'GUI:%s\n' "$*"
EOF
cat >"${install_dir}/gh-ai-credit-pulse-collector" <<'EOF'
#!/usr/bin/env bash
printf 'COLLECTOR:%s\n' "$*"
EOF
cat >"${install_dir}/install.sh" <<'EOF'
#!/usr/bin/env bash
printf 'INSTALL:%s\n' "$*"
EOF
cat >"${test_home}/.local/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "${1:-}" in
    auth) exit 0 ;;
    *) printf 'gh version test\n' ;;
esac
EOF
chmod +x \
    "${install_dir}/gh-ai-credit-pulse-gui" \
    "${install_dir}/gh-ai-credit-pulse-collector" \
    "${install_dir}/install.sh" \
    "${test_home}/.local/bin/gh"

run_wrapper() {
    HOME="${test_home}" \
        XDG_DATA_HOME="${data_home}" \
        XDG_STATE_HOME="${test_root}/state" \
        "${project_dir}/gh-ai-credit-pulse" "$@"
}

assert_contains() {
    local output="$1"
    local expected="$2"
    if [[ "${output}" != *"${expected}"* ]]; then
        printf 'Expected output to contain %q, got:\n%s\n' "${expected}" "${output}" >&2
        exit 1
    fi
}

output="$(run_wrapper)"
[[ "${output}" == 'GUI:' ]]

for help_arg in -h --help help; do
    output="$(run_wrapper "${help_arg}")"
    assert_contains "${output}" 'Usage: gh-ai-credit-pulse'
    [[ "${output}" != *'GUI:'* ]]
done

output="$(run_wrapper doctor)"
assert_contains "${output}" 'gh-ai-credit-pulse doctor'
assert_contains "${output}" '[ok]   collector starts headlessly'
[[ "${output}" != *'GUI:'* ]]

output="$(run_wrapper sample --window 24h)"
[[ "${output}" == 'COLLECTOR:sample --window 24h' ]]

output="$(run_wrapper --db /tmp/history.sqlite3 dashboard --window 7d)"
[[ "${output}" == 'COLLECTOR:--db /tmp/history.sqlite3 dashboard --window 7d' ]]

output="$(run_wrapper --db=/tmp/history.sqlite3 dashboard --window 7d)"
[[ "${output}" == 'COLLECTOR:--db=/tmp/history.sqlite3 dashboard --window 7d' ]]

output="$(run_wrapper --version)"
[[ "${output}" == 'GUI:--version' ]]

output="$(run_wrapper upgrade)"
[[ "${output}" == 'INSTALL:--update' ]]

for invalid_arg in --doctor --self-update self-update update --not-a-real-option; do
    set +e
    output="$(run_wrapper "${invalid_arg}" 2>&1)"
    status=$?
    set -e
    [[ "${status}" == 2 ]]
    assert_contains "${output}" "Unknown command or option: ${invalid_arg}"
    [[ "${output}" != *'GUI:'* ]]
done

printf 'wrapper CLI tests passed\n'
