#!/usr/bin/env bash
# Vendors tauri-runtime-cef with the launcher's patches into vendor/ (gitignored),
# where Cargo.toml's [patch.crates-io] points. The runtime's resource handler
# cannot skip to a requested byte range (patches/*range-skip.patch, ADR 0028).
# Run inside the tauri3 container before the first build and whenever a patch
# changes; cargo rebuilds the crate on its own.
set -euo pipefail

version=3.0.0-alpha.7
here=$(cd "$(dirname "$0")" && pwd)
root=$here/..
destination=$root/vendor/tauri-runtime-cef
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

curl -fsSL "https://crates.io/api/v1/crates/tauri-runtime-cef/$version/download" | tar xz -C "$work"
for patch_file in "$root"/patches/tauri-runtime-cef-$version-*.patch; do
  patch -d "$work/tauri-runtime-cef-$version" -p1 < "$patch_file"
done
# A path dependency must not carry the registry's lock file or package metadata.
rm -f "$work/tauri-runtime-cef-$version/Cargo.lock" "$work/tauri-runtime-cef-$version/.cargo_vcs_info.json"
mkdir -p "$root/vendor"
rm -rf "$destination"
mv "$work/tauri-runtime-cef-$version" "$destination"
echo "tauri-runtime-cef $version vendored into $destination"
