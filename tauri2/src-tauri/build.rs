use std::{
    collections::hash_map::RandomState,
    env, fs,
    hash::{BuildHasher, Hasher},
    path::PathBuf,
    process::Command,
};

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
    embed_access_token(&manifest_directory);
    tauri_build::build()
}

/// Embeds the launcher's Cloudflare Access service token for the downloads
/// route from a private file that never enters source control. The value is
/// masked only so it is not plain text in the binary; anyone holding the app
/// can recover it, so the SFTPGo account remains the real access control.
fn embed_access_token(manifest_directory: &std::path::Path) {
    println!("cargo:rerun-if-env-changed=DLA_LAUNCHER_ACCESS_TOKEN_FILE");
    let path = env::var_os("DLA_LAUNCHER_ACCESS_TOKEN_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_directory.join("../../state/tunnel/launcher-token.json"));
    println!("cargo:rerun-if-changed={}", path.display());
    let Ok(data) = fs::read(&path) else {
        println!(
            "cargo:warning=No launcher access token at {}; Install from DLA is unavailable in this build.",
            path.display()
        );
        return;
    };
    let value: serde_json::Value =
        serde_json::from_slice(&data).expect("the access token file is JSON");
    let field = |name: &str| {
        value[name]
            .as_str()
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| panic!("the access token file has no {name}"))
            .to_owned()
    };
    let plain = serde_json::to_vec(&serde_json::json!({
        "hostname": field("hostname"),
        "client_id": field("client_id"),
        "client_secret": field("client_secret"),
    }))
    .expect("serializable token");
    let key = random_bytes(plain.len());
    let masked: Vec<u8> = plain
        .iter()
        .zip(&key)
        .map(|(byte, mask)| byte ^ mask)
        .collect();
    println!(
        "cargo:rustc-env=DLA_LAUNCHER_ACCESS_BLOB={}{}",
        hex(&key),
        hex(&masked)
    );
}

fn random_bytes(length: usize) -> Vec<u8> {
    let state = RandomState::new();
    let mut bytes = Vec::with_capacity(length + 8);
    let mut counter = 0u64;
    while bytes.len() < length {
        let mut hasher = state.build_hasher();
        hasher.write_u64(counter);
        bytes.extend_from_slice(&hasher.finish().to_le_bytes());
        counter += 1;
    }
    bytes.truncate(length);
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
