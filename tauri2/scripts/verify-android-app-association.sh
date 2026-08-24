#!/usr/bin/env bash
set -euo pipefail

launcher_package="org.dlaproject.launcher.debug"
launcher_activity="org.dlaproject.launcher.MainActivity"
launcher_component="${launcher_package}/${launcher_activity}"
fixture_package="org.dlaproject.fixture.launch"
work_code="RJ01326398"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "${script_dir}/.." && pwd)"
source "${script_dir}/android-runtime-common.sh"
launcher_apk="${DLA_ANDROID_APK:-${tauri_dir}/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk}"
fixture_apk="${DLA_ANDROID_LAUNCH_FIXTURE_APK:-${tauri_dir}/tests/android-launch-fixture/app/build/outputs/apk/debug/app-debug.apk}"
device_fixture="/sdcard/Download/dla-launch-fixture.apk"
cdp_port="${DLA_ANDROID_CDP_PORT:-9224}"
artifact_dir="${DLA_ANDROID_ARTIFACT_DIR:-}"
temporary_directory=""

if ! command -v apksigner >/dev/null 2>&1; then
  android_sdk_root="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-/opt/android-sdk}}"
  for build_tools_directory in "${android_sdk_root}"/build-tools/*; do
    if [[ -x "${build_tools_directory}/apksigner" ]]; then
      PATH="${build_tools_directory}:${PATH}"
    fi
  done
fi

android_require_tools adb apksigner curl jq keytool node
android_resolve_adb
adb=("${DLA_ANDROID_ADB[@]}")

if [[ ! -f "${launcher_apk}" ]]; then
  echo "Android launcher APK was not found: ${launcher_apk}" >&2
  exit 1
fi
if [[ ! -f "${fixture_apk}" ]]; then
  echo "Android launch fixture APK was not found: ${fixture_apk}" >&2
  exit 1
fi

cleanup() {
  "${adb[@]}" forward --remove "tcp:${cdp_port}" >/dev/null 2>&1 || true
  "${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null 2>&1 || true
  "${adb[@]}" shell rm -f "${device_fixture}" >/dev/null 2>&1 || true
  if [[ -n "${temporary_directory}" && -d "${temporary_directory}" ]]; then
    rm -rf "${temporary_directory}"
  fi
}
trap cleanup EXIT

cdp_evaluate() {
  android_cdp_evaluate "${cdp_port}" "$1"
}

wait_for_cdp_true() {
  android_wait_for_cdp_true "${cdp_port}" "$1"
}

click_web_button() {
  android_click_web_button "${cdp_port}" "$1"
}

connect_launcher_webview() {
  android_connect_webview "$(android_wait_for_pid "${launcher_package}")" "${cdp_port}"
}

open_downloads_folder() {
  if [[ -z "$(android_find_ui_node text "Open from" || true)" ]]; then
    android_tap_ui_node content-desc "Show roots"
  fi
  android_tap_matching_ui_node text "Downloads" resource-id "android:id/title"
  android_wait_for_ui_node text "dla-launch-fixture.apk"
}

capture() {
  if [[ -n "${artifact_dir}" ]]; then
    mkdir -p "${artifact_dir}"
    sleep 2
    "${adb[@]}" exec-out screencap -p > "${artifact_dir}/$1"
  fi
}

open_launcher_library() {
  "${adb[@]}" shell am start -W -n "${launcher_component}" >/dev/null
  connect_launcher_webview
  wait_for_cdp_true "Boolean(document.querySelector('a[href=\"/library\"]'))"
  cdp_evaluate "document.querySelector('a[href=\"/library\"]')?.click(); true" >/dev/null
  wait_for_cdp_true "location.pathname === '/library'"
}

dismiss_restart_notice() {
  if [[ "$(cdp_evaluate "Boolean(document.querySelector('.support-recovery-dismiss'))")" != "true" ]]; then
    return
  fi
  cdp_evaluate "document.querySelector('.support-recovery-dismiss')?.click(); true" >/dev/null
  wait_for_cdp_true "!document.querySelector('.support-recovery-dismiss')"
}

make_mismatched_fixture() {
  temporary_directory="$(mktemp -d)"
  keytool -genkeypair -noprompt \
    -keystore "${temporary_directory}/mismatch.p12" \
    -storetype PKCS12 \
    -storepass fixture-password \
    -keypass fixture-password \
    -alias mismatch \
    -keyalg RSA \
    -keysize 2048 \
    -validity 3650 \
    -dname "CN=DLA Mismatch Fixture" >/dev/null 2>&1
  apksigner sign \
    --ks "${temporary_directory}/mismatch.p12" \
    --ks-type PKCS12 \
    --ks-pass pass:fixture-password \
    --key-pass pass:fixture-password \
    --out "${temporary_directory}/mismatch.apk" \
    "${fixture_apk}"
  apksigner verify "${temporary_directory}/mismatch.apk"
}

printf 'Preparing the reviewed Android application association fixture\n'
"${adb[@]}" install -r "${launcher_apk}" >/dev/null
"${adb[@]}" shell pm clear "${launcher_package}" >/dev/null
"${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null 2>&1 || true
"${adb[@]}" shell appops set "${launcher_package}" REQUEST_INSTALL_PACKAGES allow >/dev/null
"${adb[@]}" push "${fixture_apk}" "${device_fixture}" >/dev/null
"${adb[@]}" shell am start -W \
  -n "${launcher_component}" \
  -a android.intent.action.VIEW \
  -d "dla-launcher://works/${work_code}" >/dev/null
connect_launcher_webview
wait_for_cdp_true "location.pathname === '/works/${work_code}'"
wait_for_cdp_true "[...document.querySelectorAll('button')].some((item) => item.textContent?.trim() === 'Install Android app')"

printf 'Installing a launchable APK from one explicit catalog work\n'
click_web_button "Install Android app"
wait_for_cdp_true "location.pathname === '/android-packages' && location.search.includes('${work_code}')"
click_web_button "Choose APK"
open_downloads_folder
android_tap_ui_node_until_gone text "dla-launch-fixture.apk"
wait_for_cdp_true "document.body.innerText.includes('DLA Launch Fixture')"
wait_for_cdp_true "[...document.querySelectorAll('button')].some((item) => item.textContent?.trim() === 'Continue to Android' && !item.disabled)"
click_web_button "Continue to Android"
android_wait_for_ui_node text "Install"
capture "android-app-system-install.png"
android_tap_ui_node text "Install"
android_wait_for_package "${fixture_package}"
android_tap_ui_node_if_present text "Done" || true
"${adb[@]}" shell am start -W -n "${launcher_component}" >/dev/null
connect_launcher_webview
wait_for_cdp_true "document.body.innerText.includes('App installed')"

printf 'Binding the installed identity and reviewed signer to Library state\n'
click_web_button "Add to Library"
wait_for_cdp_true "document.body.innerText.includes('Added to your Library')"
click_web_button "Open Library"
wait_for_cdp_true "location.pathname === '/library' && document.body.innerText.includes('DLA Launch Fixture')"
wait_for_cdp_true "document.body.innerText.toLowerCase().includes('installed and ready')"
capture "android-app-library-ready.png"

printf 'Checking persistence across a launcher restart\n'
"${adb[@]}" shell am force-stop "${launcher_package}"
open_launcher_library
wait_for_cdp_true "document.body.innerText.includes('DLA Launch Fixture') && document.body.innerText.toLowerCase().includes('installed and ready')"
dismiss_restart_notice

printf 'Launching only from the explicit Library action\n'
click_web_button "Open app"
android_wait_for_ui_node text "DLA Android launch fixture is running"
if ! "${adb[@]}" shell dumpsys activity activities \
  | grep -E "(topResumedActivity=|ResumedActivity:).*${fixture_package}" >/dev/null; then
  echo "The reviewed Android application did not become the resumed activity" >&2
  exit 1
fi
capture "android-app-launched.png"

printf 'Detecting removal and recovery with the same signer\n'
open_launcher_library
"${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null
wait_for_cdp_true "document.body.innerText.toLowerCase().includes('no longer installed')"
capture "android-app-missing.png"
"${adb[@]}" install "${fixture_apk}" >/dev/null
wait_for_cdp_true "document.body.innerText.toLowerCase().includes('installed and ready')"

printf 'Refusing a reinstall signed by another certificate\n'
make_mismatched_fixture
"${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null
"${adb[@]}" install "${temporary_directory}/mismatch.apk" >/dev/null
wait_for_cdp_true "document.body.innerText.toLowerCase().includes('certificate does not match')"
wait_for_cdp_true "![...document.querySelectorAll('button')].some((item) => item.textContent?.trim() === 'Open app')"
capture "android-app-signer-mismatch.png"

printf 'Recovering the reviewed signer and removing only the Library link\n'
"${adb[@]}" shell pm uninstall "${fixture_package}" >/dev/null
"${adb[@]}" install "${fixture_apk}" >/dev/null
wait_for_cdp_true "document.body.innerText.toLowerCase().includes('installed and ready')"
click_web_button "Remove link"
click_web_button "Remove link"
wait_for_cdp_true "!document.body.innerText.includes('DLA Launch Fixture')"
if ! "${adb[@]}" shell pm path "${fixture_package}" | grep -q '^package:'; then
  echo "Removing an association unexpectedly uninstalled the Android fixture" >&2
  exit 1
fi

printf 'Android application association verification passed\n'
printf '  device: %s\n' "$("${adb[@]}" shell getprop ro.product.model | tr -d '\r')"
printf '  release: %s (API %s)\n' \
  "$("${adb[@]}" shell getprop ro.build.version.release | tr -d '\r')" \
  "$("${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
printf '  fixture: DLA Launch Fixture 2.0.0 (%s)\n' "${fixture_package}"
printf '  persistence, launch, removal, recovery, signer refusal, and link-only cleanup: passed\n'
