#!/usr/bin/env bash
set -euo pipefail

package="org.dlaproject.launcher.debug"
activity="org.dlaproject.launcher.MainActivity"
component="${package}/${activity}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "${script_dir}/.." && pwd)"
source "${script_dir}/android-runtime-common.sh"
apk="${DLA_ANDROID_APK:-${tauri_dir}/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk}"
cdp_port="${DLA_ANDROID_CDP_PORT:-9222}"
artifact_dir="${DLA_ANDROID_ARTIFACT_DIR:-}"
cold_cycles="${DLA_ANDROID_COLD_CYCLES:-3}"

if [[ ! "${cold_cycles}" =~ ^[1-9][0-9]*$ ]]; then
  echo "DLA_ANDROID_COLD_CYCLES must be a positive integer" >&2
  exit 1
fi

android_require_tools adb curl jq

if [[ ! -f "${apk}" ]]; then
  echo "Android APK was not found: ${apk}" >&2
  exit 1
fi

android_resolve_adb
adb=("${DLA_ANDROID_ADB[@]}")

cleanup() {
  "${adb[@]}" forward --remove "tcp:${cdp_port}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

wait_for_route() {
  local expected="$1"
  local route=""
  local attempt
  for attempt in $(seq 1 60); do
    route="$(android_current_route "${cdp_port}" 2>/dev/null || true)"
    if [[ "${route}" == "${expected}" ]]; then
      return
    fi
    sleep 1
  done
  echo "expected WebView route ${expected}, got ${route:-<unavailable>}" >&2
  exit 1
}

wait_for_catalog_route() {
  local route=""
  local attempt
  for attempt in $(seq 1 60); do
    route="$(android_current_route "${cdp_port}" 2>/dev/null || true)"
    if [[ "${route}" == "http://tauri.localhost/" || "${route}" == http://tauri.localhost/\?* ]]; then
      return
    fi
    sleep 1
  done
  echo "expected the catalog route after an ordinary launch, got ${route:-<unavailable>}" >&2
  exit 1
}

wait_for_delivery() {
  local attempt
  for attempt in $(seq 1 30); do
    if "${adb[@]}" logcat -d -v brief | grep -q 'read_only_deep_link_delivered'; then
      return
    fi
    sleep 1
  done
  echo "valid Intent was not delivered to the application navigation boundary" >&2
  exit 1
}

start_link() {
  local action="$1"
  local link="$2"
  "${adb[@]}" shell am start \
    -W \
    -a "${action}" \
    -c android.intent.category.BROWSABLE \
    -d "${link}" \
    -p "${package}" >/dev/null
}

printf 'Installing Android debug APK\n'
"${adb[@]}" install -r "${apk}" >/dev/null
cold_link="dla-launcher://works/RJ01326398"
cold_pid=""
for cycle in $(seq 1 "${cold_cycles}"); do
  printf 'Checking a clean cold read-only Intent (%s/%s)\n' "${cycle}" "${cold_cycles}"
  "${adb[@]}" shell pm clear "${package}" >/dev/null
  "${adb[@]}" shell am force-stop "${package}"
  "${adb[@]}" logcat -c
  start_link android.intent.action.VIEW "${cold_link}"
  cold_pid="$(android_wait_for_pid "${package}")"
  android_connect_webview "${cold_pid}" "${cdp_port}"
  wait_for_route "http://tauri.localhost/works/RJ01326398"
  wait_for_delivery
done

if [[ -n "${artifact_dir}" ]]; then
  mkdir -p "${artifact_dir}"
  "${adb[@]}" exec-out screencap -p > "${artifact_dir}/android-cold-deep-link.png"
fi

printf 'Checking a warm read-only Intent\n'
"${adb[@]}" logcat -c
start_link android.intent.action.VIEW "dla-launcher://works/BJ009272"
warm_pid="$(android_wait_for_pid "${package}")"
wait_for_route "http://tauri.localhost/works/BJ009272"
wait_for_delivery
if [[ "${warm_pid}" != "${cold_pid}" ]]; then
  echo "warm Intent restarted the launcher process" >&2
  exit 1
fi

printf 'Checking malformed and write-capable Intent rejection\n'
"${adb[@]}" logcat -c
invalid_links=(
  'dla-launcher://works/RJ01326398?launch=true'
  'dla-launcher://works/RJ01326398#section'
  'dla-launcher://works/RJ01326398/'
  'dla-launcher://works/RJ01326398/extra'
  'dla-launcher://works/%52J01326398'
  'dla-launcher://works/%2e/RJ01326398'
  'dla-launcher://scanner/RJ01326398'
  'dla-launcher://import/RJ01326398'
  'dla-launcher://launch/RJ01326398'
  'dla-launcher://works/RJ1234'
)
for link in "${invalid_links[@]}"; do
  "${adb[@]}" shell am start \
    -W \
    -n "${component}" \
    -a android.intent.action.VIEW \
    -c android.intent.category.BROWSABLE \
    -d "${link}" >/dev/null
  wait_for_route "http://tauri.localhost/works/BJ009272"
done
if "${adb[@]}" logcat -d -v brief | grep -q 'read_only_deep_link_delivered'; then
  echo "an unsupported Intent crossed the native navigation boundary" >&2
  exit 1
fi

printf 'Checking ChromeOS-compatible background recovery\n'
"${adb[@]}" shell input keyevent KEYCODE_HOME
"${adb[@]}" logcat -c
start_link org.chromium.arc.intent.action.VIEW "dla-launcher://works/VJ12345"
background_pid="$(android_wait_for_pid "${package}")"
wait_for_route "http://tauri.localhost/works/VJ12345"
wait_for_delivery
if [[ "${background_pid}" != "${cold_pid}" ]]; then
  echo "background recovery restarted the launcher process" >&2
  exit 1
fi

printf 'Checking an ordinary launch does not replay stale navigation\n'
"${adb[@]}" shell am force-stop "${package}"
"${adb[@]}" logcat -c
"${adb[@]}" shell am start \
  -W \
  -n "${component}" \
  -a android.intent.action.MAIN \
  -c android.intent.category.LAUNCHER >/dev/null
normal_pid="$(android_wait_for_pid "${package}")"
android_connect_webview "${normal_pid}" "${cdp_port}"
wait_for_catalog_route
if "${adb[@]}" logcat -d -v brief | grep -q 'read_only_deep_link_delivered'; then
  echo "a normal launch replayed a stale deep link" >&2
  exit 1
fi

printf 'Android runtime verification passed\n'
printf '  device: %s\n' "$("${adb[@]}" shell getprop ro.product.model | tr -d '\r')"
printf '  release: %s (API %s)\n' \
  "$("${adb[@]}" shell getprop ro.build.version.release | tr -d '\r')" \
  "$("${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
printf '  ABI: %s\n' "$("${adb[@]}" shell getprop ro.product.cpu.abi | tr -d '\r')"
printf '  %s clean cold starts plus warm, background, malformed, and ordinary-launch cases: passed\n' "${cold_cycles}"
