//! Times a complete Tantivy rebuild from an imported catalog into a throwaway
//! cache directory, then a few representative queries through the search
//! service.
//!
//! ```bash
//! cargo run --release -p dla-search-tantivy --example search_rebuild_bench -- <catalog.sqlite>
//! ```

use std::{
    env,
    error::Error,
    path::Path,
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use dla_application::{
    catalog::CatalogFacetFilters,
    search::{CatalogSearchIndex, CatalogSearchService, SearchRequest},
};
use dla_search_tantivy::TantivyCatalogSearch;
use dla_sqlite::SqliteCatalogStore;

const QUERIES: [&str; 4] = ["RJ0132", "whisp", "おねえさん", "asmr 耳かき"];
const WARM_ITERATIONS: usize = 20;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("search rebuild bench failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: search_rebuild_bench <catalog.sqlite>")?;
    let store = Arc::new(SqliteCatalogStore::open_existing(Path::new(&path))?);
    let cache = tempfile::tempdir()?;
    let index = Arc::new(TantivyCatalogSearch::open(
        cache.path().join("search/catalog"),
    )?);

    let started = Instant::now();
    let status = index.rebuild(&*store)?;
    println!(
        "rebuild: {:?}, {} documents in {:.2} s",
        status.state,
        status.indexed_documents,
        started.elapsed().as_secs_f64()
    );

    let service = CatalogSearchService::new(
        Arc::clone(&store) as Arc<_>,
        Arc::clone(&store) as Arc<_>,
        Arc::clone(&store) as Arc<_>,
        index as Arc<_>,
    );
    for text in QUERIES {
        let request = || SearchRequest {
            text: text.to_owned(),
            facets: CatalogFacetFilters::default(),
            limit: 24,
            offset: 0,
        };
        let started = Instant::now();
        let response = service.search(request())?;
        let first = started.elapsed();
        let mut samples = Vec::with_capacity(WARM_ITERATIONS);
        for _ in 0..WARM_ITERATIONS {
            let started = Instant::now();
            service.search(request())?;
            samples.push(started.elapsed());
        }
        samples.sort();
        println!(
            "search {text:<14} {:>5} hits   first {:>7.2} ms   p50 {:>7.2} ms   p95 {:>7.2} ms",
            response.total,
            millis(first),
            millis(samples[samples.len() / 2]),
            millis(samples[(samples.len() * 95 / 100).min(samples.len() - 1)]),
        );
    }
    Ok(())
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
