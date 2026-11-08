#!/usr/bin/env bash
# Installs the Tauri 3 CLI that bundles the Chromium build, with its bundler
# patched: tauri-bundler 3.0.0-alpha.4 copies libEGL.so and libGLESv2.so into
# the deb and the AppImage, which CEF 154 no longer ships on Linux, so bundling
# stops with "No such file or directory". The patch skips CEF files the
# distribution lacks (upstream PRs tauri-apps/tauri#16338 and #16441; drop the
# patch once a released bundler has either). Run inside the tauri3 container; the sources and the
# build land under target/tauri-cli, the binary in ~/.cargo/bin.
set -euo pipefail

cli_version=3.0.0-alpha.5
bundler_version=3.0.0-alpha.4
here=$(cd "$(dirname "$0")" && pwd)
work=${TAURI_CLI_WORK:-$here/../target/tauri-cli}
patch_file=$here/../patches/tauri-bundler-$bundler_version-optional-cef-files.patch

mkdir -p "$work"
cd "$work"
for crate in "tauri-cli:$cli_version" "tauri-bundler:$bundler_version"; do
  name=${crate%%:*}
  version=${crate##*:}
  rm -rf "$name-$version"
  curl -fsSL "https://crates.io/api/v1/crates/$name/$version/download" | tar xz
done
patch -d "tauri-bundler-$bundler_version" -p1 < "$patch_file"
# Both crates sit under the tauri3 workspace tree, so each declares its own.
printf '\n[workspace]\n' >> "tauri-bundler-$bundler_version/Cargo.toml"
printf '\n[patch.crates-io]\ntauri-bundler = { path = "../tauri-bundler-%s" }\n\n[workspace]\n' "$bundler_version" \
  >> "tauri-cli-$cli_version/Cargo.toml"
cargo install --path "tauri-cli-$cli_version" --target-dir "$work/target"
cargo tauri --version
