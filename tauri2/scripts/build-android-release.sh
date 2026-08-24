#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
tauri_dir="$(cd "${script_dir}/.." && pwd)"
android_dir="${tauri_dir}/src-tauri/gen/android"
properties_file="${android_dir}/keystore.properties"
output_root="${android_dir}/app/build/outputs"
package_id="org.dlaproject.launcher"
min_sdk=24
target_sdk=36
max_apk_bytes="${DLA_ANDROID_RELEASE_MAX_APK_BYTES:-100663296}"
max_aab_bytes="${DLA_ANDROID_RELEASE_MAX_AAB_BYTES:-134217728}"
read -r -a targets <<<"${DLA_ANDROID_RELEASE_TARGETS:-aarch64 armv7 i686 x86_64}"

fail() {
  echo "$*" >&2
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"
}

for command in apkanalyzer bundletool cargo jarsigner keytool sha256sum stat unzip; do
  require_command "${command}"
done

[[ -f "${properties_file}" ]] || fail \
  "Android release signing is not configured; copy keystore.properties.example to keystore.properties"
if grep -q 'replace-with-a-secret' "${properties_file}"; then
  fail "keystore.properties still contains example credentials"
fi
[[ "${max_apk_bytes}" =~ ^[1-9][0-9]*$ ]] || fail \
  "DLA_ANDROID_RELEASE_MAX_APK_BYTES must be a positive integer"
[[ "${max_aab_bytes}" =~ ^[1-9][0-9]*$ ]] || fail \
  "DLA_ANDROID_RELEASE_MAX_AAB_BYTES must be a positive integer"
[[ "${#targets[@]}" -gt 0 ]] || fail "at least one Android release target is required"

declare -A target_abis=(
  [aarch64]=arm64-v8a
  [armv7]=armeabi-v7a
  [i686]=x86
  [x86_64]=x86_64
)
expected_abis=()
declare -A seen_targets=()
for target in "${targets[@]}"; do
  [[ -n "${target_abis[${target}]:-}" ]] || fail "unsupported Android release target: ${target}"
  [[ -z "${seen_targets[${target}]:-}" ]] || fail "duplicate Android release target: ${target}"
  seen_targets["${target}"]=1
  expected_abis+=("${target_abis[${target}]}")
done
mapfile -t expected_abis < <(printf '%s\n' "${expected_abis[@]}" | sort)

apksigner="$(find "${ANDROID_HOME:?ANDROID_HOME is required}/build-tools" \
  -mindepth 2 -maxdepth 2 -type f -name apksigner -print | sort -V | tail -1)"
zipalign="$(find "${ANDROID_HOME}/build-tools" \
  -mindepth 2 -maxdepth 2 -type f -name zipalign -print | sort -V | tail -1)"
[[ -x "${apksigner}" ]] || fail "Android apksigner was not found"
[[ -x "${zipalign}" ]] || fail "Android zipalign was not found"

rm -rf "${output_root}/apk" "${output_root}/bundle"
rm -rf "${output_root}/mapping" "${output_root}/dla-release"

cd "${tauri_dir}"
CI=true cargo tauri android build --aab --target "${targets[@]}" --ci
CI=true cargo tauri android build --apk --split-per-abi --target "${targets[@]}" --ci

mapfile -t aabs < <(find "${output_root}/bundle" -type f -name '*.aab' -print | sort)
[[ "${#aabs[@]}" -eq 1 ]] || fail "expected one universal Android App Bundle, found ${#aabs[@]}"
aab="${aabs[0]}"

aab_verification="$(LC_ALL=C jarsigner -verify "${aab}" 2>&1)"
grep -q 'jar verified' <<<"${aab_verification}" || fail "Android App Bundle is not signed"
bundletool validate --bundle="${aab}" >/dev/null
aab_manifest="$(bundletool dump manifest --bundle="${aab}" --module=base)"
aab_package_id="$(sed -n -E 's/.* package="([^"]+)".*/\1/p; T; q' <<<"${aab_manifest}")"
aab_version_code="$(sed -n -E 's/.* android:versionCode="([0-9]+)".*/\1/p; T; q' <<<"${aab_manifest}")"
aab_version_name="$(sed -n -E 's/.* android:versionName="([^"]+)".*/\1/p; T; q' <<<"${aab_manifest}")"
aab_min_sdk="$(sed -n -E 's/.* android:minSdkVersion="([0-9]+)".*/\1/p; T; q' <<<"${aab_manifest}")"
aab_target_sdk="$(sed -n -E 's/.* android:targetSdkVersion="([0-9]+)".*/\1/p; T; q' <<<"${aab_manifest}")"
[[ "${aab_package_id}" == "${package_id}" ]] || fail \
  "Android App Bundle has an unexpected package identifier"
[[ "${aab_version_code}" =~ ^[1-9][0-9]*$ && -n "${aab_version_name}" ]] || fail \
  "Android App Bundle has an invalid version"
[[ "${aab_min_sdk}" == "${min_sdk}" && "${aab_target_sdk}" == "${target_sdk}" ]] || fail \
  "Android App Bundle has an unexpected SDK range"
grep -q 'android:debuggable="true"' <<<"${aab_manifest}" \
  && fail "Android App Bundle is debuggable"
aab_certificate_digest="$(
  LC_ALL=C keytool -printcert -jarfile "${aab}" \
    | sed -n -E 's/^[[:space:]]*SHA256: ([0-9A-Fa-f:]+)$/\1/p; T; q' \
    | tr -d ':' \
    | tr '[:upper:]' '[:lower:]'
)"
[[ -n "${aab_certificate_digest}" ]] || fail \
  "Android App Bundle signer is unavailable"
aab_size="$(stat -c '%s' "${aab}")"
(( aab_size <= max_aab_bytes )) || fail \
  "Android App Bundle exceeds the ${max_aab_bytes}-byte release budget: ${aab_size}"
mapfile -t aab_abis < <(
  unzip -Z1 "${aab}" \
    | sed -n -E 's#^(base/)?lib/([^/]+)/libdla_launcher_tauri\.so$#\2#p' \
    | sort -u
)
[[ "${aab_abis[*]}" == "${expected_abis[*]}" ]] || fail \
  "Android App Bundle ABIs do not match the requested targets: ${aab_abis[*]:-none}"

mapfile -t apks < <(find "${output_root}/apk" -type f -path '*/release/*.apk' -print | sort)
[[ "${#apks[@]}" -eq "${#expected_abis[@]}" ]] || fail \
  "expected ${#expected_abis[@]} ABI APKs, found ${#apks[@]}"

release_version_code="${aab_version_code}"
release_version_name="${aab_version_name}"
release_certificate_digest=""
observed_abis=()
apk_abi_pairs=()
for apk in "${apks[@]}"; do
  mapfile -t apk_abis < <(
    unzip -Z1 "${apk}" \
      | sed -n -E 's#^lib/([^/]+)/libdla_launcher_tauri\.so$#\1#p' \
      | sort -u
  )
  [[ "${#apk_abis[@]}" -eq 1 ]] || fail "release APK is not ABI-specific: ${apk}"
  observed_abis+=("${apk_abis[0]}")
  apk_abi_pairs+=("${apk_abis[0]}|${apk}")

  [[ "$(apkanalyzer manifest application-id "${apk}")" == "${package_id}" ]] || fail \
    "release APK has an unexpected package identifier: ${apk}"
  [[ "$(apkanalyzer manifest debuggable "${apk}")" == "false" ]] || fail \
    "release APK is debuggable: ${apk}"
  [[ "$(apkanalyzer manifest min-sdk "${apk}")" == "${min_sdk}" ]] || fail \
    "release APK has an unexpected minimum SDK: ${apk}"
  [[ "$(apkanalyzer manifest target-sdk "${apk}")" == "${target_sdk}" ]] || fail \
    "release APK has an unexpected target SDK: ${apk}"

  version_code="$(apkanalyzer manifest version-code "${apk}")"
  version_name="$(apkanalyzer manifest version-name "${apk}")"
  [[ "${version_code}" =~ ^[1-9][0-9]*$ && -n "${version_name}" ]] || fail \
    "release APK has an invalid version: ${apk}"
  if [[ "${version_code}" != "${release_version_code}" || "${version_name}" != "${release_version_name}" ]]; then
    fail "release APK versions are inconsistent"
  fi

  "${zipalign}" -c -P 16 4 "${apk}" >/dev/null
  signer_output="$(LC_ALL=C "${apksigner}" verify --verbose --print-certs "${apk}")"
  signer_digest="$(
    sed -n 's/^Signer #1 certificate SHA-256 digest: //p' <<<"${signer_output}" \
      | tr -d ':' \
      | tr '[:upper:]' '[:lower:]'
  )"
  [[ -n "${signer_digest}" ]] || fail "release APK signer is unavailable: ${apk}"
  if [[ -z "${release_certificate_digest}" ]]; then
    release_certificate_digest="${signer_digest}"
  elif [[ "${signer_digest}" != "${release_certificate_digest}" ]]; then
    fail "release APK signing identities are inconsistent"
  fi
  apk_size="$(stat -c '%s' "${apk}")"
  (( apk_size <= max_apk_bytes )) || fail \
    "release APK exceeds the ${max_apk_bytes}-byte release budget: ${apk_size} (${apk})"
done

[[ "${release_certificate_digest}" == "${aab_certificate_digest}" ]] || fail \
  "Android App Bundle and APK signing identities are inconsistent"

mapfile -t observed_abis < <(printf '%s\n' "${observed_abis[@]}" | sort)
[[ "${observed_abis[*]}" == "${expected_abis[*]}" ]] || fail \
  "release APK ABIs do not match the requested targets: ${observed_abis[*]}"

mapfile -t mappings < <(find "${output_root}/mapping" -type f -name mapping.txt -print | sort)
[[ "${#mappings[@]}" -eq "$(( ${#expected_abis[@]} + 1 ))" ]] || fail \
  "expected one R8 mapping for every release variant, found ${#mappings[@]}"
for mapping in "${mappings[@]}"; do
  [[ -s "${mapping}" ]] || fail "R8 mapping is empty: ${mapping}"
done

release_label="$(tr -c 'A-Za-z0-9._-' '_' <<<"${release_version_name}" | sed 's/_$//')"
release_dir="${output_root}/dla-release"
mkdir -p "${release_dir}/mapping"
install -m 0644 "${aab}" "${release_dir}/dla-launcher-${release_label}.aab"
for pair in "${apk_abi_pairs[@]}"; do
  abi="${pair%%|*}"
  apk="${pair#*|}"
  install -m 0644 "${apk}" "${release_dir}/dla-launcher-${release_label}-${abi}.apk"
done
for mapping in "${mappings[@]}"; do
  variant="$(basename "$(dirname "${mapping}")")"
  install -m 0644 "${mapping}" "${release_dir}/mapping/${variant}-mapping.txt"
done

manifest="${release_dir}/SHA256SUMS"
(
  cd "${release_dir}"
  sha256sum ./*.aab ./*.apk ./mapping/*.txt >"${manifest}"
)

printf 'Android release artifacts verified\n'
printf '  package: %s\n' "${package_id}"
printf '  version: %s (%s)\n' "${release_version_name}" "${release_version_code}"
printf '  AAB: %s bytes — %s\n' \
  "${aab_size}" "${release_dir}/dla-launcher-${release_label}.aab"
for pair in "${apk_abi_pairs[@]}"; do
  abi="${pair%%|*}"
  apk="${pair#*|}"
  printf '  APK %s: %s bytes — %s\n' \
    "${abi}" \
    "$(stat -c '%s' "${apk}")" \
    "${release_dir}/dla-launcher-${release_label}-${abi}.apk"
done
printf '  checksums: %s\n' "${manifest}"
