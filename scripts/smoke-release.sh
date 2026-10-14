#!/usr/bin/env bash
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
archive=$(realpath "${1:?Usage: scripts/smoke-release.sh archive.tar.gz}")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
tar -xzf "$archive" -C "$work"
package=$(find "$work" -mindepth 1 -maxdepth 1 -type d)
cd "$work" # Exercise relocation from a working directory without data.
"$package/fast-spektrafilm" --version
"$package/fast-spektrafilm-gui" --version
for binary in fast-spektrafilm fast-spektrafilm-gui; do
    "$package/$binary" --help > "$work/$binary-help.txt"
    rg -F -- "$package/data" "$work/$binary-help.txt"
done
"$package/fast-spektrafilm" profiles > "$work/profiles.txt"
[[ -s "$work/profiles.txt" ]]
"$package/fast-spektrafilm" defaults | jq -e '.io.copy_metadata' > /dev/null
"$package/fast-spektrafilm" --threads 2 process "$repo/tests/fixtures/debt/file_input.npy" \
    "$work/render.npy" --deterministic --no-spatial --no-auto-exposure
[[ -s "$work/render.npy" ]]
# --data-dir must still override automatic discovery, even for installed binaries.
if "$package/fast-spektrafilm" --data-dir "$work/missing" profiles > /dev/null 2>&1; then
    echo 'Explicit missing data directory was ignored' >&2
    exit 1
fi
if command -v xvfb-run > /dev/null; then
    status=0
    SLINT_BACKEND=winit-software xvfb-run -a timeout 10s "$package/fast-spektrafilm-gui" \
        > "$work/gui.log" 2>&1 || status=$?
    cat "$work/gui.log"
    [[ "$status" == 124 ]] || { echo "GUI exited before its event loop: $status" >&2; exit 1; }
    echo 'GUI stayed in its event loop under Xvfb'
elif [[ ${GITHUB_ACTIONS:-false} == true ]]; then
    echo 'Xvfb is required for the release GUI smoke test' >&2
    exit 1
fi
echo 'Release archive: CLI data discovery, rendering and override passed'
