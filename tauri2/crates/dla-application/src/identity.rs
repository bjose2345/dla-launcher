use dla_domain::CatalogWork;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveHashAlgorithm {
    Md5,
    Sha1,
    Sha256,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchiveHash {
    pub algorithm: ArchiveHashAlgorithm,
    pub digest: String,
}

impl ArchiveHash {
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let algorithm = match value.len() {
            32 => ArchiveHashAlgorithm::Md5,
            40 => ArchiveHashAlgorithm::Sha1,
            64 => ArchiveHashAlgorithm::Sha256,
            _ => return None,
        };
        Some(Self {
            algorithm,
            digest: value.to_ascii_lowercase(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogArchiveIdentity {
    pub work_code: String,
    pub rom_position: usize,
    pub name: String,
    pub size: String,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

#[derive(Debug, Error)]
pub enum CatalogIdentityError {
    #[error("catalog identity persistence failed: {0}")]
    Persistence(String),
}

impl CatalogIdentityError {
    pub fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }
}

pub trait CatalogIdentityReader: Send + Sync {
    fn read_works_by_codes(
        &self,
        work_codes: &[String],
    ) -> Result<Vec<CatalogWork>, CatalogIdentityError>;
    fn resolve_archive_hash(
        &self,
        hash: &ArchiveHash,
    ) -> Result<Vec<CatalogWork>, CatalogIdentityError>;
    fn find_archive_candidates_by_size(
        &self,
        size: &str,
        limit: usize,
    ) -> Result<Vec<CatalogArchiveIdentity>, CatalogIdentityError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_and_normalizes_supported_archive_hashes() {
        let md5 =
            ArchiveHash::parse("A23FB4E87995BDEAFBE48594D25A2260").expect("valid MD5 identity");
        let sha1 = ArchiveHash::parse("D37978B4F3E7CBB6F215733297BF01381DEB627F")
            .expect("valid SHA1 identity");
        let sha256 =
            ArchiveHash::parse("A23FB4E87995BDEAFBE48594D25A22609C07588775385121E26BFA73525B875A")
                .expect("valid SHA256 identity");

        assert_eq!(md5.algorithm, ArchiveHashAlgorithm::Md5);
        assert_eq!(sha1.algorithm, ArchiveHashAlgorithm::Sha1);
        assert_eq!(sha256.algorithm, ArchiveHashAlgorithm::Sha256);
        assert!(sha256.digest.bytes().all(|byte| !byte.is_ascii_uppercase()));
    }

    #[test]
    fn rejects_non_hex_and_unsupported_hash_lengths() {
        assert!(ArchiveHash::parse("not-a-hash").is_none());
        assert!(ArchiveHash::parse("0123456789abcdef").is_none());
        assert!(ArchiveHash::parse("g123456789abcdef0123456789abcdef").is_none());
    }
}
