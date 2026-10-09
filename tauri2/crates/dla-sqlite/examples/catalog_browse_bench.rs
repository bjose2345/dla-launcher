//! Times the catalog read paths behind the browse route against an imported
//! catalog: month context, a Grid page, Line day slices read one at a time and
//! concurrently, one work detail, and a 24-work hydration.
//!
//! ```bash
//! cargo run --release -p dla-sqlite --example catalog_browse_bench -- <catalog.sqlite> [iterations]
//! ```

use std::{
    env,
    error::Error,
    path::Path,
    process::ExitCode,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use dla_application::{
    catalog::{
        CatalogContextQuery, CatalogDay, CatalogError, CatalogFacetFilters, CatalogFacetSelection,
        CatalogMonth, CatalogQuery, CatalogReader, CatalogTimeline,
    },
    search::CatalogIndexSource,
};
use dla_sqlite::SqliteCatalogStore;

const DEFAULT_ITERATIONS: usize = 20;
const CONCURRENT_DAYS: usize = 8;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("catalog browse bench failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: catalog_browse_bench <catalog.sqlite> [iterations]")?;
    let iterations = env::args()
        .nth(2)
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(DEFAULT_ITERATIONS)
        .clamp(1, 500);
    let store = Arc::new(SqliteCatalogStore::open_existing(Path::new(&path))?);
    let snapshot = CatalogIndexSource::snapshot(&*store)?;
    println!(
        "catalog {} ({} real works), {iterations} warm iterations",
        snapshot.id, snapshot.real_works
    );

    let context_query = CatalogContextQuery {
        facets: no_filters(),
        timeline: CatalogTimeline::Activity,
    };
    let context = store.context(&context_query)?;
    let month = CatalogMonth::parse(&context.default_month).ok_or("default month")?;
    // A fresh store per call bypasses the per-generation context cache.
    measure("context, no filters, cold", iterations, || {
        SqliteCatalogStore::open_existing(Path::new(&path))?
            .context(&context_query)
            .map(|_| ())
    })?;
    measure("context, no filters, cached", iterations, || {
        store.context(&context_query).map(|_| ())
    })?;

    let mut one_tag = no_filters();
    if let Some(tag) = context.facets.genres.first() {
        one_tag.genres.include.push(tag.key.clone());
    }
    let tag_query = CatalogContextQuery {
        facets: one_tag,
        timeline: CatalogTimeline::Activity,
    };
    measure("context, one tag, cold", iterations, || {
        SqliteCatalogStore::open_existing(Path::new(&path))?
            .context(&tag_query)
            .map(|_| ())
    })?;
    measure("context, one tag, cached", iterations, || {
        store.context(&tag_query).map(|_| ())
    })?;

    let page_query = CatalogQuery {
        search: String::new(),
        facets: no_filters(),
        sort: "release_desc".to_owned(),
        timeline: CatalogTimeline::Activity,
        month: Some(month.clone()),
        day: None,
        limit: 24,
        offset: 0,
    };
    let page = store.browse(&page_query)?;
    measure("grid page 1", iterations, || {
        store.browse(&page_query).map(|_| ())
    })?;

    let days = page
        .day_buckets
        .iter()
        .filter(|bucket| bucket.count > 0)
        .take(CONCURRENT_DAYS)
        .map(|bucket| CatalogDay::parse(&bucket.day, &month).ok_or("day"))
        .collect::<Result<Vec<_>, _>>()?;
    let day_queries = days
        .iter()
        .map(|day| CatalogQuery {
            day: Some(day.clone()),
            limit: 12,
            ..page_query.clone()
        })
        .collect::<Vec<_>>();
    measure(
        &format!("{} line days, sequential", day_queries.len()),
        iterations,
        || {
            day_queries
                .iter()
                .try_for_each(|query| store.browse(query).map(|_| ()))
        },
    )?;
    measure(
        &format!("{} line days, concurrent", day_queries.len()),
        iterations,
        || {
            let handles = day_queries
                .iter()
                .cloned()
                .map(|query| {
                    let store = Arc::clone(&store);
                    thread::spawn(move || store.browse(&query).map(|_| ()))
                })
                .collect::<Vec<_>>();
            for handle in handles {
                handle
                    .join()
                    .map_err(|_| CatalogError::persistence("day thread panicked"))??;
            }
            Ok::<(), CatalogError>(())
        },
    )?;

    let code = page
        .items
        .first()
        .map(|work| work.code.clone())
        .ok_or("empty page")?;
    measure("work detail", iterations, || store.read(&code).map(|_| ()))?;
    let codes = page
        .items
        .iter()
        .map(|work| work.code.clone())
        .collect::<Vec<_>>();
    measure("read_works(24)", iterations, || {
        store.read_works(&codes).map(|_| ())
    })?;
    Ok(())
}

fn no_filters() -> CatalogFacetFilters {
    CatalogFacetFilters {
        ages: CatalogFacetSelection::default(),
        languages: CatalogFacetSelection::default(),
        categories: CatalogFacetSelection::default(),
        genres: CatalogFacetSelection::default(),
        file_types: CatalogFacetSelection::default(),
        miscellanies: CatalogFacetSelection::default(),
        circles: CatalogFacetSelection::default(),
    }
}

fn measure<E: Error + 'static>(
    label: &str,
    iterations: usize,
    mut operation: impl FnMut() -> Result<(), E>,
) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    operation()?;
    let first = started.elapsed();
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        operation()?;
        samples.push(started.elapsed());
    }
    samples.sort();
    println!(
        "{label:<28} first {:>8.2} ms   p50 {:>8.2} ms   p95 {:>8.2} ms",
        millis(first),
        millis(samples[samples.len() / 2]),
        millis(samples[(samples.len() * 95 / 100).min(samples.len() - 1)]),
    );
    Ok(())
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}
