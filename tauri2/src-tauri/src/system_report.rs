#[cfg(target_os = "linux")]
use std::fs;
use std::thread::available_parallelism;

use serde::Serialize;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemReport {
    pub os: String,
    pub os_version: String,
    pub kernel: String,
    pub arch: String,
    pub cpu: String,
    pub cpu_cores: u32,
    pub memory_bytes: u64,
    pub webview: String,
}

pub fn read_system_report() -> SystemReport {
    SystemReport {
        os: operating_system_name().to_string(),
        os_version: operating_system_version(),
        kernel: kernel_release(),
        arch: std::env::consts::ARCH.to_string(),
        cpu: processor_name(),
        cpu_cores: available_parallelism()
            .map(|count| count.get() as u32)
            .unwrap_or_default(),
        memory_bytes: total_memory_bytes(),
        webview: webview_description(),
    }
}

fn operating_system_name() -> &'static str {
    match std::env::consts::OS {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        "android" => "Android",
        "ios" => "iOS",
        other => other,
    }
}

#[cfg(target_os = "linux")]
fn operating_system_version() -> String {
    let Ok(contents) = fs::read_to_string("/etc/os-release") else {
        return String::new();
    };
    for line in contents.lines() {
        let Some(value) = line.strip_prefix("PRETTY_NAME=") else {
            continue;
        };
        return value.trim_matches('"').to_string();
    }
    String::new()
}

#[cfg(not(target_os = "linux"))]
fn operating_system_version() -> String {
    std::env::var("OS").unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn kernel_release() -> String {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

#[cfg(not(target_os = "linux"))]
fn kernel_release() -> String {
    String::new()
}

#[cfg(target_os = "linux")]
fn processor_name() -> String {
    let Ok(contents) = fs::read_to_string("/proc/cpuinfo") else {
        return String::new();
    };
    parse_cpu_model(&contents)
}

#[cfg(target_os = "windows")]
fn processor_name() -> String {
    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default()
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn processor_name() -> String {
    String::new()
}

#[cfg(target_os = "linux")]
fn total_memory_bytes() -> u64 {
    let Ok(contents) = fs::read_to_string("/proc/meminfo") else {
        return 0;
    };
    parse_total_memory_kib(&contents).saturating_mul(1024)
}

#[cfg(not(target_os = "linux"))]
fn total_memory_bytes() -> u64 {
    0
}

#[cfg(any(target_os = "linux", test))]
pub fn parse_cpu_model(contents: &str) -> String {
    for line in contents.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key == "model name" || key == "Model" || key == "Processor" {
            return value.trim().to_string();
        }
    }
    String::new()
}

#[cfg(any(target_os = "linux", test))]
pub fn parse_total_memory_kib(contents: &str) -> u64 {
    for line in contents.lines() {
        let Some(value) = line.strip_prefix("MemTotal:") else {
            continue;
        };
        return value
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .unwrap_or_default();
    }
    0
}

fn webview_description() -> String {
    let Ok(version) = tauri::webview_version() else {
        return String::new();
    };
    format!("{} {version}", webview_engine())
}

fn webview_engine() -> &'static str {
    match std::env::consts::OS {
        "linux" => "WebKitGTK",
        "android" => "Android WebView",
        "windows" => "WebView2",
        "macos" | "ios" => "WKWebView",
        _ => "WebView",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_cpu_model_from_proc_cpuinfo() {
        let contents = "processor\t: 0\nvendor_id\t: AuthenticAMD\nmodel name\t: AMD Ryzen 9 5900X 12-Core Processor\ncpu MHz\t\t: 3700.000\n";
        assert_eq!(
            parse_cpu_model(contents),
            "AMD Ryzen 9 5900X 12-Core Processor"
        );
    }

    #[test]
    fn falls_back_to_arm_style_cpu_keys() {
        assert_eq!(parse_cpu_model("Processor\t: ARMv8 rev 1\n"), "ARMv8 rev 1");
    }

    #[test]
    fn returns_an_empty_model_when_the_key_is_absent() {
        assert_eq!(parse_cpu_model("processor\t: 0\n"), "");
    }

    #[test]
    fn reads_total_memory_in_kibibytes() {
        let contents = "MemTotal:       32762304 kB\nMemFree:         1234 kB\n";
        assert_eq!(parse_total_memory_kib(contents), 32_762_304);
    }

    #[test]
    fn reports_zero_memory_when_meminfo_is_unexpected() {
        assert_eq!(parse_total_memory_kib("MemFree: 1234 kB\n"), 0);
        assert_eq!(parse_total_memory_kib("MemTotal: not-a-number\n"), 0);
    }
}
