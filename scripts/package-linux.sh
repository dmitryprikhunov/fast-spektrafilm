#!/usr/bin/env bash
set -euo pipefail

# Package already-built, native x86_64 Linux binaries and their original data.
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo"
tag=${1:?Usage: scripts/package-linux.sh vVERSION [binary-dir] [output-dir]}
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid release tag: $tag" >&2; exit 1; }
[[ $(uname -sm) == 'Linux x86_64' ]] || { echo 'Requires native Linux x86_64' >&2; exit 1; }
binary_dir=$(realpath "${2:-target/release}")
output=${3:-artifacts/release/dist}
mkdir -p "$output"
output=$(realpath "$output")
metadata=$(mktemp)
stage=$(mktemp -d "$output/.package-XXXXXX")
trap 'rm -f "$metadata"; rm -rf "$stage"' EXIT
cargo metadata --locked --format-version 1 --features gui --filter-platform x86_64-unknown-linux-gnu > "$metadata"
version=$(jq -r '.packages[] | select(.name == "fast-spektrafilm" and .source == null) | .version' "$metadata")
[[ "$tag" == "v$version" ]] || { echo "Tag $tag does not match Cargo version $version" >&2; exit 1; }
name="fast-spektrafilm-$version-linux-x86_64"
package="$stage/$name"
mkdir -p "$package/licenses/libraw" "$package/licenses/rust-dependencies"
for binary in fast-spektrafilm fast-spektrafilm-gui; do
    [[ -x "$binary_dir/$binary" ]]
    [[ $("$binary_dir/$binary" --version) == "$binary $version" ]]
    cp "$binary_dir/$binary" "$package/"
done
cp -a data "$package/"
cp -a licenses/. "$package/licenses/"
cp -a docs "$package/"
cp README.md GUI.md DEVELOPMENT.md RELEASES.md MIGRATION.md VALIDATION.md BENCHMARKS.md \
    LICENSE SPEKTRAFILM_LICENSE.txt BASELINE.json REFERENCE_MANIFEST.json Cargo.lock "$package/"
cp vendor/libraw/{COPYRIGHT,LICENSE.LGPL,LICENSE.CDDL,PROVENANCE.json} "$package/licenses/libraw/"
rust_notices=${SPEKTRAFILM_RUST_NOTICES:-$(rustc --print sysroot)/share/doc/rust}
[[ -f "$rust_notices/COPYRIGHT-library.html" ]] || {
    echo 'Rust library notices are required; use rustup or set SPEKTRAFILM_RUST_NOTICES to their directory' >&2
    exit 1
}
mkdir -p "$package/licenses/rust-standard-library"
cp "$rust_notices/COPYRIGHT-library.html" "$package/licenses/rust-standard-library/"
cp -a "$rust_notices/licenses" "$package/licenses/rust-standard-library/"

# Retain crate notices verbatim, including nested LICENSES and native vendor files.
jq -r '.packages[] | select(.source != null) | [.manifest_path,.name,.version] | @tsv' "$metadata" |
while IFS=$'\t' read -r manifest crate crate_version; do
    directory=$(dirname -- "$manifest")
    destination="$package/licenses/rust-dependencies/$crate-$crate_version"
    mkdir -p "$destination"
    while IFS= read -r notice; do
        relative=${notice#"$directory/"}
        case "${relative^^}" in
            LICENSE*|LICENCE*|COPYING*|COPYRIGHT*|NOTICE*|README*|*/LICENSE*|*/LICENCE*|*/COPYING*|*/COPYRIGHT*|*/NOTICE*)
                mkdir -p "$destination/$(dirname -- "$relative")"
                cp "$notice" "$destination/$relative"
                ;;
        esac
    done < <(rg --files --hidden --no-ignore "$directory")
done
{
    printf 'crate\tversion\tlicense\trepository\n'
    jq -r '.packages[] | select(.source != null) | [.name,.version,.license // "See source",.repository // ""] | @tsv' "$metadata"
} > "$package/licenses/RUST-DEPENDENCIES.tsv"

commit=$(git rev-parse HEAD)
jq -n --arg version "$version" --arg commit "$commit" --arg rustc "$(rustc --version)" \
    --arg libc "$(getconf GNU_LIBC_VERSION)" --arg runner "${ImageOS:-local}" \
    '{version:$version,commit:$commit,target:"x86_64-unknown-linux-gnu",features:["bundled-libraw","gui"],profile:"release",lto:"thin",codegen_units:1,rustc:$rustc,build_libc:$libc,runner:$runner}' \
    > "$package/BUILD.json"
for binary in fast-spektrafilm fast-spektrafilm-gui; do
    printf '%s\n' "$binary"
    ldd "$binary_dir/$binary"
done > "$package/DYNAMIC-LIBRARIES.txt"
printf 'Matching source: https://github.com/dmitryprikhunov/fast-spektrafilm/tree/%s\nSource archive: fast-spektrafilm-%s-source.tar.gz\nBuild instructions: DEVELOPMENT.md\n' \
    "$commit" "$version" > "$package/SOURCE.txt"
tar -C "$stage" -czf "$output/$name.tar.gz" "$name"
git archive --format=tar --prefix="fast-spektrafilm-$version-source/" HEAD |
    gzip -n > "$output/fast-spektrafilm-$version-source.tar.gz"
(cd "$output" && sha256sum "$name.tar.gz" "fast-spektrafilm-$version-source.tar.gz" > SHA256SUMS)
echo "$output/$name.tar.gz"
