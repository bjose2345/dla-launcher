#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "${script_dir}/.." && pwd)"
android_dir="${tauri_dir}/src-tauri/gen/android"
properties_file="${android_dir}/keystore.properties"
output_root="${android_dir}/app/build/outputs"
package="org.dlaproject.launcher"
activity="org.dlaproject.launcher.MainActivity"
component="${package}/${activity}"
short_component="${package}/.MainActivity"
alias="dla-release-verification"
password="dla-release-verification-only"
artifact_dir="${DLA_ANDROID_ARTIFACT_DIR:-${tauri_dir}/target/android-release-artifacts}"
rendered_frame_min_bytes="${DLA_ANDROID_RENDERED_FRAME_MIN_BYTES:-32768}"
render_settle_seconds="${DLA_ANDROID_RENDER_SETTLE_SECONDS:-15}"
source "${script_dir}/android-runtime-common.sh"

if [[ ! "${rendered_frame_min_bytes}" =~ ^[1-9][0-9]*$ ]]; then
  echo "DLA_ANDROID_RENDERED_FRAME_MIN_BYTES must be a positive integer" >&2
  exit 1
fi
if [[ ! "${render_settle_seconds}" =~ ^[0-9]+$ ]]; then
  echo "DLA_ANDROID_RENDER_SETTLE_SECONDS must be a non-negative integer" >&2
  exit 1
fi

if [[ -e "${properties_file}" ]]; then
  echo "Refusing to replace an existing Android signing configuration: ${properties_file}" >&2
  exit 1
fi

android_require_tools adb apkanalyzer bundletool jq keytool unzip
android_resolve_adb
adb=("${DLA_ANDROID_ADB[@]}")
backup_root="${output_root}/.dla-release-verification-backup"
saved_output_names=()

if [[ -e "${backup_root}" ]]; then
  echo "Refusing to replace an interrupted Android release backup: ${backup_root}" >&2
  exit 1
fi

temporary_dir="$(mktemp -d)"
keystore="${temporary_dir}/verification.jks"
apk_set="${temporary_dir}/device.apks"
device_apk_dir="${temporary_dir}/device-apks"
device_spec="${temporary_dir}/device-spec.json"
upgrade_config="${temporary_dir}/upgrade.json"
generated_state_root="${temporary_dir}/generated-state"
generated_state_paths=(
  "${android_dir}/app/tauri.properties"
  "${android_dir}/app/src/main/assets/tauri.conf.json"
)
declare -A saved_generated_state=()

cleanup() {
  "${adb[@]}" uninstall "${package}" >/dev/null 2>&1 || true
  rm -f "${properties_file}"
  rm -rf \
    "${output_root}/apk" \
    "${output_root}/bundle" \
    "${output_root}/mapping" \
    "${output_root}/dla-release"
  local output_name
  for output_name in "${saved_output_names[@]}"; do
    mv "${backup_root}/${output_name}" "${output_root}/${output_name}"
  done
  rmdir "${backup_root}" >/dev/null 2>&1 || true
  local generated_index generated_path
  for generated_index in "${!generated_state_paths[@]}"; do
    generated_path="${generated_state_paths[${generated_index}]}"
    rm -f "${generated_path}"
    if [[ -n "${saved_generated_state[${generated_index}]:-}" ]]; then
      cp -a "${generated_state_root}/${generated_index}" "${generated_path}"
    fi
  done
  rm -rf "${temporary_dir}"
  rm -f "${artifact_dir}/android-release-upgrade.png.tmp"
}
trap cleanup EXIT

mkdir -p "${generated_state_root}"
for generated_index in "${!generated_state_paths[@]}"; do
  generated_path="${generated_state_paths[${generated_index}]}"
  if [[ -e "${generated_path}" ]]; then
    cp -a "${generated_path}" "${generated_state_root}/${generated_index}"
    saved_generated_state["${generated_index}"]=1
  fi
done

mkdir -p "${backup_root}"
for output_name in apk bundle mapping dla-release; do
  if [[ -e "${output_root}/${output_name}" ]]; then
    mv "${output_root}/${output_name}" "${backup_root}/${output_name}"
    saved_output_names+=("${output_name}")
  fi
done

keytool -genkeypair -noprompt \
  -keystore "${keystore}" \
  -storetype PKCS12 \
  -storepass "${password}" \
  -keypass "${password}" \
  -alias "${alias}" \
  -keyalg RSA \
  -keysize 3072 \
  -validity 10000 \
  -dname "CN=DLA Release Verification, O=DLA Project, C=XX" >/dev/null 2>&1

umask 077
printf '%s\n' \
  "storeFile=${keystore}" \
  "storePassword=${password}" \
  "keyAlias=${alias}" \
  "keyPassword=${password}" \
  >"${properties_file}"

"${script_dir}/build-android-release.sh"

mapfile -t aabs < <(find "${output_root}/dla-release" -maxdepth 1 -type f -name '*.aab' -print | sort)
[[ "${#aabs[@]}" -eq 1 ]] || {
  echo "expected one Android App Bundle" >&2
  exit 1
}
aab="${aabs[0]}"
x86_64_apk=""
while IFS= read -r candidate; do
  archive_entries="$(unzip -Z1 "${candidate}")"
  if grep -Fxq 'lib/x86_64/libdla_launcher_tauri.so' <<<"${archive_entries}"; then
    x86_64_apk="${candidate}"
    break
  fi
done < <(find "${output_root}/dla-release" -maxdepth 1 -type f -name '*-x86_64.apk' -print | sort)
[[ -n "${x86_64_apk}" ]] || {
  echo "x86_64 release APK was not found" >&2
  exit 1
}

# bundletool's ddmlib connection does not honor ADB_SERVER_SOCKET. Describe the
# device through the established CLI connection, then install the selected set
# through that same explicit connection.
device_abis="$("${adb[@]}" shell getprop ro.product.cpu.abilist | tr -d '\r ')"
device_primary_abi="$("${adb[@]}" shell getprop ro.product.cpu.abi | tr -d '\r ')"
device_locale="$("${adb[@]}" shell getprop persist.sys.locale | tr -d '\r ')"
if [[ -z "${device_locale}" ]]; then
  device_locale="$("${adb[@]}" shell getprop ro.product.locale | tr -d '\r ')"
fi
device_locale="${device_locale//_/-}"
density_state="$("${adb[@]}" shell wm density | tr -d '\r')"
screen_density="$(
  sed -n -E 's/^Override density: ([0-9]+)$/\1/p; T; q' <<<"${density_state}"
)"
if [[ -z "${screen_density}" ]]; then
  screen_density="$(
    sed -n -E 's/^Physical density: ([0-9]+)$/\1/p; T; q' <<<"${density_state}"
  )"
fi
sdk_version="$("${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r ')"
[[ -n "${device_abis}" && -n "${device_primary_abi}" && -n "${device_locale}" ]] || {
  echo "Android device identity is incomplete" >&2
  exit 1
}
[[ "${screen_density}" =~ ^[1-9][0-9]*$ && "${sdk_version}" =~ ^[1-9][0-9]*$ ]] || {
  echo "Android device display or SDK information is invalid" >&2
  exit 1
}
supported_abis_json="$(
  jq -cn --arg value "${device_abis}" '$value | split(",") | map(select(length > 0))'
)"
jq -n \
  --argjson supportedAbis "${supported_abis_json}" \
  --arg locale "${device_locale}" \
  --argjson screenDensity "${screen_density}" \
  --argjson sdkVersion "${sdk_version}" \
  '{supportedAbis: $supportedAbis, supportedLocales: [$locale], screenDensity: $screenDensity, sdkVersion: $sdkVersion}' \
  >"${device_spec}"

bundletool build-apks \
  --bundle="${aab}" \
  --output="${apk_set}" \
  --ks="${keystore}" \
  --ks-key-alias="${alias}" \
  --ks-pass="pass:${password}" \
  --key-pass="pass:${password}" \
  --device-spec="${device_spec}" >/dev/null

bundletool extract-apks \
  --apks="${apk_set}" \
  --device-spec="${device_spec}" \
  --output-dir="${device_apk_dir}" >/dev/null
mapfile -t device_apks < <(find "${device_apk_dir}" -type f -name '*.apk' -print | sort)
[[ "${#device_apks[@]}" -gt 0 ]] || {
  echo "the Android App Bundle produced no APKs for the emulator" >&2
  exit 1
}
device_native_library="lib/${device_primary_abi}/libdla_launcher_tauri.so"
device_native_found=false
for candidate in "${device_apks[@]}"; do
  archive_entries="$(unzip -Z1 "${candidate}")"
  if grep -Fxq "${device_native_library}" <<<"${archive_entries}"; then
    device_native_found=true
    break
  fi
done
[[ "${device_native_found}" == true ]] || {
  echo "the AAB-derived APK set lacks the emulator's native library" >&2
  exit 1
}

printf 'Installing a clean device-specific APK set generated from the AAB\n'
"${adb[@]}" uninstall "${package}" >/dev/null 2>&1 || true
"${adb[@]}" logcat -c
if [[ "${#device_apks[@]}" -eq 1 ]]; then
  "${adb[@]}" install "${device_apks[0]}" >/dev/null
else
  "${adb[@]}" install-multiple "${device_apks[@]}" >/dev/null
fi
android_wait_for_package "${package}"

release_is_resumed() {
  local activity_state resumed_state
  activity_state="$("${adb[@]}" shell dumpsys activity activities)"
  resumed_state="$(
    grep -E 'topResumedActivity=|ResumedActivity:' <<<"${activity_state}" || true
  )"
  [[ "${resumed_state}" == *"${component}"* \
    || "${resumed_state}" == *"${short_component}"* ]]
}

check_runtime_health() {
  local pid runtime_log
  pid="$("${adb[@]}" shell pidof "${package}" 2>/dev/null | tr -d '\r')"
  if [[ -z "${pid}" ]]; then
    echo "Android release process is not running" >&2
    return 1
  fi
  runtime_log="$("${adb[@]}" logcat -d --pid="${pid}" -v brief)"
  if grep -Eq 'FATAL EXCEPTION|Fatal signal|panicked at' <<<"${runtime_log}"; then
    echo "Android release emitted a fatal runtime record" >&2
    return 1
  fi
}

start_and_check() {
  "${adb[@]}" shell am start -W -n "${component}" >/dev/null
  android_wait_for_pid "${package}" >/dev/null
  local attempt
  for attempt in $(seq 1 60); do
    if release_is_resumed; then
      sleep 2
      check_runtime_health
      return
    fi
    sleep 1
  done
  echo "Android release activity did not become resumed" >&2
  return 1
}

capture_rendered_frame() {
  local destination="${artifact_dir}/android-release-upgrade.png"
  local temporary="${destination}.tmp"
  local attempt screenshot_bytes
  mkdir -p "${artifact_dir}"
  for attempt in $(seq 1 60); do
    "${adb[@]}" exec-out screencap -p >"${temporary}"
    screenshot_bytes="$(wc -c <"${temporary}" | tr -d ' ')"
    if (( screenshot_bytes >= rendered_frame_min_bytes )); then
      sleep "${render_settle_seconds}"
      "${adb[@]}" exec-out screencap -p >"${temporary}"
      screenshot_bytes="$(wc -c <"${temporary}" | tr -d ' ')"
    fi
    if (( screenshot_bytes >= rendered_frame_min_bytes )); then
      mv "${temporary}" "${destination}"
      return
    fi
    sleep 1
  done
  rm -f "${temporary}"
  echo "Android release did not render a reviewable frame" >&2
  return 1
}

start_and_check
installed_version_code="$(
  package_state="$("${adb[@]}" shell dumpsys package "${package}")"
  sed -n -E 's/.*versionCode=([0-9]+).*/\1/p; T; q' <<<"${package_state}" \
    | tr -d '\r'
)"
[[ "${installed_version_code}" =~ ^[0-9]+$ ]] || {
  echo "installed release version code is unavailable" >&2
  exit 1
}

upgrade_version_code="$((installed_version_code + 1))"
printf '{"bundle":{"android":{"versionCode":%s}}}\n' "${upgrade_version_code}" >"${upgrade_config}"
printf 'Building and installing a same-key upgrade (%s → %s)\n' \
  "${installed_version_code}" "${upgrade_version_code}"
cd "${tauri_dir}"
CI=true cargo tauri android build \
  --apk \
  --split-per-abi \
  --target x86_64 \
  --config "${upgrade_config}" \
  --ci >/dev/null

check_runtime_health

upgrade_apk=""
while IFS= read -r candidate; do
  archive_entries="$(unzip -Z1 "${candidate}")"
  if [[ "$(apkanalyzer manifest version-code "${candidate}")" == "${upgrade_version_code}" ]] \
    && grep -Fxq 'lib/x86_64/libdla_launcher_tauri.so' <<<"${archive_entries}"; then
    upgrade_apk="${candidate}"
    break
  fi
done < <(find "${output_root}/apk" -type f -path '*/release/*.apk' -print | sort)
[[ -n "${upgrade_apk}" ]] || {
  echo "higher-version x86_64 release APK was not generated" >&2
  exit 1
}

"${adb[@]}" install -r "${upgrade_apk}" >/dev/null
upgraded_version_code="$(
  package_state="$("${adb[@]}" shell dumpsys package "${package}")"
  sed -n -E 's/.*versionCode=([0-9]+).*/\1/p; T; q' <<<"${package_state}" \
    | tr -d '\r'
)"
[[ "${upgraded_version_code}" == "${upgrade_version_code}" ]] || {
  echo "Android did not retain the higher release version" >&2
  exit 1
}
"${adb[@]}" logcat -c
start_and_check
capture_rendered_frame
release_is_resumed || {
  echo "Android release was not resumed after rendering" >&2
  exit 1
}
check_runtime_health

printf 'Android release verification passed\n'
printf '  device: %s\n' "$("${adb[@]}" shell getprop ro.product.model | tr -d '\r')"
printf '  release: %s (API %s)\n' \
  "$("${adb[@]}" shell getprop ro.build.version.release | tr -d '\r')" \
  "$("${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r')"
printf '  ABI: %s\n' "$("${adb[@]}" shell getprop ro.product.cpu.abi | tr -d '\r')"
printf '  upgrade: %s → %s\n' "${installed_version_code}" "${upgraded_version_code}"
printf '  screenshot: %s\n' "${artifact_dir}/android-release-upgrade.png"
