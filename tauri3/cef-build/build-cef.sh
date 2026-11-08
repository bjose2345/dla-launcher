#!/usr/bin/env bash
# Builds the CEF binary distribution the Chromium build runs on, with the
# decoders Spotify's prebuilt CEF leaves out (H.264, AAC, MP3, and H.265 in
# software through the patches/ directory), at the exact CEF version the
# pinned `cef` crate expects. Runs on the build VM (cef-forge: 24 threads,
# 64 GB, 300 GB) and takes hours; see README.md.
#
#   build-cef.sh fetch    # Chromium + CEF checkout at the pinned commit (once)
#   build-cef.sh patch    # apply patches/ to the checkout (build does this too)
#   build-cef.sh unpatch  # take them out again, for a plain H.264 build
#   build-cef.sh build    # Release build, minimal distribution tarball
#   build-cef.sh all      # fetch, patch, build
#
# Patches live beside this script in patches/ and are applied with git apply,
# skipping ones already in; the file name says which checkout they belong to
# (ffmpeg-* is third_party/ffmpeg, its own git repository; anything else is
# chromium/src). CEF_OUT is where the tarball and its archive.json land.
#
# The pins come from tauri3/.cef/<version>/cef_linux_x86_64/include/cef_version.h
# of the checkout that will consume the build; keep them in step with the
# `cef` crate in tauri3/src-tauri/Cargo.toml.
set -euo pipefail

CEF_BRANCH=${CEF_BRANCH:-8037}
CEF_COMMIT=${CEF_COMMIT:-14c5a089e8452874cb1a556dc3fde26ed2d87723}
CEF_VERSION=${CEF_VERSION:-154.0.34+g14c5a08+chromium-154.0.8037.98}
CEF_ROOT=${CEF_ROOT:-$HOME/cef}
CEF_OUT=${CEF_OUT:-$CEF_ROOT/out}
PATCHES_DIR=${PATCHES_DIR:-$(cd "$(dirname "$0")" && pwd)/patches}
AUTOMATE=$CEF_ROOT/automate/automate-git.py
# An official build (LTO, PGO) as Spotify ships, without symbols; the codec
# flags are the point of the exercise. use_siso=false keeps autoninja on plain
# ninja: siso, Chromium's newer runner, exits in offline mode on this VM.
export GN_DEFINES=${GN_DEFINES:-"is_official_build=true proprietary_codecs=true ffmpeg_branding=Chrome use_sysroot=true symbol_level=0 blink_symbol_level=0 v8_symbol_level=0 use_thin_lto=true use_siso=false"}
export CEF_ARCHIVE_FORMAT=${CEF_ARCHIVE_FORMAT:-tar.bz2}
# The link of libcef.so opens tens of thousands of files.
ulimit -n 65536 || true

fetch() {
  mkdir -p "$CEF_ROOT/automate"
  if [ ! -f "$AUTOMATE" ]; then
    curl -fsSL -o "$AUTOMATE" https://raw.githubusercontent.com/chromiumembedded/cef/master/tools/automate/automate-git.py
  fi
  python3 "$AUTOMATE" --download-dir="$CEF_ROOT" --branch="$CEF_BRANCH" --checkout="$CEF_COMMIT" \
    --x64-build --no-build --no-distrib
  # Chromium's own list of host packages; needs sudo.
  "$CEF_ROOT/chromium/src/build/install-build-deps.sh" --no-prompt
}

patch_dir() {
  case "$(basename "$1")" in
    ffmpeg-*) echo "$CEF_ROOT/chromium/src/third_party/ffmpeg" ;;
    *) echo "$CEF_ROOT/chromium/src" ;;
  esac
}

apply_patches() {
  local file dir
  for file in "$PATCHES_DIR"/*.patch; do
    [ -e "$file" ] || continue
    dir=$(patch_dir "$file")
    if git -C "$dir" apply --check --reverse "$file" >/dev/null 2>&1; then
      echo "already applied: $(basename "$file")"
    else
      git -C "$dir" apply --check "$file"
      git -C "$dir" apply "$file"
      echo "applied: $(basename "$file") in $dir"
    fi
  done
}

remove_patches() {
  local file dir
  for file in "$PATCHES_DIR"/*.patch; do
    [ -e "$file" ] || continue
    dir=$(patch_dir "$file")
    if git -C "$dir" apply --check --reverse "$file" >/dev/null 2>&1; then
      git -C "$dir" apply --reverse "$file"
      echo "removed: $(basename "$file") from $dir"
    else
      echo "not applied: $(basename "$file")"
    fi
  done
}

build() {
  apply_patches
  # An official build wants Chromium's PGO profile, which the checkout does
  # not fetch on its own (gn stops with "requested profile ... doesn't exist").
  (cd "$CEF_ROOT/chromium/src" && PATH="$CEF_ROOT/depot_tools:$PATH" python3 tools/update_pgo_profiles.py \
    --target=linux update --gs-url-base=chromium-optimization-profiles/pgo_profiles)
  # V8's builtins profile, the same way (snapshot_blob.bin needs x64.profile).
  # The target is cefsimple, not the default cefclient: the minimal
  # distribution ships neither, and cefclient needs GTK headers the sysroot
  # does not carry. automate-git adds chrome_sandbox on Linux.
  (cd "$CEF_ROOT/chromium/src" && PATH="$CEF_ROOT/depot_tools:$PATH" python3 v8/tools/builtins-pgo/download_profiles.py download --depot-tools="$CEF_ROOT/depot_tools")
  python3 "$AUTOMATE" --download-dir="$CEF_ROOT" --branch="$CEF_BRANCH" --checkout="$CEF_COMMIT" \
    --x64-build --no-update --force-build --no-debug-build --minimal-distrib-only \
    --build-target=cefsimple
  local archive
  archive=$(ls -t "$CEF_ROOT"/chromium/src/cef/binary_distrib/cef_binary_*_linux64_minimal.tar.bz2 | head -1)
  local sha1
  sha1=$(sha1sum "$archive" | cut -d' ' -f1)
  mkdir -p "$CEF_OUT"
  cp "$archive" "$CEF_OUT/"
  printf '{\n  "type": "minimal",\n  "name": "%s",\n  "sha1": "%s"\n}\n' "$(basename "$archive")" "$sha1" > "$CEF_OUT/$(basename "$archive").archive.json"
  echo "built: $CEF_OUT/$(basename "$archive") (sha1 $sha1)"
  echo "check: $(grep -E 'define CEF_VERSION ' "$CEF_ROOT/chromium/src/cef/include/cef_version.h" 2>/dev/null || true)"
}

case "${1:-all}" in
  fetch) fetch ;;
  patch) apply_patches ;;
  unpatch) remove_patches ;;
  build) build ;;
  all) fetch; build ;;
  *) echo "usage: $0 fetch|patch|unpatch|build|all" >&2; exit 2 ;;
esac
