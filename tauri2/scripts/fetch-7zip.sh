#!/bin/sh
# Downloads the pinned official 7-Zip release that the desktop app bundles for
# RAR and 7z preparation, verifies each download against its pinned SHA-256,
# and lays out the sidecar, Windows' 7z.dll and each platform's license text.
# The pins were taken from identical copies on 7-zip.org and its GitHub
# releases. Linux gets the static 7zzs; Windows gets 7z.exe and 7z.dll from the
# installer, unpacked with that 7zzs. Keep tauri2/ui/src/includedSoftware.ts
# in step with the version.
set -eu
version=2604
linux_sha256=fc0327ba27e89bd086cf426dff17d77de582953cdbbc10a6576540a06853ffcd
windows_sha256=d54bf805f9f3704d1e8db2fa3498ae7ef2df0312b40b558e7c71c734430a665d
base="https://github.com/ip7z/7zip/releases/download/26.04"
dir="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/binaries"
linux_bin="$dir/dla-7zip-x86_64-unknown-linux-gnu"
windows_bin="$dir/dla-7zip-x86_64-pc-windows-msvc.exe"
mkdir -p "$dir/7zip/linux" "$dir/7zip/windows"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

fetch() {
  asset=$1 expected=$2
  curl -fsSL --retry 3 -o "$work/$asset" "$base/$asset"
  if ! echo "$expected  $work/$asset" | sha256sum -c --status; then
    echo "checksum mismatch for $asset; nothing was installed" >&2
    exit 1
  fi
}

fetch "7z$version-linux-x64.tar.xz" "$linux_sha256"
mkdir "$work/linux"
tar -xJf "$work/7z$version-linux-x64.tar.xz" -C "$work/linux" 7zzs License.txt
fetch "7z$version-x64.exe" "$windows_sha256"
"$work/linux/7zzs" x -y -bd -bso0 -o"$work/windows" "$work/7z$version-x64.exe" \
  7z.exe 7z.dll License.txt

install -m 0755 "$work/linux/7zzs" "$linux_bin"
install -m 0644 "$work/linux/License.txt" "$dir/7zip/linux/License.txt"
install -m 0755 "$work/windows/7z.exe" "$windows_bin"
install -m 0644 "$work/windows/7z.dll" "$dir/7zip/windows/7z.dll"
install -m 0644 "$work/windows/License.txt" "$dir/7zip/windows/License.txt"
echo "installed 7-Zip 26.04 for Linux and Windows in $dir"
