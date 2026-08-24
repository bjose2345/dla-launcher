use std::collections::{BTreeMap, BTreeSet};

use dla_domain::scanner::{
    ResolvedScanIdentity, ScanEvidenceKind, ScanEvidenceObservation, ScanMatchCandidate,
    ScanMatchConfidence, ScanMatchDecision, ScanMatchOutcome,
};

mod media;
mod package;
mod package_set;

pub use media::{
    MediaClassificationError, MediaClassificationRequest, classify_media, classify_media_type,
};
pub use package::{classify_package, classify_package_media_paths};
pub use package_set::{PackageSourceSetError, discover_package_source_set};

const MINIMUM_PRODUCT_DIGITS: usize = 5;
const MAXIMUM_PRODUCT_DIGITS: usize = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProductCodeSource {
    DirectoryName,
    FileName,
}

pub fn detect_product_codes(
    value: &str,
    source: ProductCodeSource,
) -> Vec<ScanEvidenceObservation> {
    let bytes = value.as_bytes();
    let mut found = BTreeSet::new();
    let mut index = 0;
    while index + 2 <= bytes.len() {
        let prefix = [
            bytes[index].to_ascii_uppercase(),
            bytes[index + 1].to_ascii_uppercase(),
        ];
        if !matches!(&prefix, b"RJ" | b"BJ" | b"VJ")
            || (index > 0 && bytes[index - 1].is_ascii_alphanumeric())
        {
            index += 1;
            continue;
        }

        let mut digit_start = index + 2;
        if bytes
            .get(digit_start)
            .is_some_and(|byte| matches!(byte, b'_' | b'-'))
        {
            digit_start += 1;
        }
        let mut digit_end = digit_start;
        while bytes.get(digit_end).is_some_and(u8::is_ascii_digit) {
            digit_end += 1;
        }
        let digit_count = digit_end - digit_start;
        let valid_end = bytes
            .get(digit_end)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric());
        if (MINIMUM_PRODUCT_DIGITS..=MAXIMUM_PRODUCT_DIGITS).contains(&digit_count) && valid_end {
            let digits = &bytes[digit_start..digit_end];
            let mut code = String::with_capacity(2 + digits.len());
            code.push(prefix[0] as char);
            code.push(prefix[1] as char);
            code.extend(digits.iter().map(|byte| *byte as char));
            found.insert(code);
        }
        index = digit_end.max(index + 1);
    }

    let reason_code = match source {
        ProductCodeSource::DirectoryName => "code_in_directory_name",
        ProductCodeSource::FileName => "code_in_filename",
    };
    found
        .into_iter()
        .map(|normalized_value| ScanEvidenceObservation {
            kind: ScanEvidenceKind::ProductCode,
            normalized_value,
            reason_code: reason_code.to_owned(),
        })
        .collect()
}

pub fn resolve_scan_identities(identities: &[ResolvedScanIdentity]) -> ScanMatchDecision {
    let mut candidates = BTreeMap::<String, (ScanMatchConfidence, BTreeSet<String>)>::new();
    for identity in identities {
        let work_code = identity.work_code.trim().to_ascii_uppercase();
        if work_code.is_empty() {
            continue;
        }
        let candidate = candidates
            .entry(work_code)
            .or_insert((identity.confidence, BTreeSet::new()));
        candidate.0 = candidate.0.max(identity.confidence);
        candidate.1.extend(
            identity
                .reason_codes
                .iter()
                .map(|reason| reason.trim().to_owned())
                .filter(|reason| !reason.is_empty()),
        );
    }

    let mut candidates = candidates
        .into_iter()
        .map(|(work_code, (confidence, reasons))| ScanMatchCandidate {
            work_code,
            confidence,
            reason_codes: reasons.into_iter().collect(),
            rank: 0,
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| left.work_code.cmp(&right.work_code))
    });
    for (index, candidate) in candidates.iter_mut().enumerate() {
        candidate.rank = index as u32 + 1;
    }

    match candidates.as_slice() {
        [] => ScanMatchDecision {
            outcome: ScanMatchOutcome::Unmatched,
            selected_work_code: None,
            confidence: None,
            candidates,
        },
        [candidate] => ScanMatchDecision {
            outcome: ScanMatchOutcome::Matched,
            selected_work_code: Some(candidate.work_code.clone()),
            confidence: Some(candidate.confidence),
            candidates,
        },
        _ => ScanMatchDecision {
            outcome: ScanMatchOutcome::Ambiguous,
            selected_work_code: None,
            confidence: None,
            candidates,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn extracts_normalized_codes_without_duplicates() {
        let evidence = detect_product_codes(
            "作品 rj01326398 + RJ_01326398 + bj-12345 + VJ987654.zip",
            ProductCodeSource::FileName,
        );

        assert_eq!(
            evidence
                .iter()
                .map(|item| item.normalized_value.as_str())
                .collect::<Vec<_>>(),
            vec!["BJ12345", "RJ01326398", "VJ987654"]
        );
        assert!(
            evidence
                .iter()
                .all(|item| item.reason_code == "code_in_filename")
        );
    }

    #[test]
    fn ignores_embedded_and_malformed_codes() {
        assert!(
            detect_product_codes("prefixRJ01326398suffix", ProductCodeSource::FileName).is_empty()
        );
        assert!(detect_product_codes("RJ1234", ProductCodeSource::FileName).is_empty());
        assert!(detect_product_codes("RJ12345678901", ProductCodeSource::FileName).is_empty());
    }

    #[test]
    fn resolves_one_identity_and_merges_its_reasons() {
        let decision = resolve_scan_identities(&[
            identity(
                "rj01326398",
                ScanMatchConfidence::Strong,
                "code_in_filename",
            ),
            identity(
                "RJ01326398",
                ScanMatchConfidence::Exact,
                "archive_sha256_match",
            ),
        ]);

        assert_eq!(decision.outcome, ScanMatchOutcome::Matched);
        assert_eq!(decision.selected_work_code.as_deref(), Some("RJ01326398"));
        assert_eq!(decision.confidence, Some(ScanMatchConfidence::Exact));
        assert_eq!(decision.candidates[0].reason_codes.len(), 2);
    }

    #[test]
    fn preserves_conflicting_identities_as_ambiguous() {
        let decision = resolve_scan_identities(&[
            identity(
                "RJ01326398",
                ScanMatchConfidence::Strong,
                "code_in_directory_name",
            ),
            identity(
                "RJ01653537",
                ScanMatchConfidence::Exact,
                "archive_sha256_match",
            ),
        ]);

        assert_eq!(decision.outcome, ScanMatchOutcome::Ambiguous);
        assert_eq!(decision.selected_work_code, None);
        assert_eq!(decision.candidates[0].work_code, "RJ01653537");
    }

    #[test]
    fn shared_scenarios_preserve_detection_and_decision_contracts() {
        let manifest: ScenarioManifest = serde_json::from_str(include_str!(
            "../../../../shared/fixtures/scenarios/manifest-v1.json"
        ))
        .expect("scanner scenario manifest");
        assert_eq!(manifest.schema_version, 1);

        let mut scenario_ids = BTreeSet::new();
        for scenario in manifest.scenarios {
            assert!(scenario_ids.insert(scenario.id.clone()));
            assert!(!scenario.description.trim().is_empty());
            let mut product_codes = BTreeSet::new();
            for entry in &scenario.entries {
                assert!(!entry.relative_path.trim().is_empty());
                assert!(matches!(
                    entry.kind,
                    FixtureEntryKind::File | FixtureEntryKind::Directory
                ));
                if entry.symbolic_link {
                    assert_eq!(entry.kind, FixtureEntryKind::Directory);
                }
                if !entry.readable {
                    assert!(scenario.issues.contains(&FixtureIssue::PermissionDenied));
                }
                let source = match entry.source {
                    FixtureSource::DirectoryName => ProductCodeSource::DirectoryName,
                    FixtureSource::FileName => ProductCodeSource::FileName,
                };
                product_codes.extend(
                    detect_product_codes(&entry.relative_path, source)
                        .into_iter()
                        .map(|evidence| evidence.normalized_value),
                );
            }
            let identities = scenario
                .identities
                .into_iter()
                .map(|item| ResolvedScanIdentity {
                    work_code: item.work_code,
                    confidence: match item.confidence {
                        FixtureConfidence::Possible => ScanMatchConfidence::Possible,
                        FixtureConfidence::Strong => ScanMatchConfidence::Strong,
                        FixtureConfidence::Exact => ScanMatchConfidence::Exact,
                    },
                    reason_codes: item.reason_codes,
                })
                .collect::<Vec<_>>();
            let decision = resolve_scan_identities(&identities);

            assert_eq!(
                product_codes.into_iter().collect::<Vec<_>>(),
                scenario.expected.product_codes,
                "{} product-code evidence",
                scenario.id
            );
            assert_eq!(
                decision.outcome,
                match scenario.expected.outcome {
                    FixtureOutcome::Matched => ScanMatchOutcome::Matched,
                    FixtureOutcome::Ambiguous => ScanMatchOutcome::Ambiguous,
                    FixtureOutcome::Unmatched => ScanMatchOutcome::Unmatched,
                },
                "{} match outcome",
                scenario.id
            );
            assert_eq!(
                decision.selected_work_code, scenario.expected.selected_work_code,
                "{} selected work",
                scenario.id
            );
        }
    }

    fn identity(
        work_code: &str,
        confidence: ScanMatchConfidence,
        reason: &str,
    ) -> ResolvedScanIdentity {
        ResolvedScanIdentity {
            work_code: work_code.to_owned(),
            confidence,
            reason_codes: vec![reason.to_owned()],
        }
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ScenarioManifest {
        schema_version: u32,
        scenarios: Vec<FixtureScenario>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureScenario {
        id: String,
        description: String,
        entries: Vec<FixtureEntry>,
        identities: Vec<FixtureIdentity>,
        issues: Vec<FixtureIssue>,
        expected: FixtureExpected,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureEntry {
        relative_path: String,
        source: FixtureSource,
        kind: FixtureEntryKind,
        readable: bool,
        symbolic_link: bool,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum FixtureSource {
        DirectoryName,
        FileName,
    }

    #[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum FixtureEntryKind {
        File,
        Directory,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureIdentity {
        work_code: String,
        confidence: FixtureConfidence,
        reason_codes: Vec<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum FixtureConfidence {
        Possible,
        Strong,
        Exact,
    }

    #[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
    #[serde(rename_all = "snake_case")]
    enum FixtureIssue {
        RootUnavailable,
        PermissionDenied,
        EntryVanished,
        UnsupportedEntry,
        Io,
        Persistence,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FixtureExpected {
        product_codes: Vec<String>,
        outcome: FixtureOutcome,
        selected_work_code: Option<String>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum FixtureOutcome {
        Matched,
        Ambiguous,
        Unmatched,
    }
}
