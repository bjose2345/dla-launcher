use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let package_path = manifest_directory.join("../../package.json");
    println!("cargo:rerun-if-changed={}", package_path.display());
    let package: serde_json::Value =
        serde_json::from_slice(&fs::read(&package_path).expect("read workspace package.json"))
            .expect("parse workspace package.json");
    let issue_url = package
        .pointer("/bugs/url")
        .and_then(serde_json::Value::as_str)
        .expect("package.json bugs.url must be configured");
    let version = package
        .get("version")
        .and_then(serde_json::Value::as_str)
        .expect("package.json version must be configured");
    println!("cargo:rustc-env=DLA_SUPPORT_ISSUES_URL={issue_url}");
    println!("cargo:rustc-env=DLA_LAUNCHER_VERSION={version}");
    println!("cargo:rerun-if-env-changed=DLA_BUILD_ID");
    let repository = manifest_directory.join("../..");
    let build_id = env::var("DLA_BUILD_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| git_build_id(&repository, version));
    println!("cargo:rustc-env=DLA_BUILD_ID={build_id}");
    tauri_build::build()
}

fn git_build_id(repository: &std::path::Path, fallback: &str) -> String {
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(repository)
        .output()
    else {
        return fallback.to_owned();
    };
    if !output.status.success() {
        return fallback.to_owned();
    }
    let revision = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if revision.is_empty() {
        fallback.to_owned()
    } else {
        revision
    }
}
