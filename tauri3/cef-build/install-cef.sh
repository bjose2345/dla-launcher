#!/usr/bin/env bash
# Puts a CEF distribution built by build-cef.sh where the `cef` crate looks
# (CEF_PATH/<version>/cef_linux_x86_64, laid out as the crate's own download
# is, with the archive.json its build script checks), replacing Spotify's
# download of the same version.
#
#   install-cef.sh <cef_binary_...linux64_minimal.tar.bz2> [CEF_PATH]
#
# CEF_PATH defaults to tauri3/.cef, the compose default. The archive.json
# beside the tarball (written by build-cef.sh) is copied in; without it one is
# made from the file name and its sha1.
set -euo pipefail

archive=${1:?tarball}
cef_path=${2:-${CEF_PATH:-$(cd "$(dirname "$0")/.." && pwd)/.cef}}
name=$(basename "$archive")
version=$(printf '%s' "$name" | sed -E 's/^cef_binary_([0-9]+\.[0-9]+\.[0-9]+)\+.*/\1/')
[ "$version" != "$name" ] || { echo "not a CEF archive name: $name" >&2; exit 2; }

target=$cef_path/$version/cef_linux_x86_64
work=$(mktemp -d "$cef_path/extract.XXXXXX")
trap 'rm -rf "$work"' EXIT
tar -xjf "$archive" -C "$work"
extracted=$work/${name%.tar.bz2}
# The crate flattens the archive (download-cef's extract_target_archive):
# Release/ becomes the directory; Resources/*, CMakeLists.txt, cmake/, include/,
# libcef_dll/ and CREDITS.html move into it; the rest is dropped.
rm -rf "$target"
mkdir -p "$(dirname "$target")"
mv "$extracted/Release" "$target"
for entry in "$extracted"/Resources/* "$extracted"/CMakeLists.txt "$extracted"/cmake "$extracted"/include "$extracted"/libcef_dll "$extracted"/CREDITS.html; do
  [ -e "$entry" ] || continue
  mv "$entry" "$target/"
done
if [ -f "$archive.archive.json" ]; then
  cp "$archive.archive.json" "$target/archive.json"
else
  printf '{\n  "type": "minimal",\n  "name": "%s",\n  "sha1": "%s"\n}\n' "$name" "$(sha1sum "$archive" | cut -d' ' -f1)" > "$target/archive.json"
fi
echo "installed $name into $target"
grep -E 'define CEF_VERSION ' "$target/include/cef_version.h"
