use std::{
    io::Read,
    sync::{Arc, Mutex},
    time::Duration,
};

use dla_application::delivery::DeliveryError;
use dla_domain::delivery::DeliveryProblem;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use ureq::Agent;

use crate::{
    DeliveryClient, Endpoint,
    client::{expect_success, unreachable},
};

/// Read-only overlay files on their own service, using the current Install
/// session. Signing out also removes this client's authority to make requests.
pub struct OverlayClient {
    endpoint: Endpoint,
    agent: Agent,
    session: Arc<DeliveryClient>,
    requests: Mutex<()>,
}

impl OverlayClient {
    pub fn new(endpoint: Endpoint, session: Arc<DeliveryClient>) -> Result<Self, DeliveryError> {
        Ok(Self {
            agent: endpoint.agent(Duration::from_secs(600))?,
            endpoint,
            session,
            requests: Mutex::new(()),
        })
    }

    pub fn signed_in(&self) -> bool {
        self.session.signed_in_as().is_some()
    }

    pub fn read(&self, path: &str, limit: u64) -> Result<Vec<u8>, DeliveryError> {
        let relative = path.strip_prefix("/overlays/dlsite/");
        if limit > 64 << 20
            || !relative.is_some_and(|relative| {
                relative.split('/').all(|part| {
                    !part.is_empty() && !part.starts_with('.') && !part.contains(['\\', '%', '\0'])
                })
            })
        {
            return Err(DeliveryError::problem(
                DeliveryProblem::NotInArchive,
                "not an overlay file",
            ));
        }
        let _request = self.requests.lock().expect("overlay request lock");
        let authorization = self.session.authorization()?;
        let response = self
            .agent
            .get(format!(
                "{}/file?path={}&max_bytes={limit}",
                self.endpoint.base(),
                utf8_percent_encode(path, NON_ALPHANUMERIC)
            ))
            .header("Authorization", authorization.as_str())
            .call()
            .map_err(unreachable)?;
        let mut response = expect_success(response)?;
        let mut bytes = Vec::new();
        response
            .body_mut()
            .as_reader()
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(unreachable)?;
        if bytes.len() as u64 > limit {
            return Err(DeliveryError::problem(
                DeliveryProblem::Unavailable,
                "overlay file is larger than expected",
            ));
        }
        Ok(bytes)
    }
}
