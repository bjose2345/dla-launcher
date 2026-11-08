//! Engine detection against real installed works. Point `DLA_ENGINE_LIBRARY`
//! at a folder of them and run with `--ignored --nocapture`.

use std::path::Path;

use dla_detection::detect_game_engine;
use walkdir::WalkDir;

#[test]
#[ignore = "needs DLA_ENGINE_LIBRARY, a folder of installed works"]
fn reports_the_engine_of_each_installed_work() {
    let library = std::env::var("DLA_ENGINE_LIBRARY").expect("DLA_ENGINE_LIBRARY");
    let mut works = std::fs::read_dir(&library)
        .expect("library folder")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    works.sort();
    for work in works {
        let files = relative_files(&work);
        let detection = detect_game_engine(files.iter().map(String::as_str));
        println!(
            "{:<14} {}",
            work.file_name().unwrap_or_default().to_string_lossy(),
            detection.map_or_else(
                || "-".to_owned(),
                |found| format!(
                    "{} in {}",
                    found.engine.label(),
                    found.root.as_ref().map_or("(top)", |root| root.as_str())
                )
            )
        );
    }
}

fn relative_files(root: &Path) -> Vec<String> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter_map(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .ok()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
        })
        .collect()
}
