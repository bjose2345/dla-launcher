#!/bin/sh
# Downloads the pinned cloudflared release that the app bundles as a sidecar
# and verifies each file against the SHA-256 published in the release notes,
# plus the release's Apache license text. The sidecar is named dla-cloudflared
# because Linux packages install sidecars in /usr/bin, beside any cloudflared
# the system already has.
# It is the release DLA Collab bundles; a new version is adopted only after it
# passes the live downloads check in dla-collab's deploy/RECEIVER_0.5.0.md.
# Keep tauri2/ui/src/includedSoftware.ts in step with the version.
set -eu
version=2026.9.3
linux_sha256=77e26d8d900e0b8469f416239d14b5f296525fdf79fee6f511ef55609e3fbac2
windows_sha256=f096265ec2fcbe9bb6e2d64268db167ced3fcbb83d894bdb9e2fcdb26f2ea7e2
license_sha256=58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd
base="https://github.com/cloudflare/cloudflared/releases/download/$version"
dir="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"
mkdir -p "$dir"

fetch() {
  url=$1 target=$2 expected=$3 mode=$4
  if [ -f "$dir/$target" ] && echo "$expected  $dir/$target" | sha256sum -c --status; then
    echo "ok       $target"
    return
  fi
  tmp="$dir/$target.download"
  curl -fsSL --retry 3 -o "$tmp" "$url"
  if ! echo "$expected  $tmp" | sha256sum -c --status; then
    rm -f "$tmp"
    echo "checksum mismatch for $url; nothing was installed" >&2
    exit 1
  fi
  chmod "$mode" "$tmp"
  mv "$tmp" "$dir/$target"
  echo "fetched  $target"
}

mkdir -p "$dir/cloudflared"
fetch "$base/cloudflared-linux-amd64" dla-cloudflared-x86_64-unknown-linux-gnu "$linux_sha256" 0755
fetch "$base/cloudflared-windows-amd64.exe" dla-cloudflared-x86_64-pc-windows-msvc.exe "$windows_sha256" 0755
fetch "https://raw.githubusercontent.com/cloudflare/cloudflared/$version/LICENSE" \
  cloudflared/LICENSE "$license_sha256" 0644
