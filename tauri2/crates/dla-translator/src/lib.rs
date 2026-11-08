//! The player's own translators, behind the application's `Translator` port.

mod deepl;
mod local_model;
mod names;
mod pack;
mod reply;
mod settings;

use std::time::Duration;

use dla_application::translation::TranslatorError;
use ureq::Agent;

pub use deepl::DeepLTranslator;
pub use local_model::{LocalModelTranslator, TEMPERATURE, TOP_P, batch_request, read_batch_reply};
pub use names::NameList;
pub use pack::{ArchiveFiles, ArchivePacks, FileError, FolderFiles, INDEX, OVERLAYS};
pub use settings::{SavedTranslator, TranslatorSettingsFile};

// A local model on a CPU can take minutes for a full batch.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

fn agent() -> Agent {
    let config = Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build();
    Agent::new_with_config(config)
}

fn request_error(error: ureq::Error) -> TranslatorError {
    match error {
        ureq::Error::StatusCode(status) => TranslatorError::Refused(format!("HTTP {status}")),
        other => TranslatorError::Unreachable(other.to_string()),
    }
}

fn post_json(
    agent: &Agent,
    url: &str,
    authorization: Option<&str>,
    body: &serde_json::Value,
) -> Result<serde_json::Value, TranslatorError> {
    let mut request = agent.post(url).header("Content-Type", "application/json");
    if let Some(authorization) = authorization {
        request = request.header("Authorization", authorization);
    }
    let mut response = request.send(body.to_string()).map_err(request_error)?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|error| TranslatorError::UnreadableReply(error.to_string()))?;
    serde_json::from_str(&text).map_err(|error| TranslatorError::UnreadableReply(error.to_string()))
}
