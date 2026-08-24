#!/usr/bin/env bash
set -euo pipefail

package="org.dlaproject.launcher.debug"
activity="org.dlaproject.launcher.MainActivity"
component="${package}/${activity}"
fixture_package="org.dlaproject.fixture.apk"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "${script_dir}/.." && pwd)"
source "${script_dir}/android-runtime-common.sh"
launcher_apk="${DLA_ANDROID_APK:-${tauri_dir}/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk}"
fixture_apk="${DLA_ANDROID_FIXTURE_APK:-${tauri_dir}/tests/android-apk-fixture/app/build/outputs/apk/debug/app-debug.apk}"
device_fixture="/sdcard/Download/dla-apk-fixture.apk"
cdp_port="${DLA_ANDROID_CDP_PORT:-9223}"
artifact_dir="${DLA_ANDROID_ARTIFACT_DIR:-}"

android_require_tools adb curl jq node
android_resolve_adb
adb=("${DLA_ANDROID_ADB[@]}")

if [[ ! -f "${launcher_apk}" ]]; then
  echo "Android launcher APK was not found: ${launcher_apk}" >&2
  exit 1
fi
if [[ ! -f "${fixture_apk}" ]]; then
  echo "Android fixture APK was not found: ${fixture_apk}" >&2
  exit 1
fi

cleanup() {
  "${adb[@]}" forward --remove "tcp:${cdp_port}" >/dev/null 2>&1 || true
  "${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null 2>&1 || true
  "${adb[@]}" shell rm -f "${device_fixture}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

cdp_evaluate() {
  node "${script_dir}/android-cdp.mjs" "${cdp_port}" "$1"
}

wait_for_cdp_true() {
  local expression="$1"
  local attempt
  for attempt in $(seq 1 90); do
    if [[ "$(cdp_evaluate "${expression}" 2>/dev/null || true)" == "true" ]]; then
      return
    fi
    sleep 1
  done
  echo "WebView condition did not become true: ${expression}" >&2
  exit 1
}

click_web_button() {
  local label="$1"
  local expression
  expression="(() => { const button = [...document.querySelectorAll('button')].find((item) => item.textContent?.trim() === '${label}'); if (!button || button.disabled) return false; button.click(); return true; })()"
  if [[ "$(cdp_evaluate "${expression}")" != "true" ]]; then
    echo "WebView button is unavailable: ${label}" >&2
    exit 1
  fi
}

dump_ui() {
  "${adb[@]}" shell uiautomator dump /sdcard/dla-window.xml >/dev/null 2>&1
  "${adb[@]}" exec-out cat /sdcard/dla-window.xml
}

find_ui_node() {
  dump_ui | node "${script_dir}/android-ui-node.mjs" "$@" 2>/dev/null
}

tap_matching_ui_node() {
  local point=""
  local attempt
  for attempt in $(seq 1 60); do
    point="$(find_ui_node "$@" || true)"
    if [[ -n "${point}" ]]; then
      read -r x y <<<"${point}"
      "${adb[@]}" shell input tap "${x}" "${y}"
      return
    fi
    sleep 1
  done
  echo "Matching Android UI node was not found: $*" >&2
  exit 1
}

open_downloads_folder() {
  if [[ -z "$(find_ui_node text "Open from" || true)" ]]; then
    tap_ui_node content-desc "Show roots"
  fi
  tap_matching_ui_node text "Downloads" resource-id "android:id/title"
  wait_for_ui_node text "dla-apk-fixture.apk"
}

tap_ui_node() {
  local attribute="$1"
  local value="$2"
  local point=""
  local attempt
  for attempt in $(seq 1 60); do
    point="$(find_ui_node "${attribute}" "${value}" || true)"
    if [[ -n "${point}" ]]; then
      read -r x y <<<"${point}"
      "${adb[@]}" shell input tap "${x}" "${y}"
      return
    fi
    sleep 1
  done
  echo "Android UI node was not found: ${attribute}=${value}" >&2
  exit 1
}

tap_ui_node_if_present() {
  local point=""
  point="$(find_ui_node "$1" "$2" || true)"
  if [[ -z "${point}" ]]; then
    return 1
  fi
  read -r x y <<<"${point}"
  "${adb[@]}" shell input tap "${x}" "${y}"
}

wait_for_ui_node() {
  local attribute="$1"
  local value="$2"
  local attempt
  for attempt in $(seq 1 60); do
    if [[ -n "$(find_ui_node "${attribute}" "${value}" || true)" ]]; then
      return
    fi
    sleep 1
  done
  echo "Android UI node was not found: ${attribute}=${value}" >&2
  exit 1
}

tap_ui_node_until_gone() {
  local attribute="$1"
  local value="$2"
  local point=""
  local attempt
  for attempt in $(seq 1 60); do
    point="$(find_ui_node "${attribute}" "${value}" || true)"
    if [[ -z "${point}" ]]; then
      return
    fi
    read -r x y <<<"${point}"
    "${adb[@]}" shell input tap "${x}" "${y}"
    sleep 1
  done
  echo "Android UI node did not close: ${attribute}=${value}" >&2
  exit 1
}

wait_for_package() {
  local attempt
  for attempt in $(seq 1 60); do
    if "${adb[@]}" shell pm path "${fixture_package}" 2>/dev/null | grep -q '^package:'; then
      return
    fi
    sleep 1
  done
  echo "Android did not install the fixture package" >&2
  exit 1
}

capture() {
  if [[ -n "${artifact_dir}" ]]; then
    mkdir -p "${artifact_dir}"
    sleep 2
    "${adb[@]}" exec-out screencap -p > "${artifact_dir}/$1"
  fi
}

printf 'Preparing isolated Android APK fixture\n'
"${adb[@]}" install -r "${launcher_apk}" >/dev/null
"${adb[@]}" shell pm clear "${package}" >/dev/null
"${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null 2>&1 || true
"${adb[@]}" shell appops set "${package}" REQUEST_INSTALL_PACKAGES default >/dev/null
"${adb[@]}" push "${fixture_apk}" "${device_fixture}" >/dev/null
"${adb[@]}" shell am start -W -n "${component}" -a android.intent.action.MAIN \
  -c android.intent.category.LAUNCHER >/dev/null
pid="$(android_wait_for_pid "${package}")"
android_connect_webview "${pid}" "${cdp_port}"

wait_for_cdp_true "Boolean(document.querySelector('a[href=\"/android-packages\"]'))"
cdp_evaluate "document.querySelector('a[href=\"/android-packages\"]')?.click(); true" >/dev/null
wait_for_cdp_true "location.pathname === '/android-packages' && document.body.innerText.includes('Install an Android app')"
capture "android-apk-empty.png"

printf 'Selecting and inspecting one APK through Android\n'
click_web_button "Choose APK"
open_downloads_folder
tap_ui_node_until_gone text "dla-apk-fixture.apk"
wait_for_cdp_true "document.body.innerText.includes('DLA APK Fixture')"
cdp_evaluate "document.querySelector('.android-package-details')?.setAttribute('open', ''); true" >/dev/null
wait_for_cdp_true "document.body.innerText.includes('org.dlaproject.fixture.apk')"
wait_for_cdp_true "document.body.innerText.includes('Allow installs from DLA Launcher')"
capture "android-apk-inspection.png"
cdp_evaluate "(() => { const apkPage = document.querySelector('.android-package-page'); if (apkPage) apkPage.scrollTop = apkPage.scrollHeight; return true; })()" >/dev/null
capture "android-apk-inspection-details.png"
cdp_evaluate "(() => { const apkPage = document.querySelector('.android-package-page'); if (apkPage) apkPage.scrollTop = 0; return true; })()" >/dev/null

printf 'Granting the app-specific Android source approval\n'
click_web_button "Open Android settings"
if ! tap_ui_node_if_present resource-id "android:id/switch_widget"; then
  tap_ui_node text "Allow from this source"
fi
for attempt in $(seq 1 30); do
  if "${adb[@]}" shell appops get "${package}" REQUEST_INSTALL_PACKAGES 2>/dev/null | grep -q 'allow'; then
    break
  fi
  if [[ "${attempt}" -eq 30 ]]; then
    echo "Android did not grant app-specific source approval" >&2
    exit 1
  fi
  sleep 1
done
"${adb[@]}" shell input keyevent KEYCODE_BACK
wait_for_cdp_true "!document.body.innerText.includes('Allow installs from DLA Launcher') && ![...document.querySelectorAll('button')].find((item) => item.textContent?.trim() === 'Continue to Android')?.disabled"

printf 'Handing installation to Android system confirmation\n'
click_web_button "Continue to Android"
wait_for_ui_node text "Install"
capture "android-apk-system-confirmation.png"
tap_ui_node text "Install"
wait_for_package
tap_ui_node_if_present text "Done" || true
"${adb[@]}" shell am start -W -n "${component}" >/dev/null
android_connect_webview "$(android_wait_for_pid "${package}")" "${cdp_port}"
wait_for_cdp_true "document.body.innerText.includes('App installed')"
cdp_evaluate "(() => { const apkPage = document.querySelector('.android-package-page'); if (apkPage) apkPage.scrollTop = apkPage.scrollHeight; return true; })()" >/dev/null
capture "android-apk-installed.png"

printf 'Android APK installation verification passed\n'
printf '  device: %s\n' "$("${adb[@]}" shell getprop ro.product.model | tr -d '\r')"
printf '  release: %s (API %s)\n' \
  "$("${adb[@]}" shell getprop ro.build.version.release | tr -d '\r')" \
  "$("${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
printf '  fixture: DLA APK Fixture 1.2.3 (%s)\n' "${fixture_package}"
printf '  selection, inspection, source approval, system confirmation, callback, and cleanup: passed\n'
