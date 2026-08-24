use std::collections::HashSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AndroidAppAssociationId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAppAssociation {
    pub id: AndroidAppAssociationId,
    pub work_code: String,
    pub package_name: String,
    pub application_label: String,
    pub expected_signing_certificate_sha256: Vec<String>,
    pub associated_version_name: Option<String>,
    pub associated_version_code: String,
    pub associated_at: String,
    pub updated_at: String,
    pub last_launched_at: Option<String>,
    pub launch_count: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AndroidAppRuntimeState {
    Ready,
    NotLaunchable,
    Missing,
    SignerMismatch,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAppRuntimeStatus {
    pub state: AndroidAppRuntimeState,
    pub application_label: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<String>,
    pub technical_detail: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AndroidAppView {
    pub association: AndroidAppAssociation,
    pub runtime: AndroidAppRuntimeStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AndroidAppAssociationError {
    InvalidId,
    InvalidWorkCode,
    InvalidPackageName,
    InvalidApplicationLabel,
    InvalidVersion,
    MissingSigningCertificate,
    InvalidSigningCertificate,
    InvalidTimestamp,
}

impl std::fmt::Display for AndroidAppAssociationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidId => "invalid Android application association identifier",
            Self::InvalidWorkCode => "invalid Android application work code",
            Self::InvalidPackageName => "invalid Android package name",
            Self::InvalidApplicationLabel => "invalid Android application label",
            Self::InvalidVersion => "invalid Android application version",
            Self::MissingSigningCertificate => "Android application has no signing certificate",
            Self::InvalidSigningCertificate => "invalid Android signing-certificate fingerprint",
            Self::InvalidTimestamp => "invalid Android application association timestamp",
        })
    }
}

impl std::error::Error for AndroidAppAssociationError {}

impl AndroidAppAssociation {
    pub fn validate(&self) -> Result<(), AndroidAppAssociationError> {
        if !valid_opaque_id(&self.id.0) {
            return Err(AndroidAppAssociationError::InvalidId);
        }
        if self.work_code.trim().is_empty() || self.work_code.len() > 64 {
            return Err(AndroidAppAssociationError::InvalidWorkCode);
        }
        if !valid_package_name(&self.package_name) {
            return Err(AndroidAppAssociationError::InvalidPackageName);
        }
        if self.application_label.trim().is_empty() || self.application_label.len() > 512 {
            return Err(AndroidAppAssociationError::InvalidApplicationLabel);
        }
        if self.associated_version_code.is_empty()
            || self.associated_version_code.len() > 32
            || !self
                .associated_version_code
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            || self
                .associated_version_name
                .as_ref()
                .is_some_and(|version| version.len() > 256)
        {
            return Err(AndroidAppAssociationError::InvalidVersion);
        }
        if self.expected_signing_certificate_sha256.is_empty() {
            return Err(AndroidAppAssociationError::MissingSigningCertificate);
        }
        if self.expected_signing_certificate_sha256.len() > 32
            || self
                .expected_signing_certificate_sha256
                .iter()
                .any(|fingerprint| !valid_sha256(fingerprint))
            || self
                .expected_signing_certificate_sha256
                .iter()
                .map(|fingerprint| fingerprint.to_ascii_lowercase())
                .collect::<HashSet<_>>()
                .len()
                != self.expected_signing_certificate_sha256.len()
        {
            return Err(AndroidAppAssociationError::InvalidSigningCertificate);
        }
        if self.associated_at.trim().is_empty()
            || self.updated_at.trim().is_empty()
            || self
                .last_launched_at
                .as_ref()
                .is_some_and(|timestamp| timestamp.trim().is_empty())
        {
            return Err(AndroidAppAssociationError::InvalidTimestamp);
        }
        Ok(())
    }
}

impl AndroidAppAssociationId {
    pub fn validate(&self) -> Result<(), AndroidAppAssociationError> {
        if valid_opaque_id(&self.0) {
            Ok(())
        } else {
            Err(AndroidAppAssociationError::InvalidId)
        }
    }
}

pub fn valid_package_name(value: &str) -> bool {
    value.len() <= 255
        && value.split('.').count() >= 2
        && value.split('.').all(|segment| {
            let mut bytes = segment.bytes();
            bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
                && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
}

pub fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_opaque_id(value: &str) -> bool {
    (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_a_certificate_bound_association() {
        let association = fixture();
        assert_eq!(association.validate(), Ok(()));
    }

    #[test]
    fn rejects_unsafe_package_names_and_duplicate_certificates() {
        let mut association = fixture();
        association.package_name = "../fixture".to_owned();
        assert_eq!(
            association.validate(),
            Err(AndroidAppAssociationError::InvalidPackageName)
        );

        association = fixture();
        association
            .expected_signing_certificate_sha256
            .push("a".repeat(64));
        assert_eq!(
            association.validate(),
            Err(AndroidAppAssociationError::InvalidSigningCertificate)
        );
    }

    fn fixture() -> AndroidAppAssociation {
        AndroidAppAssociation {
            id: AndroidAppAssociationId("android-app-1234567890".to_owned()),
            work_code: "RJ01326398".to_owned(),
            package_name: "org.dlaproject.fixture".to_owned(),
            application_label: "Fixture".to_owned(),
            expected_signing_certificate_sha256: vec!["a".repeat(64)],
            associated_version_name: Some("1.0".to_owned()),
            associated_version_code: "1".to_owned(),
            associated_at: "2026-08-22T12:00:00Z".to_owned(),
            updated_at: "2026-08-22T12:00:00Z".to_owned(),
            last_launched_at: None,
            launch_count: 0,
        }
    }
}
