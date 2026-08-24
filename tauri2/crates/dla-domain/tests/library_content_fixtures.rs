use std::{collections::HashSet, fs, path::PathBuf};

use dla_domain::installation::{InstallationDetection, LaunchTarget, MediaType, RelativePath};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureManifest {
    schema_version: u32,
    scenarios: Vec<FixtureScenario>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureScenario {
    id: String,
    description: String,
    fixture_root: RelativePath,
    expected: InstallationDetection,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../shared/fixtures/library-content")
}

#[test]
fn library_content_scenarios_are_safe_complete_and_deterministic() {
    let root = fixture_root();
    let manifest: FixtureManifest = serde_json::from_str(
        &fs::read_to_string(root.join("manifest-v1.json")).expect("fixture manifest"),
    )
    .expect("valid fixture manifest");
    assert_eq!(manifest.schema_version, 1);
    assert!(!manifest.scenarios.is_empty());

    let mut scenario_ids = HashSet::new();
    let mut covered_types = HashSet::new();
    for scenario in manifest.scenarios {
        assert!(scenario_ids.insert(scenario.id.clone()));
        assert!(!scenario.description.trim().is_empty());
        scenario
            .expected
            .validate()
            .expect("valid detection contract");
        let scenario_root = root.join(scenario.fixture_root.as_str());
        assert!(scenario_root.is_dir(), "missing scenario: {}", scenario.id);

        let paths = scenario
            .expected
            .content_items
            .iter()
            .map(|item| item.relative_path.as_str())
            .collect::<Vec<_>>();
        let mut sorted_paths = paths.clone();
        sorted_paths.sort_unstable();
        assert_eq!(
            paths, sorted_paths,
            "unstable content order: {}",
            scenario.id
        );

        for item in &scenario.expected.content_items {
            covered_types.insert(item.media_type);
            assert!(
                scenario_root.join(item.relative_path.as_str()).is_file(),
                "missing fixture path: {}/{}",
                scenario.id,
                item.relative_path
            );
        }
        for candidate in &scenario.expected.launch_candidates {
            if let LaunchTarget::RelativePath(target) = &candidate.target {
                assert!(
                    scenario
                        .expected
                        .content_items
                        .iter()
                        .any(|item| item.relative_path == *target),
                    "candidate target is not classified: {}/{}",
                    scenario.id,
                    target
                );
            }
        }
    }

    assert!(covered_types.is_superset(&HashSet::from([
        MediaType::Executable,
        MediaType::Audio,
        MediaType::Image,
        MediaType::Pdf,
        MediaType::Video,
        MediaType::Archive,
        MediaType::AndroidPackage,
        MediaType::Unknown,
    ])));
}
