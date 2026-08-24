#!/usr/bin/env bash
set -euo pipefail

task_dir="$(mktemp -d)"
cleanup() {
  WINEPREFIX="${task_dir}/wine-prefix" wineserver -k >/dev/null 2>&1 || true
  WINEPREFIX="${task_dir}/wine-prefix" wineserver -w >/dev/null 2>&1 || true
  rm -rf "${task_dir}"
}
trap cleanup EXIT

fixture="${task_dir}/dla-wine32-smoke.exe"
runtime_dir="${task_dir}/runtime"
mkdir -m 0700 "${runtime_dir}"
i686-w64-mingw32-gcc \
  -Os \
  -s \
  crates/dla-launch/tests/fixtures/wine32-smoke.c \
  -o "${fixture}"

fixture_kind="$(file -b "${fixture}")"
if [[ "${fixture_kind}" != *"PE32 executable"* || "${fixture_kind}" == *"PE32+"* ]]; then
  echo "expected a 32-bit PE32 fixture, got: ${fixture_kind}" >&2
  exit 1
fi

export DLA_WINE_BINARY=wine
export DLA_WINE32_FIXTURE="${fixture}"
export WINEARCH=win32
export WINEDEBUG=-all
export WINEPREFIX="${task_dir}/wine-prefix"
export XDG_RUNTIME_DIR="${runtime_dir}"

timeout --signal=TERM 120s xvfb-run -a cargo test \
  -p dla-launch \
  --test wine32_runtime \
  -- \
  --ignored \
  --nocapture
