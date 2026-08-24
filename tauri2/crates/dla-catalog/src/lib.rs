#[cfg(any(test, feature = "test-fixtures"))]
use std::collections::HashSet;

use dla_domain::{CatalogRelation, CatalogRomContents, CatalogWorkDetail};
use serde::Deserialize;
#[cfg(any(test, feature = "test-fixtures"))]
use thiserror::Error;

#[cfg(any(test, feature = "test-fixtures"))]
const SYNTHETIC_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/testdata/catalog-v2.json"
));

#[derive(Clone, Debug)]
pub struct CatalogFixture {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub works: Vec<CatalogWorkDetail>,
    pub relations: Vec<CatalogRelation>,
    pub rom_contents: Vec<CatalogRomContentsFixture>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRomContentsFixture {
    pub work_code: String,
    pub rom_position: usize,
    pub contents: CatalogRomContents,
}

#[cfg(any(test, feature = "test-fixtures"))]
#[derive(Debug, Error)]
pub enum FixtureError {
    #[error("invalid {kind} catalog fixture: {source}")]
    Json {
        kind: &'static str,
        source: serde_json::Error,
    },
    #[error("fixture schema version {0} is unsupported")]
    SchemaVersion(u32),
    #[error("fixture kind is {actual}, expected {expected}")]
    Kind {
        actual: String,
        expected: &'static str,
    },
    #[error("fixture contains an empty {field} for {code}")]
    EmptyField { field: &'static str, code: String },
    #[error("fixture contains duplicate work code {0}")]
    DuplicateCode(String),
    #[error("fixture contains unsupported image URL {url} for {code}")]
    ImageUrl { code: String, url: String },
}

#[cfg(any(test, feature = "test-fixtures"))]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureFile {
    schema_version: u32,
    snapshot_id: String,
    kind: String,
    source: String,
    works: Vec<CatalogWorkDetail>,
    relations: Vec<CatalogRelation>,
}

pub fn empty() -> CatalogFixture {
    CatalogFixture {
        schema_version: 2,
        snapshot_id: "empty-catalog-v1".to_owned(),
        works: Vec::new(),
        relations: Vec::new(),
        rom_contents: Vec::new(),
    }
}

#[cfg(any(test, feature = "test-fixtures"))]
pub fn load_test_fixture() -> Result<CatalogFixture, FixtureError> {
    let synthetic = parse_fixture("synthetic", SYNTHETIC_FIXTURE)?;
    let works = synthetic
        .works
        .into_iter()
        .map(|mut work| {
            work.work.synthetic = true;
            work
        })
        .collect::<Vec<_>>();
    validate_works(&works)?;
    let relations = synthetic.relations;
    validate_relations(&works, &relations)?;

    Ok(CatalogFixture {
        schema_version: synthetic.schema_version,
        snapshot_id: synthetic.snapshot_id,
        works,
        relations,
        rom_contents: Vec::new(),
    })
}

#[cfg(any(test, feature = "test-fixtures"))]
fn parse_fixture(expected_kind: &'static str, value: &str) -> Result<FixtureFile, FixtureError> {
    let fixture: FixtureFile =
        serde_json::from_str(value).map_err(|source| FixtureError::Json {
            kind: expected_kind,
            source,
        })?;
    if fixture.schema_version != 2 {
        return Err(FixtureError::SchemaVersion(fixture.schema_version));
    }
    if fixture.kind != expected_kind {
        return Err(FixtureError::Kind {
            actual: fixture.kind,
            expected: expected_kind,
        });
    }
    if fixture.snapshot_id.trim().is_empty() || fixture.source.trim().is_empty() {
        return Err(FixtureError::EmptyField {
            field: "fixture metadata",
            code: expected_kind.to_owned(),
        });
    }
    Ok(fixture)
}

#[cfg(any(test, feature = "test-fixtures"))]
fn validate_works(works: &[CatalogWorkDetail]) -> Result<(), FixtureError> {
    let mut codes = HashSet::with_capacity(works.len());
    for detail in works {
        let work = &detail.work;
        for (field, value) in [
            ("code", work.code.as_str()),
            ("sourceCode", work.source_code.as_str()),
            ("title", work.title.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(FixtureError::EmptyField {
                    field,
                    code: work.code.clone(),
                });
            }
        }

        if !codes.insert(work.code.to_lowercase()) {
            return Err(FixtureError::DuplicateCode(work.code.clone()));
        }
        for url in work
            .main_image_urls
            .iter()
            .chain(&work.thumbnail_urls)
            .chain(&detail.sample_image_urls)
        {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                return Err(FixtureError::ImageUrl {
                    code: work.code.clone(),
                    url: url.clone(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "test-fixtures"))]
fn validate_relations(
    works: &[CatalogWorkDetail],
    relations: &[CatalogRelation],
) -> Result<(), FixtureError> {
    let codes = works
        .iter()
        .map(|detail| detail.work.code.to_lowercase())
        .collect::<HashSet<_>>();
    let mut unique = HashSet::with_capacity(relations.len());
    for relation in relations {
        for (field, value) in [
            ("parentWorkCode", relation.parent_work_code.as_str()),
            ("childWorkCode", relation.child_work_code.as_str()),
            ("relationTypeCode", relation.relation_type_code.as_str()),
            ("relationTypeLabel", relation.relation_type_label.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(FixtureError::EmptyField {
                    field,
                    code: relation.child_work_code.clone(),
                });
            }
        }
        if !codes.contains(&relation.parent_work_code.to_lowercase())
            || !codes.contains(&relation.child_work_code.to_lowercase())
        {
            return Err(FixtureError::EmptyField {
                field: "relation endpoint",
                code: relation.child_work_code.clone(),
            });
        }
        if !unique.insert((
            relation.parent_work_code.to_lowercase(),
            relation.child_work_code.to_lowercase(),
            relation.relation_type_code.to_lowercase(),
        )) {
            return Err(FixtureError::DuplicateCode(format!(
                "{}>{}:{}",
                relation.parent_work_code, relation.child_work_code, relation.relation_type_code
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_catalog_is_empty() {
        let fixture = empty();
        assert_eq!(fixture.schema_version, 2);
        assert!(fixture.works.is_empty());
        assert!(fixture.relations.is_empty());
        assert!(fixture.rom_contents.is_empty());
    }

    #[test]
    fn loads_the_synthetic_test_snapshot() {
        let fixture = load_test_fixture().expect("fixture");
        assert_eq!(fixture.schema_version, 2);
        assert_eq!(fixture.works.len(), 12);
        assert!(fixture.works.iter().all(|work| work.work.synthetic));
        assert!(
            fixture
                .works
                .iter()
                .any(|work| work.work.code == "DLA-SYNTH-0008")
        );
    }
}
