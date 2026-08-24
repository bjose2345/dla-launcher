use dla_application::scanner::{ScanClock, ScanIdentifiers};
use dla_domain::scanner::ScanSessionId;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub struct SystemScanClock;

impl ScanClock for SystemScanClock {
    fn now(&self) -> String {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .expect("RFC 3339 timestamp")
    }
}

pub struct SystemScanIdentifiers;

impl ScanIdentifiers for SystemScanIdentifiers {
    fn stable_id(&self, namespace: &str, value: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(namespace.as_bytes());
        hasher.update([0]);
        hasher.update(value.as_bytes());
        format!("{namespace}-{}", hex::encode(hasher.finalize()))
    }

    fn new_session_id(&self) -> ScanSessionId {
        ScanSessionId(format!("session-{}", Uuid::new_v4()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_are_repeatable_and_namespaced() {
        let identifiers = SystemScanIdentifiers;
        assert_eq!(
            identifiers.stable_id("entry", "root/path"),
            identifiers.stable_id("entry", "root/path")
        );
        assert_ne!(
            identifiers.stable_id("entry", "root/path"),
            identifiers.stable_id("result", "root/path")
        );
    }
}
