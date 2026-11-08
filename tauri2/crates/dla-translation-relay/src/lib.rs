//! The local relay: game plugins on the player's own PC ask the launcher for
//! translations here. It listens only on 127.0.0.1, on a random port, and
//! answers only requests that carry the password of this launch.

use std::{
    collections::HashMap,
    io::{self, Read},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

use dla_domain::translation::TextKind;
use serde::{Deserialize, Serialize};
use tiny_http::{Header, Method, Request, Response, Server};

const MAX_BODY_BYTES: u64 = 2 * 1024 * 1024;
// A game asks for its current scene and the next ones; more than this at once
// means something is wrong, and the translator could not keep up anyway.
const MAX_ACTIVE_REQUESTS: usize = 8;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TranslateRequest {
    pub work: String,
    pub texts: Vec<RelayText>,
    #[serde(default)]
    pub urgent: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RelayText {
    pub text: String,
    pub kind: TextKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NamesRequest {
    pub work: String,
    pub names: Vec<String>,
    #[serde(default)]
    pub urgent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RelayError {
    NoTranslator,
    Translator(String),
    Internal(String),
}

pub trait RelayHandler: Send + Sync {
    fn translate(&self, request: TranslateRequest) -> Result<Vec<Option<String>>, RelayError>;

    fn names(&self, request: NamesRequest) -> Result<HashMap<String, String>, RelayError>;
}

pub struct TranslationRelay {
    address: SocketAddr,
    token: String,
    server: Arc<Server>,
    listener: Option<JoinHandle<()>>,
}

impl TranslationRelay {
    pub fn start(handler: Arc<dyn RelayHandler>) -> io::Result<Self> {
        let server = Arc::new(Server::http("127.0.0.1:0").map_err(io::Error::other)?);
        let address = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| io::Error::other("the relay is not on a network address"))?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let listener = {
            let server = server.clone();
            let token = token.clone();
            thread::Builder::new()
                .name("dla-translation-relay".to_owned())
                .spawn(move || listen(&server, &token, &handler))?
        };
        Ok(Self {
            address,
            token,
            server,
            listener: Some(listener),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The password game plugins send as `Authorization: Bearer <token>`.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for TranslationRelay {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
    }
}

fn listen(server: &Server, token: &str, handler: &Arc<dyn RelayHandler>) {
    let active = Arc::new(AtomicUsize::new(0));
    for request in server.incoming_requests() {
        if active.fetch_add(1, Ordering::SeqCst) >= MAX_ACTIVE_REQUESTS {
            active.fetch_sub(1, Ordering::SeqCst);
            let _ = request.respond(error(503, "busy"));
            continue;
        }
        let token = token.to_owned();
        let handler = handler.clone();
        let finished = active.clone();
        let spawned = thread::Builder::new()
            .name("dla-translation-request".to_owned())
            .spawn(move || {
                answer(request, &token, handler.as_ref());
                finished.fetch_sub(1, Ordering::SeqCst);
            });
        if spawned.is_err() {
            active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

fn answer(mut request: Request, token: &str, handler: &dyn RelayHandler) {
    let method = request.method().clone();
    let path = request
        .url()
        .split('?')
        .next()
        .unwrap_or_default()
        .to_owned();
    let response = if !authorized(&request, token) {
        error(401, "unauthorized")
    } else {
        match (method, path.as_str()) {
            (Method::Get, "/v1/health") => json(200, &serde_json::json!({ "ok": true })),
            (Method::Post, "/v1/translate") => match body::<TranslateRequest>(&mut request) {
                Ok(translate) => match handler.translate(translate) {
                    Ok(translations) => {
                        json(200, &serde_json::json!({ "translations": translations }))
                    }
                    Err(failure) => relay_error(&failure),
                },
                Err(response) => response,
            },
            (Method::Post, "/v1/names") => match body::<NamesRequest>(&mut request) {
                Ok(names) => match handler.names(names) {
                    Ok(names) => json(200, &serde_json::json!({ "names": names })),
                    Err(failure) => relay_error(&failure),
                },
                Err(response) => response,
            },
            _ => error(404, "not_found"),
        }
    };
    let _ = request.respond(response);
}

fn authorized(request: &Request, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    request
        .headers()
        .iter()
        .any(|header| header.field.equiv("Authorization") && header.value.as_str() == expected)
}

fn body<T: for<'de> Deserialize<'de>>(
    request: &mut Request,
) -> Result<T, Response<io::Cursor<Vec<u8>>>> {
    let mut bytes = Vec::new();
    request
        .as_reader()
        .take(MAX_BODY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| error(400, "unreadable_body"))?;
    if bytes.len() as u64 > MAX_BODY_BYTES {
        return Err(error(413, "too_large"));
    }
    serde_json::from_slice(&bytes).map_err(|_| error(400, "bad_request"))
}

fn relay_error(failure: &RelayError) -> Response<io::Cursor<Vec<u8>>> {
    match failure {
        RelayError::NoTranslator => error(503, "no_translator"),
        RelayError::Translator(_) => error(502, "translator_unavailable"),
        RelayError::Internal(_) => error(500, "internal"),
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

fn error(status: u16, code: &str) -> Response<io::Cursor<Vec<u8>>> {
    json(status, &ErrorBody { error: code })
}

fn json(status: u16, body: &impl Serialize) -> Response<io::Cursor<Vec<u8>>> {
    let bytes = serde_json::to_vec(body).unwrap_or_default();
    let content_type =
        Header::from_bytes("Content-Type", "application/json").expect("valid header");
    Response::from_data(bytes)
        .with_status_code(status)
        .with_header(content_type)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        seen: Mutex<Vec<TranslateRequest>>,
        no_translator: bool,
    }

    impl RelayHandler for Recorder {
        fn translate(&self, request: TranslateRequest) -> Result<Vec<Option<String>>, RelayError> {
            if self.no_translator {
                return Err(RelayError::NoTranslator);
            }
            let translations = request
                .texts
                .iter()
                .map(|text| (text.text == "はい").then(|| "Yes".to_owned()))
                .collect();
            self.seen.lock().unwrap().push(request);
            Ok(translations)
        }

        fn names(&self, request: NamesRequest) -> Result<HashMap<String, String>, RelayError> {
            Ok(request
                .names
                .into_iter()
                .map(|name| (name, "Yukina".to_owned()))
                .collect())
        }
    }

    fn post(
        relay: &TranslationRelay,
        path: &str,
        token: &str,
        body: &str,
    ) -> Result<serde_json::Value, u16> {
        let url = format!("http://{}{path}", relay.address());
        match ureq::post(&url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send(body)
        {
            Ok(mut response) => {
                Ok(serde_json::from_str(&response.body_mut().read_to_string().unwrap()).unwrap())
            }
            Err(ureq::Error::StatusCode(status)) => Err(status),
            Err(other) => panic!("request failed: {other}"),
        }
    }

    #[test]
    fn translates_for_a_plugin_that_knows_the_password() {
        let recorder = Arc::new(Recorder::default());
        let relay = TranslationRelay::start(recorder.clone()).unwrap();
        assert!(relay.address().ip().is_loopback());
        let reply = post(
            &relay,
            "/v1/translate",
            relay.token(),
            r#"{"work":"RJ1","texts":[{"text":"はい","kind":"choice"},{"text":"剣","kind":"database"}],"urgent":true}"#,
        )
        .unwrap();
        assert_eq!(reply, serde_json::json!({ "translations": ["Yes", null] }));
        let seen = recorder.seen.lock().unwrap();
        assert_eq!(seen[0].work, "RJ1");
        assert!(seen[0].urgent);
        assert_eq!(seen[0].texts[1].kind, TextKind::Database);
    }

    #[test]
    fn refuses_requests_without_the_password() {
        let relay = TranslationRelay::start(Arc::new(Recorder::default())).unwrap();
        assert_eq!(
            post(
                &relay,
                "/v1/translate",
                "guess",
                r#"{"work":"RJ1","texts":[]}"#
            ),
            Err(401)
        );
    }

    #[test]
    fn answers_bad_requests_and_unknown_paths_plainly() {
        let relay = TranslationRelay::start(Arc::new(Recorder::default())).unwrap();
        assert_eq!(
            post(&relay, "/v1/translate", relay.token(), "not json"),
            Err(400)
        );
        assert_eq!(post(&relay, "/v1/elsewhere", relay.token(), "{}"), Err(404));
    }

    #[test]
    fn says_when_no_translator_is_set_up() {
        let relay = TranslationRelay::start(Arc::new(Recorder {
            no_translator: true,
            ..Recorder::default()
        }))
        .unwrap();
        assert_eq!(
            post(
                &relay,
                "/v1/translate",
                relay.token(),
                r#"{"work":"RJ1","texts":[{"text":"はい","kind":"choice"}]}"#
            ),
            Err(503)
        );
    }

    #[test]
    fn spells_names() {
        let relay = TranslationRelay::start(Arc::new(Recorder::default())).unwrap();
        let reply = post(
            &relay,
            "/v1/names",
            relay.token(),
            r#"{"work":"RJ1","names":["ユキナ"]}"#,
        )
        .unwrap();
        assert_eq!(
            reply,
            serde_json::json!({ "names": { "ユキナ": "Yukina" } })
        );
    }

    #[test]
    fn stops_listening_when_dropped() {
        let relay = TranslationRelay::start(Arc::new(Recorder::default())).unwrap();
        let (address, token) = (relay.address(), relay.token().to_owned());
        drop(relay);
        // Tests run side by side, so another test's relay may get the freed
        // port; it does not take this relay's password either.
        let reply = ureq::post(&format!("http://{address}/v1/names"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .send(r#"{"work":"RJ1","names":[]}"#);
        assert!(reply.is_err());
    }
}
