use std::{
    collections::HashSet,
    env,
    error::Error,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use dla_application::recommendation::{
    CatalogRecommendationLaneKey, CatalogRecommendationReasonKind, CatalogRecommendationService,
    CatalogRecommendations,
};
use dla_sqlite::SqliteCatalogStore;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, types::Type};

const DEFAULT_WARM_ITERATIONS: usize = 20;
const MAXIMUM_WARM_ITERATIONS: usize = 200;
const MAXIMUM_LANE_ITEMS: usize = 12;
const TOP_ITEMS_TO_PRINT: usize = 5;

type GateResult<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScenarioKind {
    Game,
    Voice,
    Manga,
    SparseMetadata,
    LargeCircle,
}

impl ScenarioKind {
    fn label(self) -> &'static str {
        match self {
            Self::Game => "game",
            Self::Voice => "voice",
            Self::Manga => "manga",
            Self::SparseMetadata => "sparse_metadata",
            Self::LargeCircle => "large_circle",
        }
    }
}

#[derive(Clone, Debug)]
struct ScenarioAnchor {
    kind: ScenarioKind,
    work_code: String,
    title: String,
    release_date: String,
    tag_count: usize,
    circle_count: usize,
    category_count: usize,
    file_format_count: usize,
    language_count: usize,
    miscellany_count: usize,
    largest_circle_size: Option<usize>,
}

#[derive(Debug)]
struct ScenarioMeasurement {
    anchor: ScenarioAnchor,
    first_call: Duration,
    warm_p50: Duration,
    warm_p95: Duration,
    recommendations: CatalogRecommendations,
    warnings: Vec<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("recommendation quality gate failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> GateResult<()> {
    let (catalog_path, warm_iterations) = parse_arguments()?;
    let inspection = open_read_only(&catalog_path)?;
    let snapshot = read_snapshot(&inspection)?;
    let anchors = select_anchors(&inspection)?;
    drop(inspection);

    let store = Arc::new(SqliteCatalogStore::open_existing(&catalog_path)?);
    let service = CatalogRecommendationService::new(store);
    let mut measurements = Vec::with_capacity(anchors.len());
    for anchor in anchors {
        measurements.push(measure_scenario(&service, anchor, warm_iterations)?);
    }

    println!("recommendation real-catalog quality gate");
    println!("snapshot: {}", snapshot.0);
    println!("works: {}", snapshot.1);
    println!("warm iterations per scenario: {warm_iterations}");
    println!();

    let mut warning_count = 0;
    for measurement in &measurements {
        print_measurement(measurement);
        warning_count += measurement.warnings.len();
    }

    let first_max = measurements
        .iter()
        .map(|measurement| measurement.first_call)
        .max()
        .unwrap_or_default();
    let warm_p95_max = measurements
        .iter()
        .map(|measurement| measurement.warm_p95)
        .max()
        .unwrap_or_default();
    println!("summary");
    println!("  scenarios: {}", measurements.len());
    println!("  invariant failures: 0");
    println!("  relevance warnings: {warning_count}");
    println!("  slowest first call: {}", format_duration(first_max));
    println!("  slowest warm p95: {}", format_duration(warm_p95_max));

    Ok(())
}

fn parse_arguments() -> GateResult<(PathBuf, usize)> {
    let mut arguments = env::args().skip(1);
    let catalog_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("usage: recommendation_quality_gate <catalog.sqlite> [warm-iterations]")?;
    let warm_iterations = arguments
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(DEFAULT_WARM_ITERATIONS);
    if warm_iterations == 0 || warm_iterations > MAXIMUM_WARM_ITERATIONS {
        return Err(
            format!("warm iterations must be between 1 and {MAXIMUM_WARM_ITERATIONS}").into(),
        );
    }
    if arguments.next().is_some() {
        return Err("too many arguments".into());
    }
    Ok((catalog_path, warm_iterations))
}

fn open_read_only(path: &Path) -> GateResult<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn read_snapshot(connection: &Connection) -> GateResult<(String, usize)> {
    let (snapshot_id, work_count) = connection.query_row(
        "SELECT snapshot_id, real_work_count + synthetic_work_count
         FROM catalog_snapshot
         WHERE singleton = 1",
        [],
        |row| Ok((row.get(0)?, row.get::<_, i64>(1)?)),
    )?;
    Ok((snapshot_id, usize::try_from(work_count)?))
}

fn select_anchors(connection: &Connection) -> GateResult<Vec<ScenarioAnchor>> {
    let mut anchors = vec![
        select_anchor(
            connection,
            ScenarioKind::Game,
            "EXISTS (
             SELECT 1
             FROM catalog_work_category category
             WHERE category.work_code = work.work_code
               AND category.category_code IN ('role_playing', 'adventure', 'simulation', 'action')
         )
         AND EXISTS (
             SELECT 1
             FROM catalog_work_file_format file_format
             WHERE file_format.work_code = work.work_code
               AND file_format.file_format_code = 'application'
         )",
            "work.release_date DESC, tag_count DESC, work.work_code ASC",
        )?,
        select_anchor(
            connection,
            ScenarioKind::Voice,
            "EXISTS (
             SELECT 1
             FROM catalog_work_category category
             WHERE category.work_code = work.work_code
               AND category.category_code = 'voice_asmr'
         )",
            "work.release_date DESC, tag_count DESC, work.work_code ASC",
        )?,
        select_anchor(
            connection,
            ScenarioKind::Manga,
            "EXISTS (
             SELECT 1
             FROM catalog_work_category category
             WHERE category.work_code = work.work_code
               AND category.category_code IN ('manga', 'digital_comic')
         )",
            "work.release_date DESC, tag_count DESC, work.work_code ASC",
        )?,
        select_anchor(
            connection,
            ScenarioKind::SparseMetadata,
            "1 = 1",
            "facet_count ASC, work.release_date DESC, work.work_code ASC",
        )?,
    ];
    anchors.push(select_large_circle_anchor(connection, &anchors)?);
    Ok(anchors)
}

fn select_anchor(
    connection: &Connection,
    kind: ScenarioKind,
    predicate: &str,
    ordering: &str,
) -> GateResult<ScenarioAnchor> {
    let sql = format!(
        "SELECT work.work_code,
                work.title,
                work.release_date,
                (SELECT COUNT(*) FROM catalog_work_tag value WHERE value.work_code = work.work_code) AS tag_count,
                (SELECT COUNT(*) FROM catalog_work_circle value WHERE value.work_code = work.work_code) AS circle_count,
                (SELECT COUNT(*) FROM catalog_work_category value WHERE value.work_code = work.work_code) AS category_count,
                (SELECT COUNT(*) FROM catalog_work_file_format value WHERE value.work_code = work.work_code) AS file_format_count,
                (SELECT COUNT(*) FROM catalog_work_language value WHERE value.work_code = work.work_code) AS language_count,
                (SELECT COUNT(*) FROM catalog_work_miscellany value WHERE value.work_code = work.work_code) AS miscellany_count,
                (SELECT COUNT(*) FROM catalog_work_tag value WHERE value.work_code = work.work_code)
                  + (SELECT COUNT(*) FROM catalog_work_circle value WHERE value.work_code = work.work_code)
                  + (SELECT COUNT(*) FROM catalog_work_category value WHERE value.work_code = work.work_code)
                  + (SELECT COUNT(*) FROM catalog_work_file_format value WHERE value.work_code = work.work_code)
                  + (SELECT COUNT(*) FROM catalog_work_language value WHERE value.work_code = work.work_code)
                  + (SELECT COUNT(*) FROM catalog_work_miscellany value WHERE value.work_code = work.work_code) AS facet_count
         FROM catalog_work work
         WHERE work.is_synthetic = 0 AND {predicate}
         ORDER BY {ordering}
         LIMIT 1"
    );
    connection
        .query_row(&sql, [], |row| read_anchor_row(row, kind, None))
        .optional()?
        .ok_or_else(|| format!("no {} anchor exists in this catalog", kind.label()).into())
}

fn select_large_circle_anchor(
    connection: &Connection,
    existing: &[ScenarioAnchor],
) -> GateResult<ScenarioAnchor> {
    let largest_circle = connection.query_row(
        "SELECT circle_id, COUNT(*) AS work_count
         FROM catalog_work_circle
         GROUP BY circle_id
         ORDER BY work_count DESC, circle_id ASC
         LIMIT 1",
        [],
        |row| Ok((row.get::<_, i64>(0)?, read_count(row, 1)?)),
    )?;
    let excluded = existing
        .iter()
        .map(|anchor| anchor.work_code.as_str())
        .collect::<Vec<_>>();
    let sql =
        "SELECT work.work_code,
                work.title,
                work.release_date,
                (SELECT COUNT(*) FROM catalog_work_tag value WHERE value.work_code = work.work_code) AS tag_count,
                (SELECT COUNT(*) FROM catalog_work_circle value WHERE value.work_code = work.work_code) AS circle_count,
                (SELECT COUNT(*) FROM catalog_work_category value WHERE value.work_code = work.work_code) AS category_count,
                (SELECT COUNT(*) FROM catalog_work_file_format value WHERE value.work_code = work.work_code) AS file_format_count,
                (SELECT COUNT(*) FROM catalog_work_language value WHERE value.work_code = work.work_code) AS language_count,
                (SELECT COUNT(*) FROM catalog_work_miscellany value WHERE value.work_code = work.work_code) AS miscellany_count
         FROM catalog_work work
         JOIN catalog_work_circle work_circle ON work_circle.work_code = work.work_code
         WHERE work.is_synthetic = 0
           AND work_circle.circle_id = ?1
           AND work.work_code NOT IN (?2, ?3, ?4, ?5)
         ORDER BY tag_count DESC, work.release_date DESC, work.work_code ASC
         LIMIT 1";
    connection
        .query_row(
            sql,
            params![
                largest_circle.0,
                excluded[0],
                excluded[1],
                excluded[2],
                excluded[3]
            ],
            |row| read_anchor_row(row, ScenarioKind::LargeCircle, Some(largest_circle.1)),
        )
        .optional()?
        .ok_or_else(|| "no large-circle anchor exists in this catalog".into())
}

fn read_anchor_row(
    row: &rusqlite::Row<'_>,
    kind: ScenarioKind,
    largest_circle_size: Option<usize>,
) -> rusqlite::Result<ScenarioAnchor> {
    Ok(ScenarioAnchor {
        kind,
        work_code: row.get(0)?,
        title: row.get(1)?,
        release_date: row.get(2)?,
        tag_count: read_count(row, 3)?,
        circle_count: read_count(row, 4)?,
        category_count: read_count(row, 5)?,
        file_format_count: read_count(row, 6)?,
        language_count: read_count(row, 7)?,
        miscellany_count: read_count(row, 8)?,
        largest_circle_size,
    })
}

fn read_count(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<usize> {
    let value = row.get::<_, i64>(index)?;
    usize::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}

fn measure_scenario(
    service: &CatalogRecommendationService,
    anchor: ScenarioAnchor,
    warm_iterations: usize,
) -> GateResult<ScenarioMeasurement> {
    let started = Instant::now();
    let recommendations = service.read(&anchor.work_code)?;
    let first_call = started.elapsed();
    validate_recommendations(&recommendations)?;

    let mut warm_samples = Vec::with_capacity(warm_iterations);
    for _ in 0..warm_iterations {
        let started = Instant::now();
        let repeated = service.read(&anchor.work_code)?;
        warm_samples.push(started.elapsed());
        if repeated != recommendations {
            return Err(format!(
                "{} produced nondeterministic recommendations",
                anchor.work_code
            )
            .into());
        }
    }
    warm_samples.sort_unstable();
    let warm_p50 = percentile(&warm_samples, 50);
    let warm_p95 = percentile(&warm_samples, 95);
    let warnings = relevance_warnings(&anchor, &recommendations);
    Ok(ScenarioMeasurement {
        anchor,
        first_call,
        warm_p50,
        warm_p95,
        recommendations,
        warnings,
    })
}

fn validate_recommendations(recommendations: &CatalogRecommendations) -> GateResult<()> {
    let anchor = canonical(&recommendations.anchor_work_code);
    let mut seen = HashSet::new();
    for lane in &recommendations.lanes {
        if lane.items.len() > MAXIMUM_LANE_ITEMS {
            return Err(format!("{:?} exceeded its item limit", lane.key).into());
        }
        for item in &lane.items {
            let work_code = canonical(&item.work.code);
            if work_code == anchor {
                return Err("a recommendation lane contains its anchor".into());
            }
            if !seen.insert(work_code) {
                return Err("a work appears in more than one recommendation lane".into());
            }
            if item.reasons.is_empty() {
                return Err(format!("{} has no explanation", item.work.code).into());
            }
            if lane.key == CatalogRecommendationLaneKey::SameCircle
                && !item
                    .reasons
                    .iter()
                    .any(|reason| reason.kind == CatalogRecommendationReasonKind::SameCircle)
            {
                return Err(format!("{} lacks a same-circle reason", item.work.code).into());
            }
        }
    }
    Ok(())
}

fn relevance_warnings(
    anchor: &ScenarioAnchor,
    recommendations: &CatalogRecommendations,
) -> Vec<String> {
    let same_circle = recommendations
        .lanes
        .iter()
        .find(|lane| lane.key == CatalogRecommendationLaneKey::SameCircle);
    let similar = recommendations
        .lanes
        .iter()
        .find(|lane| lane.key == CatalogRecommendationLaneKey::Similar);
    let mut warnings = Vec::new();
    if anchor.kind == ScenarioKind::LargeCircle
        && same_circle.is_none_or(|lane| lane.items.is_empty())
    {
        warnings.push("the largest-circle anchor produced no same-circle lane".to_owned());
    }
    if matches!(
        anchor.kind,
        ScenarioKind::Game | ScenarioKind::Voice | ScenarioKind::Manga
    ) && similar.is_none_or(|lane| lane.items.is_empty())
    {
        warnings.push("the representative anchor produced no similar-work lane".to_owned());
    }
    if let Some(lane) = similar {
        let weak_only = lane
            .items
            .iter()
            .take(TOP_ITEMS_TO_PRINT)
            .filter(|item| {
                item.reasons.iter().all(|reason| {
                    matches!(
                        reason.kind,
                        CatalogRecommendationReasonKind::SharedFileFormat
                            | CatalogRecommendationReasonKind::SharedLanguage
                    )
                })
            })
            .count();
        if weak_only > 0 {
            warnings.push(format!(
                "{weak_only} top similar works rely only on file-format or language evidence"
            ));
        }
    }
    warnings
}

fn percentile(sorted: &[Duration], percent: usize) -> Duration {
    let index = (sorted.len() - 1).saturating_mul(percent) / 100;
    sorted[index]
}

fn print_measurement(measurement: &ScenarioMeasurement) {
    let anchor = &measurement.anchor;
    println!("scenario: {}", anchor.kind.label());
    println!(
        "  anchor: {} | {} | released {}",
        anchor.work_code, anchor.title, anchor.release_date
    );
    println!(
        "  facets: tags={} circles={} categories={} formats={} languages={} miscellany={}",
        anchor.tag_count,
        anchor.circle_count,
        anchor.category_count,
        anchor.file_format_count,
        anchor.language_count,
        anchor.miscellany_count
    );
    if let Some(circle_size) = anchor.largest_circle_size {
        println!("  largest circle size: {circle_size}");
    }
    println!(
        "  latency: first={} warm_p50={} warm_p95={}",
        format_duration(measurement.first_call),
        format_duration(measurement.warm_p50),
        format_duration(measurement.warm_p95)
    );
    for lane in &measurement.recommendations.lanes {
        println!("  lane {:?}: {} items", lane.key, lane.items.len());
        for item in lane.items.iter().take(TOP_ITEMS_TO_PRINT) {
            let reasons = item
                .reasons
                .iter()
                .map(|reason| format!("{:?}:{}", reason.kind, reason.label))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "    {} score={} | {} | {}",
                item.work.code, item.score, item.work.title, reasons
            );
        }
    }
    if measurement.recommendations.lanes.is_empty() {
        println!("  lanes: none");
    }
    for warning in &measurement.warnings {
        println!("  warning: {warning}");
    }
    println!();
}

fn format_duration(duration: Duration) -> String {
    format!("{:.2} ms", duration.as_secs_f64() * 1_000.0)
}

fn canonical(value: &str) -> String {
    value.trim().to_lowercase()
}
