#!/usr/bin/env bash

android_runtime_script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

android_require_tools() {
  local tool
  for tool in "$@"; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      echo "required command is unavailable: ${tool}" >&2
      return 1
    fi
  done
}

android_resolve_adb() {
  if [[ -n "${DLA_ANDROID_SERIAL:-}" ]]; then
    DLA_ANDROID_ADB=(adb -s "${DLA_ANDROID_SERIAL}")
    return
  fi

  local devices=()
  mapfile -t devices < <(adb devices | awk '$2 == "device" { print $1 }')
  if [[ "${#devices[@]}" -ne 1 ]]; then
    echo "expected one ready Android device; set DLA_ANDROID_SERIAL when more are connected" >&2
    return 1
  fi
  DLA_ANDROID_ADB=(adb -s "${devices[0]}")
}

android_wait_for_pid() {
  local package="$1"
  local pid=""
  local attempt
  for attempt in $(seq 1 60); do
    pid="$("${DLA_ANDROID_ADB[@]}" shell pidof "${package}" 2>/dev/null | tr -d '\r')"
    if [[ -n "${pid}" ]]; then
      printf '%s\n' "${pid}"
      return
    fi
    sleep 1
  done
  echo "launcher process did not start" >&2
  return 1
}

android_connect_webview() {
  local pid="$1"
  local port="$2"
  "${DLA_ANDROID_ADB[@]}" forward --remove "tcp:${port}" >/dev/null 2>&1 || true
  "${DLA_ANDROID_ADB[@]}" forward "tcp:${port}" "localabstract:webview_devtools_remote_${pid}" >/dev/null
}

android_current_route() {
  local port="$1"
  curl -fsS "http://127.0.0.1:${port}/json" | jq -r '.[0].url // empty'
}

android_cdp_evaluate() {
  local port="$1"
  local expression="$2"
  node "${android_runtime_script_dir}/android-cdp.mjs" "${port}" "${expression}"
}

android_wait_for_cdp_true() {
  local port="$1"
  local expression="$2"
  local attempt
  for attempt in $(seq 1 90); do
    if [[ "$(android_cdp_evaluate "${port}" "${expression}" 2>/dev/null || true)" == "true" ]]; then
      return
    fi
    sleep 1
  done
  echo "WebView condition did not become true: ${expression}" >&2
  return 1
}

android_click_web_button() {
  local port="$1"
  local label="$2"
  local expression
  expression="(() => { const button = [...document.querySelectorAll('button')].find((item) => item.textContent?.trim() === '${label}'); if (!button || button.disabled) return false; button.click(); return true; })()"
  if [[ "$(android_cdp_evaluate "${port}" "${expression}")" != "true" ]]; then
    echo "WebView button is unavailable: ${label}" >&2
    return 1
  fi
}

android_dump_ui() {
  "${DLA_ANDROID_ADB[@]}" shell uiautomator dump /sdcard/dla-window.xml >/dev/null 2>&1
  "${DLA_ANDROID_ADB[@]}" exec-out cat /sdcard/dla-window.xml
}

android_find_ui_node() {
  android_dump_ui | node "${android_runtime_script_dir}/android-ui-node.mjs" "$@" 2>/dev/null
}

android_tap_matching_ui_node() {
  local point=""
  local attempt
  for attempt in $(seq 1 60); do
    point="$(android_find_ui_node "$@" || true)"
    if [[ -n "${point}" ]]; then
      read -r x y <<<"${point}"
      "${DLA_ANDROID_ADB[@]}" shell input tap "${x}" "${y}"
      return
    fi
    sleep 1
  done
  echo "Matching Android UI node was not found: $*" >&2
  return 1
}

android_tap_ui_node() {
  local attribute="$1"
  local value="$2"
  android_tap_matching_ui_node "${attribute}" "${value}"
}

android_tap_ui_node_if_present() {
  local point=""
  point="$(android_find_ui_node "$1" "$2" || true)"
  if [[ -z "${point}" ]]; then
    return 1
  fi
  read -r x y <<<"${point}"
  "${DLA_ANDROID_ADB[@]}" shell input tap "${x}" "${y}"
}

android_wait_for_ui_node() {
  local attribute="$1"
  local value="$2"
  local attempt
  for attempt in $(seq 1 60); do
    if [[ -n "$(android_find_ui_node "${attribute}" "${value}" || true)" ]]; then
      return
    fi
    sleep 1
  done
  echo "Android UI node was not found: ${attribute}=${value}" >&2
  return 1
}

android_tap_ui_node_until_gone() {
  local attribute="$1"
  local value="$2"
  local point=""
  local attempt
  for attempt in $(seq 1 60); do
    point="$(android_find_ui_node "${attribute}" "${value}" || true)"
    if [[ -z "${point}" ]]; then
      return
    fi
    read -r x y <<<"${point}"
    "${DLA_ANDROID_ADB[@]}" shell input tap "${x}" "${y}"
    sleep 1
  done
  echo "Android UI node did not close: ${attribute}=${value}" >&2
  return 1
}

android_wait_for_package() {
  local package="$1"
  local attempt
  for attempt in $(seq 1 60); do
    if "${DLA_ANDROID_ADB[@]}" shell pm path "${package}" 2>/dev/null | grep -q '^package:'; then
      return
    fi
    sleep 1
  done
  echo "Android did not install package: ${package}" >&2
  return 1
}
