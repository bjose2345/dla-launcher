//! The client, locator and transfer against the real receiver's downloads
//! listener and its mock SFTPGo, built in dla-collab with
//! `scripts/build-selftest-binaries.sh` and copied to
//! `tauri2/target/selftest-bin`. Skipped when those binaries are absent.

use std::{
    fs,
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use dla_application::delivery::{
    DeliveryControl, DeliveryError, DeliveryLocator, DeliveryTransfer, ReleaseLookup,
};
use dla_domain::delivery::{DeliveryArchive, DeliveryPhase, DeliveryProblem};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{DeliveryClient, Endpoint, FileTransfer, ManifestLocator};

struct Servers {
    children: Vec<Child>,
    base: String,
    _directories: Vec<tempfile::TempDir>,
}

impl Drop for Servers {
    fn drop(&mut self) {
        for child in &mut self.children {
            child.kill().ok();
            child.wait().ok();
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(archive: &std::path::Path) -> Option<Servers> {
    let binaries = std::env::var_os("DLA_GO_SELFTEST_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/selftest-bin")
        });
    if !binaries.join("receiver").is_file() || !binaries.join("mockdav").is_file() {
        eprintln!(
            "skipped: no Go self-test binaries in {}",
            binaries.display()
        );
        return None;
    }
    let (mock, receiver, downloads) = (free_port(), free_port(), free_port());
    let data = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let children = vec![
        Command::new(binaries.join("mockdav"))
            .env("DLA_MOCK_LISTEN", format!("127.0.0.1:{mock}"))
            .env("DLA_MOCK_DATA", data.path())
            .env("DLA_MOCK_DLSITE", archive)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        Command::new(binaries.join("receiver"))
            .env("DLA_RECEIVER_MODE", "mock")
            .env("DLA_RECEIVER_DATA", staging.path())
            .env(
                "DLA_MOCK_URL",
                format!("http://127.0.0.1:{mock}/dav/upload"),
            )
            .env("DLA_RECEIVER_LISTEN", format!("127.0.0.1:{receiver}"))
            .env("DLA_DOWNLOADS_LISTEN", format!("127.0.0.1:{downloads}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    ];
    let servers = Servers {
        children,
        base: format!("http://127.0.0.1:{downloads}/v1"),
        _directories: vec![data, staging],
    };
    for port in [mock, downloads] {
        let deadline = Instant::now() + Duration::from_secs(10);
        while TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "port {port} did not open");
            thread::sleep(Duration::from_millis(50));
        }
    }
    Some(servers)
}

struct Go;

impl DeliveryControl for Go {
    fn check(&self) -> Result<(), DeliveryError> {
        Ok(())
    }
}

#[test]
fn the_launcher_speaks_the_receivers_downloads_protocol() {
    let archive = tempfile::tempdir().unwrap();
    fs::create_dir_all(archive.path().join("2026-10")).unwrap();
    let data: Vec<u8> = (0..3_000_000u32)
        .map(|n| (n.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let sha256 = hex::encode(Sha256::digest(&data));
    fs::write(archive.path().join("2026-10/RJ01234567 作品.zip"), &data).unwrap();
    fs::write(
        archive.path().join("2026-10/_manifest.json"),
        format!(
            r#"{{"manifest_info":{{}},"files":{{"RJ01234567 作品.zip":{{"work_id":"RJ01234567","size":{},"hashes":{{"sha256":"{}"}}}}}}}}"#,
            data.len(),
            sha256.to_uppercase()
        ),
    )
    .unwrap();
    let Some(servers) = start(archive.path()) else {
        return;
    };

    let client = DeliveryClient::new(Endpoint::self_test(&servers.base).unwrap()).unwrap();
    match client.sign_in("demo-a", Zeroizing::new("wrong".into())) {
        Err(DeliveryError::Problem {
            problem: DeliveryProblem::Credentials,
            ..
        }) => {}
        other => panic!("wrong password: {other:?}"),
    }
    client
        .sign_in("demo-a", Zeroizing::new("local-demo".into()))
        .unwrap();
    let client = Arc::new(client);

    let wanted = DeliveryArchive {
        file_name: "RJ01234567 作品.zip".to_owned(),
        size: data.len() as u64,
        sha256,
        remote_path: None,
        complete: false,
    };
    let locator = ManifestLocator::new(Arc::clone(&client), None);
    let paths = locator
        .locate(
            &ReleaseLookup {
                work_code: "RJ01234567",
                release_date: "2026-10-01",
                update_date: "",
                archives: std::slice::from_ref(&wanted),
            },
            &Go,
        )
        .unwrap();
    assert_eq!(paths, ["/dlsite/2026-10/RJ01234567 作品.zip"]);

    let located = DeliveryArchive {
        remote_path: Some(paths[0].clone()),
        ..wanted
    };
    // Several ranges, as a large archive would need, each with If-Range.
    let transfer = FileTransfer::with_range_bytes(Arc::clone(&client), 700_000);
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join(".dla-downloads/RJ01234567");
    let mut phases = Vec::new();
    transfer
        .fetch(
            &located,
            &folder.to_string_lossy(),
            &Go,
            &mut |phase, held| phases.push((phase, held)),
        )
        .unwrap();
    assert_eq!(fs::read(folder.join("RJ01234567 作品.zip")).unwrap(), data);
    assert_eq!(phases.last(), Some(&(DeliveryPhase::Verifying, 3_000_000)));
    assert!(phases.contains(&(DeliveryPhase::Receiving, 2_100_000)));

    match client.read_small("/dlsite/../etc/passwd", 100) {
        Err(DeliveryError::Problem {
            problem: DeliveryProblem::NotInArchive,
            ..
        }) => {}
        other => panic!("path outside the archive: {other:?}"),
    }
    drop(servers);
}
