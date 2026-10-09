//! The client, transfer and locator against a stand-in for the receiver's
//! downloads listener that speaks the same protocol: Basic sign-in, folder
//! listings, ranged files with Content-Range and Last-Modified, and the
//! listener's refusals.

use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use dla_application::delivery::{
    DeliveryControl, DeliveryError, DeliveryLocator, DeliveryTransfer, ReleaseLookup,
};
use dla_domain::delivery::{DeliveryArchive, DeliveryPhase, DeliveryProblem};
use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::{DeliveryClient, Endpoint, FileTransfer, ManifestLocator, PART_SUFFIX};

const LAST_MODIFIED: &str = "Wed, 07 Oct 2026 20:01:31 GMT";

#[derive(Default)]
struct State {
    files: BTreeMap<String, Vec<u8>>,
    /// The next answers refuse with this status and reason.
    refuse: Option<(u16, &'static str)>,
    /// The next file answer closes after this many body bytes.
    cut_after: Option<usize>,
    /// Answers to If-Range as if the file had been replaced.
    replaced: bool,
    requests: Vec<String>,
}

struct FakeService {
    state: Arc<Mutex<State>>,
    base: String,
}

impl FakeService {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = Arc::clone(&shared);
                thread::spawn(move || answer(stream, &state));
            }
        });
        Self { state, base }
    }

    fn put(&self, path: &str, data: &[u8]) {
        self.state
            .lock()
            .unwrap()
            .files
            .insert(path.to_owned(), data.to_vec());
    }

    fn requests(&self, prefix: &str) -> usize {
        self.state
            .lock()
            .unwrap()
            .requests
            .iter()
            .filter(|request| request.starts_with(prefix))
            .count()
    }

    fn client(&self) -> Arc<DeliveryClient> {
        let client = DeliveryClient::new(Endpoint::self_test(&self.base).unwrap()).unwrap();
        client
            .sign_in("tier3", Zeroizing::new("right".into()))
            .unwrap();
        Arc::new(client)
    }
}

fn respond(
    stream: &mut TcpStream,
    status: u16,
    headers: &[(&str, String)],
    body: &[u8],
    cut: Option<usize>,
) {
    let mut head = format!(
        "HTTP/1.1 {status} X\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let sent = cut.unwrap_or(body.len()).min(body.len());
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body[..sent]);
}

fn refusal(stream: &mut TcpStream, status: u16, reason: &str) {
    let body = format!(r#"{{"error":"refused","reason":"{reason}"}}"#);
    respond(
        stream,
        status,
        &[("Content-Type", "application/json".into())],
        body.as_bytes(),
        None,
    );
}

fn answer(mut stream: TcpStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let target = line
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let mut headers = BTreeMap::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).unwrap();
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let (endpoint, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let path = query
        .strip_prefix("path=")
        .map(|value| percent_decode_str(value).decode_utf8_lossy().into_owned())
        .unwrap_or_default();
    let mut state = state.lock().unwrap();
    state.requests.push(format!("{endpoint} {path}"));
    let signed_in = headers
        .get("authorization")
        .and_then(|value| value.strip_prefix("Basic "))
        .and_then(|value| STANDARD.decode(value).ok())
        .is_some_and(|value| value == b"tier3:right");
    if !signed_in {
        return refusal(&mut stream, 401, "credentials");
    }
    if let Some((status, reason)) = state.refuse {
        return refusal(&mut stream, status, reason);
    }
    match endpoint {
        "/v1" => respond(&mut stream, 200, &[], br#"{"protocol":1}"#, None),
        "/v1/list" => {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            let mut entries: BTreeMap<String, (bool, usize)> = BTreeMap::new();
            for (name, data) in &state.files {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    match rest.split_once('/') {
                        Some((folder, _)) => entries.insert(folder.to_owned(), (true, 0)),
                        None => entries.insert(rest.to_owned(), (false, data.len())),
                    };
                }
            }
            if entries.is_empty() {
                return refusal(&mut stream, 404, "not_found");
            }
            let listed: Vec<String> = entries
                .into_iter()
                .map(|(name, (directory, size))| {
                    format!(r#"{{"name":"{name}","size":{size},"directory":{directory},"modified":"2026-10-07T20:01:31Z"}}"#)
                })
                .collect();
            let body = format!(r#"{{"path":"{path}","entries":[{}]}}"#, listed.join(","));
            respond(&mut stream, 200, &[], body.as_bytes(), None);
        }
        "/v1/file" => {
            let Some(data) = state.files.get(&path).cloned() else {
                return refusal(&mut stream, 404, "not_found");
            };
            let cut = state.cut_after.take();
            let replaced = state.replaced;
            drop(state);
            let range = headers
                .get("range")
                .and_then(|value| value.strip_prefix("bytes="));
            let if_range_fails = headers.contains_key("if-range") && replaced;
            match range {
                Some(range) if !if_range_fails => {
                    let (start, end) = range.split_once('-').unwrap();
                    let start: usize = start.parse().unwrap();
                    let end: usize = end.parse::<usize>().unwrap().min(data.len() - 1);
                    respond(
                        &mut stream,
                        206,
                        &[
                            (
                                "Content-Range",
                                format!("bytes {start}-{end}/{}", data.len()),
                            ),
                            ("Last-Modified", LAST_MODIFIED.into()),
                        ],
                        &data[start..=end],
                        cut,
                    );
                }
                _ => respond(
                    &mut stream,
                    200,
                    &[("Last-Modified", LAST_MODIFIED.into())],
                    &data,
                    cut,
                ),
            }
        }
        _ => refusal(&mut stream, 404, "unknown"),
    }
}

struct Go;

impl DeliveryControl for Go {
    fn check(&self) -> Result<(), DeliveryError> {
        Ok(())
    }
}

fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn archive(name: &str, data: &[u8], remote: Option<&str>) -> DeliveryArchive {
    DeliveryArchive {
        file_name: name.to_owned(),
        size: data.len() as u64,
        sha256: sha256(data),
        remote_path: remote.map(str::to_owned),
        complete: false,
    }
}

fn problem(result: Result<impl std::fmt::Debug, DeliveryError>) -> DeliveryProblem {
    match result {
        Err(DeliveryError::Problem { problem, .. }) => problem,
        other => panic!("expected a problem, got {other:?}"),
    }
}

fn staging(root: &tempfile::TempDir) -> String {
    root.path()
        .join(".dla-downloads")
        .join("RJ01234567")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn signing_in_keeps_only_accepted_credentials() {
    let service = FakeService::start();
    let client = DeliveryClient::new(Endpoint::self_test(&service.base).unwrap()).unwrap();
    assert_eq!(
        problem(client.list("/dlsite")),
        DeliveryProblem::SignInRequired
    );
    assert_eq!(
        problem(client.sign_in("tier3", Zeroizing::new("wrong".into()))),
        DeliveryProblem::Credentials
    );
    assert_eq!(client.signed_in_as(), None);
    client
        .sign_in(" tier3 ", Zeroizing::new("right".into()))
        .unwrap();
    assert_eq!(client.signed_in_as().as_deref(), Some("tier3"));
    client.sign_out();
    assert_eq!(client.signed_in_as(), None);
}

#[test]
fn downloads_in_ranges_resumes_after_a_cut_and_verifies() {
    let service = FakeService::start();
    let data: Vec<u8> = (0..1000u32).map(|n| (n % 251) as u8).collect();
    service.put("/dlsite/2026-10/RJ01234567 作品.zip", &data);
    let transfer = FileTransfer::with_range_bytes(service.client(), 128);
    let root = tempfile::tempdir().unwrap();
    let folder = staging(&root);
    let wanted = archive(
        "RJ01234567 作品.zip",
        &data,
        Some("/dlsite/2026-10/RJ01234567 作品.zip"),
    );

    // The connection drops 50 bytes into the third range.
    service.state.lock().unwrap().cut_after = None;
    let mut phases = Vec::new();
    {
        let mut ranges = 0;
        let state = Arc::clone(&service.state);
        let result = transfer.fetch(&wanted, &folder, &Go, &mut |phase, held| {
            phases.push((phase, held));
            if phase == DeliveryPhase::Receiving && held == 256 && ranges == 0 {
                ranges += 1;
                state.lock().unwrap().cut_after = Some(50);
            }
        });
        assert_eq!(problem(result), DeliveryProblem::Unreachable);
    }
    let part = format!("{folder}/{}{PART_SUFFIX}", wanted.sha256);
    assert_eq!(fs::metadata(&part).unwrap().len(), 306, "kept what arrived");

    let mut phases = Vec::new();
    transfer
        .fetch(&wanted, &folder, &Go, &mut |phase, held| {
            phases.push((phase, held))
        })
        .unwrap();
    assert_eq!(
        fs::read(format!("{folder}/RJ01234567 作品.zip")).unwrap(),
        data
    );
    assert!(fs::metadata(&part).is_err(), "the part was renamed");
    assert!(
        phases.contains(&(DeliveryPhase::Receiving, 306)),
        "continued at 306: {phases:?}"
    );
    assert_eq!(phases.last(), Some(&(DeliveryPhase::Verifying, 1000)));

    // A finished file found later is checked rather than fetched again.
    let before = service.requests("/v1/file");
    transfer
        .fetch(&wanted, &folder, &Go, &mut |_, _| {})
        .unwrap();
    assert_eq!(service.requests("/v1/file"), before);

    transfer.discard(&folder).unwrap();
    assert!(!root.path().join(".dla-downloads").exists());
    assert!(root.path().exists());
}

#[test]
fn a_wrong_file_is_deleted_and_refusals_keep_their_meaning() {
    let service = FakeService::start();
    service.put("/dlsite/2026-10/RJ1.zip", b"not what the catalog says");
    let transfer = FileTransfer::with_range_bytes(service.client(), 8);
    let root = tempfile::tempdir().unwrap();
    let folder = staging(&root);
    let mut expected = archive(
        "RJ1.zip",
        b"not what the catalog says",
        Some("/dlsite/2026-10/RJ1.zip"),
    );
    expected.sha256 = "0".repeat(64);
    assert_eq!(
        problem(transfer.fetch(&expected, &folder, &Go, &mut |_, _| {})),
        DeliveryProblem::Corrupt
    );
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0, "nothing kept");

    let good = archive(
        "RJ1.zip",
        b"not what the catalog says",
        Some("/dlsite/2026-10/RJ1.zip"),
    );
    for (status, reason, expected) in [
        (403, "allowance", DeliveryProblem::Allowance),
        (429, "busy", DeliveryProblem::Busy),
        (429, "too_many", DeliveryProblem::TooManyAttempts),
        (404, "not_found", DeliveryProblem::NotInArchive),
        (503, "unavailable", DeliveryProblem::Unavailable),
    ] {
        service.state.lock().unwrap().refuse = Some((status, reason));
        assert_eq!(
            problem(transfer.fetch(&good, &folder, &Go, &mut |_, _| {})),
            expected,
            "{reason}"
        );
    }
    service.state.lock().unwrap().refuse = None;

    // The file is replaced on the server between two ranges.
    let mut seen = 0;
    let state = Arc::clone(&service.state);
    let result = transfer.fetch(&good, &folder, &Go, &mut |phase, _| {
        if phase == DeliveryPhase::Receiving {
            seen += 1;
            if seen == 2 {
                state.lock().unwrap().replaced = true;
            }
        }
    });
    assert_eq!(problem(result), DeliveryProblem::Unavailable);

    // Only a work folder inside the staging folder can be discarded.
    assert_eq!(
        problem(transfer.discard(&root.path().to_string_lossy())),
        DeliveryProblem::InstallFolder
    );
    assert!(root.path().exists());
}

#[test]
fn archives_are_found_by_sha256_in_the_manifests() {
    let service = FakeService::start();
    let original = b"part one of the original release".to_vec();
    let second = b"part two".to_vec();
    let update = b"the updated release".to_vec();
    let manifest = |files: &[(&str, &[u8])]| {
        let entries: Vec<String> = files
            .iter()
            .map(|(name, data)| {
                format!(
                    r#""{name}":{{"size":{},"hashes":{{"sha256":"{}"}}}}"#,
                    data.len(),
                    sha256(data).to_uppercase()
                )
            })
            .collect();
        format!(
            r#"{{"manifest_info":{{}},"files":{{{}}}}}"#,
            entries.join(",")
        )
    };
    service.put(
        "/dlsite/2025-11/_manifest.json",
        manifest(&[("RJ1.part1.exe", &original), ("RJ1.part2.rar", &second)]).as_bytes(),
    );
    service.put("/dlsite/2025-11/RJ1.part1.exe", &original);
    service.put(
        "/dlsite/2026-02/_manifest.json",
        manifest(&[("RJ1_v2.zip", &update)]).as_bytes(),
    );
    service.put("/dlsite/2026-03/_manifest.json", manifest(&[]).as_bytes());
    let cache = tempfile::tempdir().unwrap();
    let locator = ManifestLocator::new(service.client(), Some(cache.path().join("manifests.json")));

    let parts = [
        archive("RJ1.part1.exe", &original, None),
        archive("RJ1.part2.rar", &second, None),
    ];
    let lookup = |archives: &[DeliveryArchive], release: &'static str, update: &'static str| {
        let archives = archives.to_vec();
        move |locator: &ManifestLocator| {
            locator.locate(
                &ReleaseLookup {
                    work_code: "RJ1",
                    release_date: release,
                    update_date: update,
                    archives: &archives,
                },
                &Go,
            )
        }
    };
    let found = lookup(&parts, "2025-11-07", "")(&locator).unwrap();
    assert_eq!(
        found,
        [
            "/dlsite/2025-11/RJ1.part1.exe",
            "/dlsite/2025-11/RJ1.part2.rar"
        ]
    );
    assert_eq!(
        service.requests("/v1/list /dlsite"),
        1,
        "only the hinted month was listed"
    );

    // Not in the hinted months: every month is searched, newest first.
    let updated = [archive("RJ1_v2.zip", &update, None)];
    let found = lookup(&updated, "2025-11-07", "")(&locator).unwrap();
    assert_eq!(found, ["/dlsite/2026-02/RJ1_v2.zip"]);

    // Unchanged manifests come from the cache.
    let reads = service.requests("/v1/file /dlsite/2026-02/_manifest.json");
    lookup(&updated, "2026-02-21", "")(&locator).unwrap();
    assert_eq!(
        service.requests("/v1/file /dlsite/2026-02/_manifest.json"),
        reads
    );

    let missing = [archive("RJ1.zip", b"never archived", None)];
    assert_eq!(
        problem(lookup(&missing, "2025-11-07", "")(&locator)),
        DeliveryProblem::NotInArchive
    );
}
