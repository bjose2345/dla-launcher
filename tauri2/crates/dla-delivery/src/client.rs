use std::{
    fmt,
    io::Read,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use dla_application::delivery::DeliveryError;
use dla_domain::delivery::DeliveryProblem;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use ureq::{
    Agent, Body,
    config::Config,
    http::{Response, Uri},
    tls::{Certificate, RootCerts, TlsConfig, TlsProvider},
    unversioned::{
        resolver::{ResolvedSocketAddrs, Resolver},
        transport::{DefaultConnector, NextTimeout},
    },
};
use zeroize::Zeroizing;

/// The receiver's read-only downloads listener on its private address. The
/// connection itself goes to the local cloudflared forward.
pub const PRIVATE_DOWNLOADS: &str = "https://172.30.250.2:5009/v1";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
// A slow but healthy link must still fit one range (about 55 KB/s).
const RANGE_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_JSON_BYTES: u64 = 16 << 20;

/// Where to open the TCP connection instead of the server's own address,
/// such as the local `cloudflared access tcp` listener. TLS is still verified
/// for the server's address. `None` means the forward is not ready yet.
pub type Dial = Arc<dyn Fn() -> Option<SocketAddr> + Send + Sync>;

#[derive(Clone)]
enum Target {
    Private { ca_pem: Arc<[u8]> },
    SelfTest { base: String },
}

/// The downloads service: the private listener, trusted only through the
/// DLA upload CA, or a plain-HTTP loopback stand-in for the self-test.
#[derive(Clone)]
pub struct Endpoint {
    target: Target,
    dial: Option<Dial>,
}

impl Endpoint {
    pub fn private(ca_pem: Arc<[u8]>) -> Self {
        Self {
            target: Target::Private { ca_pem },
            dial: None,
        }
    }

    pub fn self_test(base: &str) -> Result<Self, DeliveryError> {
        let invalid = || {
            DeliveryError::problem(
                DeliveryProblem::Unavailable,
                "the self-test downloads address must be plain HTTP on loopback at /v1",
            )
        };
        let uri: Uri = base.parse().map_err(|_| invalid())?;
        let loopback = uri
            .host()
            .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
            .and_then(|host| host.parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if uri.scheme_str() != Some("http")
            || !loopback
            || uri.path() != "/v1"
            || uri.query().is_some()
        {
            return Err(invalid());
        }
        Ok(Self {
            target: Target::SelfTest {
                base: base.to_owned(),
            },
            dial: None,
        })
    }

    pub fn through(mut self, dial: Dial) -> Self {
        self.dial = Some(dial);
        self
    }

    pub fn is_self_test(&self) -> bool {
        matches!(self.target, Target::SelfTest { .. })
    }

    fn base(&self) -> &str {
        match &self.target {
            Target::Private { .. } => PRIVATE_DOWNLOADS,
            Target::SelfTest { base } => base,
        }
    }

    fn agent(&self, body_timeout: Duration) -> Result<Agent, DeliveryError> {
        let tls = match &self.target {
            Target::Private { ca_pem } => {
                let ca = Certificate::from_pem(ca_pem).map_err(|_| {
                    DeliveryError::problem(
                        DeliveryProblem::Unavailable,
                        "the DLA upload CA is invalid",
                    )
                })?;
                TlsConfig::builder()
                    .provider(TlsProvider::Rustls)
                    .root_certs(RootCerts::new_with_certs(&[ca]))
                    .build()
            }
            Target::SelfTest { .. } => TlsConfig::default(),
        };
        let config = Agent::config_builder()
            .https_only(!self.is_self_test())
            .tls_config(tls)
            .proxy(None)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_recv_response(Some(RESPONSE_TIMEOUT))
            .timeout_recv_body(Some(body_timeout))
            .user_agent(concat!("dla-launcher/", env!("CARGO_PKG_VERSION")))
            .build();
        Ok(match &self.dial {
            Some(dial) => Agent::with_parts(
                config,
                DefaultConnector::default(),
                DialResolver(Arc::clone(dial)),
            ),
            None => config.into(),
        })
    }
}

struct DialResolver(Dial);

impl fmt::Debug for DialResolver {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DialResolver")
    }
}

impl Resolver for DialResolver {
    fn resolve(
        &self,
        _uri: &Uri,
        _config: &Config,
        _timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let address = (self.0)().ok_or(ureq::Error::HostNotFound)?;
        let mut addresses = self.empty();
        addresses.push(address);
        Ok(addresses)
    }
}

pub struct Credentials {
    pub username: String,
    pub password: Zeroizing<String>,
}

impl Credentials {
    fn authorization(&self) -> Zeroizing<String> {
        let token = Zeroizing::new(format!("{}:{}", self.username, self.password.as_str()));
        Zeroizing::new(format!("Basic {}", STANDARD.encode(token.as_bytes())))
    }
}

/// One entry of a folder listing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub directory: bool,
    pub modified: String,
}

#[derive(Deserialize)]
struct Listing {
    entries: Vec<Entry>,
}

#[derive(Deserialize)]
struct Refusal {
    #[serde(default)]
    reason: String,
    #[serde(default)]
    error: String,
}

/// One answered range of a file.
pub struct RangeReply {
    /// The whole file's size, from Content-Range.
    pub total: u64,
    pub last_modified: Option<String>,
    pub body: Body,
}

/// A signed-in session with the downloads service. The password is kept only
/// in memory and sent with each request.
pub struct DeliveryClient {
    endpoint: Endpoint,
    json: Agent,
    files: Agent,
    credentials: Mutex<Option<Credentials>>,
}

impl DeliveryClient {
    pub fn new(endpoint: Endpoint) -> Result<Self, DeliveryError> {
        Ok(Self {
            json: endpoint.agent(RESPONSE_TIMEOUT)?,
            files: endpoint.agent(RANGE_TIMEOUT)?,
            endpoint,
            credentials: Mutex::new(None),
        })
    }

    /// Checks the account with the service, and keeps it for later requests
    /// only if the service accepts it.
    pub fn sign_in(
        &self,
        username: &str,
        password: Zeroizing<String>,
    ) -> Result<(), DeliveryError> {
        let credentials = Credentials {
            username: username.trim().to_owned(),
            password,
        };
        let response = self.send(&self.json, "", &[], &credentials)?;
        expect_success(response)?;
        *self.credentials.lock().expect("credentials lock") = Some(credentials);
        Ok(())
    }

    pub fn sign_out(&self) {
        *self.credentials.lock().expect("credentials lock") = None;
    }

    pub fn signed_in_as(&self) -> Option<String> {
        self.credentials
            .lock()
            .expect("credentials lock")
            .as_ref()
            .map(|credentials| credentials.username.clone())
    }

    pub fn list(&self, path: &str) -> Result<Vec<Entry>, DeliveryError> {
        let response = self.signed_in(&self.json, "/list", path, &[])?;
        let mut response = expect_success(response)?;
        let listing: Listing = read_json(&mut response)?;
        Ok(listing.entries)
    }

    /// Reads a small file whole, such as a `_manifest.json`.
    pub fn read_small(&self, path: &str, limit: u64) -> Result<Vec<u8>, DeliveryError> {
        let response = self.signed_in(&self.json, "/file", path, &[])?;
        let mut response = expect_success(response)?;
        let mut data = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut data)
            .map_err(unreachable)?;
        if data.len() as u64 > limit {
            return Err(DeliveryError::problem(
                DeliveryProblem::Unavailable,
                format!("{path} is larger than expected"),
            ));
        }
        Ok(data)
    }

    /// Requests `length` bytes of a file from `offset`. `if_range` is the
    /// Last-Modified of the first answer, so a file replaced meanwhile is
    /// refused rather than mixed.
    pub fn range(
        &self,
        path: &str,
        offset: u64,
        length: u64,
        if_range: Option<&str>,
    ) -> Result<RangeReply, DeliveryError> {
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        let mut headers = vec![("Range", range)];
        if let Some(value) = if_range {
            headers.push(("If-Range", value.to_owned()));
        }
        let response = self.signed_in(&self.files, "/file", path, &headers)?;
        if response.status().as_u16() == 200 && if_range.is_some() {
            return Err(DeliveryError::problem(
                DeliveryProblem::Unavailable,
                "the archive changed on the server during the download",
            ));
        }
        let response = expect_success(response)?;
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned)
        };
        let total = header("Content-Range")
            .and_then(|value| content_range(&value, offset))
            .ok_or_else(|| {
                DeliveryError::problem(
                    DeliveryProblem::Unavailable,
                    "the server sent an unexpected range",
                )
            })?;
        let last_modified = header("Last-Modified");
        Ok(RangeReply {
            total,
            last_modified,
            body: response.into_body(),
        })
    }

    fn signed_in(
        &self,
        agent: &Agent,
        endpoint: &str,
        path: &str,
        headers: &[(&str, String)],
    ) -> Result<Response<Body>, DeliveryError> {
        let guard = self.credentials.lock().expect("credentials lock");
        let credentials = guard.as_ref().ok_or_else(|| {
            DeliveryError::problem(DeliveryProblem::SignInRequired, "sign in to DLA first")
        })?;
        let query = format!(
            "{endpoint}?path={}",
            utf8_percent_encode(path, NON_ALPHANUMERIC)
        );
        let authorization = credentials.authorization();
        drop(guard);
        self.send_with(agent, &query, headers, &authorization)
    }

    fn send(
        &self,
        agent: &Agent,
        endpoint: &str,
        headers: &[(&str, String)],
        credentials: &Credentials,
    ) -> Result<Response<Body>, DeliveryError> {
        self.send_with(agent, endpoint, headers, &credentials.authorization())
    }

    fn send_with(
        &self,
        agent: &Agent,
        endpoint: &str,
        headers: &[(&str, String)],
        authorization: &str,
    ) -> Result<Response<Body>, DeliveryError> {
        let mut request = agent
            .get(format!("{}{endpoint}", self.endpoint.base()))
            .header("Authorization", authorization);
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        request.call().map_err(unreachable)
    }
}

fn unreachable(error: impl fmt::Display) -> DeliveryError {
    DeliveryError::problem(DeliveryProblem::Unreachable, error.to_string())
}

fn read_json<T: serde::de::DeserializeOwned>(
    response: &mut Response<Body>,
) -> Result<T, DeliveryError> {
    let mut data = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_JSON_BYTES)
        .read_to_end(&mut data)
        .map_err(unreachable)?;
    serde_json::from_slice(&data).map_err(|_| {
        DeliveryError::problem(
            DeliveryProblem::Unavailable,
            "the downloads service sent an invalid answer",
        )
    })
}

/// Turns a refusal from the downloads service into its stable problem.
fn expect_success(mut response: Response<Body>) -> Result<Response<Body>, DeliveryError> {
    let status = response.status().as_u16();
    if matches!(status, 200 | 206) {
        return Ok(response);
    }
    let refusal: Refusal = read_json(&mut response).unwrap_or(Refusal {
        reason: String::new(),
        error: String::new(),
    });
    let problem = match (status, refusal.reason.as_str()) {
        (401, _) => DeliveryProblem::Credentials,
        (429, "busy") => DeliveryProblem::Busy,
        (429, _) => DeliveryProblem::TooManyAttempts,
        (403, "allowance") => DeliveryProblem::Allowance,
        (400 | 403 | 404, _) => DeliveryProblem::NotInArchive,
        (502 | 504, _) => DeliveryProblem::Unreachable,
        _ => DeliveryProblem::Unavailable,
    };
    let detail = if refusal.error.is_empty() {
        format!("the downloads service answered {status}")
    } else {
        refusal.error
    };
    Err(DeliveryError::problem(problem, detail))
}

/// The total size from `bytes start-end/total`, if it starts at `offset`.
fn content_range(value: &str, offset: u64) -> Option<u64> {
    let range = value.strip_prefix("bytes ")?;
    let (span, total) = range.split_once('/')?;
    let (start, _) = span.split_once('-')?;
    (start.parse::<u64>().ok()? == offset).then_some(())?;
    total.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_content_ranges() {
        assert_eq!(content_range("bytes 100-199/1000", 100), Some(1000));
        assert_eq!(content_range("bytes 100-199/1000", 0), None);
        assert_eq!(content_range("bytes */1000", 0), None);
        assert_eq!(content_range("items 0-1/2", 0), None);
    }

    #[test]
    fn the_self_test_address_is_loopback_http_only() {
        assert!(Endpoint::self_test("http://127.0.0.1:9092/v1").is_ok());
        assert!(Endpoint::self_test("http://[::1]:9092/v1").is_ok());
        for refused in [
            "https://127.0.0.1:9092/v1",
            "http://10.0.0.1:9092/v1",
            "http://example.org/v1",
            "http://127.0.0.1:9092/dav",
            "http://127.0.0.1:9092/v1?x=1",
        ] {
            assert!(Endpoint::self_test(refused).is_err(), "{refused}");
        }
    }
}
