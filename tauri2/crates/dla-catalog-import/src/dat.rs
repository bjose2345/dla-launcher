use std::{io::BufRead, path::Path};

use dla_application::catalog_import::{
    CatalogImportCancellationToken, CatalogImportError, CatalogPackageManifest,
};
use dla_domain::{
    CatalogRom, CatalogRomEntry, CatalogWork, CatalogWorkDetail, Category, NamedReference,
};
use dla_sqlite::SqliteCatalogImportWriter;
use quick_xml::{
    Reader, XmlVersion,
    events::{BytesStart, Event},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DatImportStats {
    pub work_entries: u64,
    pub unique_works: u64,
    pub roms: u64,
    pub files: u64,
}

#[derive(Default)]
struct WorkBuilder {
    release_key: String,
    code: String,
    title: String,
    site: String,
    circle: String,
    release_date: String,
    version: String,
    categories: String,
    tags: String,
    file_types: String,
    miscellanies: String,
    languages: String,
    drm: String,
    persisted: bool,
}

#[derive(Clone, Copy)]
enum TextField {
    Title,
    Site,
    Circle,
    ReleaseDate,
    Version,
    Categories,
    Tags,
    FileTypes,
    Miscellanies,
    Languages,
    Drm,
}

pub fn import_dat<R: BufRead>(
    input: R,
    manifest: &CatalogPackageManifest,
    writer: &mut SqliteCatalogImportWriter,
    cancellation: &CatalogImportCancellationToken,
    mut on_progress: impl FnMut(DatImportStats) -> Result<(), CatalogImportError>,
) -> Result<DatImportStats, CatalogImportError> {
    let mut reader = Reader::from_reader(input);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut work: Option<WorkBuilder> = None;
    let mut current_rom: Option<(String, usize)> = None;
    let mut text_field: Option<(TextField, String)> = None;
    let mut stats = DatImportStats::default();
    let includes_contents = manifest
        .fields
        .iter()
        .any(|field| field == "rom.contents.path");

    loop {
        if cancellation.is_cancelled() {
            return Err(CatalogImportError::Cancelled);
        }
        match reader
            .read_event_into(&mut buffer)
            .map_err(CatalogImportError::invalid)?
        {
            Event::Start(element) => match element.name().as_ref() {
                b"work" => {
                    if work.is_some() {
                        return Err(CatalogImportError::invalid("nested DAT work element"));
                    }
                    let release_key = attribute(&reader, &element, b"name")?
                        .ok_or_else(|| CatalogImportError::invalid("DAT work is missing name"))?;
                    let code = base_work_code(&release_key);
                    work = Some(WorkBuilder {
                        release_key,
                        code,
                        ..WorkBuilder::default()
                    });
                }
                b"rom" => {
                    let current = work
                        .as_mut()
                        .ok_or_else(|| CatalogImportError::invalid("ROM appears outside a work"))?;
                    persist_work(current, manifest, writer, &mut stats)?;
                    let rom = parse_rom(&reader, &element, current, includes_contents)?;
                    let position = writer
                        .insert_rom(&current.code, &rom)
                        .map_err(CatalogImportError::persistence)?;
                    if includes_contents {
                        writer
                            .initialize_rom_contents(&current.code, position)
                            .map_err(CatalogImportError::persistence)?;
                    }
                    current_rom = Some((current.code.clone(), position));
                    stats.roms += 1;
                }
                b"file" => {
                    import_file(&reader, &element, current_rom.as_ref(), writer, &mut stats)?;
                }
                name => {
                    if work.is_some()
                        && let Some(field) = text_field_for(name)
                    {
                        text_field = Some((field, String::new()));
                    }
                }
            },
            Event::Empty(element) => match element.name().as_ref() {
                b"rom" => {
                    let current = work
                        .as_mut()
                        .ok_or_else(|| CatalogImportError::invalid("ROM appears outside a work"))?;
                    persist_work(current, manifest, writer, &mut stats)?;
                    let rom = parse_rom(&reader, &element, current, includes_contents)?;
                    let position = writer
                        .insert_rom(&current.code, &rom)
                        .map_err(CatalogImportError::persistence)?;
                    if includes_contents {
                        writer
                            .initialize_rom_contents(&current.code, position)
                            .map_err(CatalogImportError::persistence)?;
                    }
                    stats.roms += 1;
                }
                b"file" => {
                    import_file(&reader, &element, current_rom.as_ref(), writer, &mut stats)?;
                }
                _ => {}
            },
            Event::Text(text) => {
                if let Some((_, value)) = &mut text_field {
                    let decoded = text.decode().map_err(CatalogImportError::invalid)?;
                    let unescaped = quick_xml::escape::unescape(&decoded)
                        .map_err(CatalogImportError::invalid)?;
                    value.push_str(&unescaped);
                }
            }
            Event::CData(text) => {
                if let Some((_, value)) = &mut text_field {
                    value.push_str(&text.decode().map_err(CatalogImportError::invalid)?);
                }
            }
            Event::GeneralRef(reference) => {
                if let Some((_, value)) = &mut text_field {
                    if let Some(character) = reference
                        .resolve_char_ref()
                        .map_err(CatalogImportError::invalid)?
                    {
                        value.push(character);
                    } else {
                        let decoded = reference.decode().map_err(CatalogImportError::invalid)?;
                        value.push_str(match decoded.as_ref() {
                            "amp" => "&",
                            "lt" => "<",
                            "gt" => ">",
                            "apos" => "'",
                            "quot" => "\"",
                            name => {
                                return Err(CatalogImportError::invalid(format!(
                                    "DAT contains unsupported entity &{name};"
                                )));
                            }
                        });
                    }
                }
            }
            Event::End(element) => match element.name().as_ref() {
                b"work" => {
                    let mut completed = work
                        .take()
                        .ok_or_else(|| CatalogImportError::invalid("unexpected DAT work end"))?;
                    persist_work(&mut completed, manifest, writer, &mut stats)?;
                    stats.work_entries += 1;
                    if stats.work_entries.is_multiple_of(128) {
                        on_progress(stats)?;
                    }
                }
                b"rom" => current_rom = None,
                name => {
                    if let Some((field, value)) = text_field.take() {
                        if text_field_for(name).is_some() {
                            if let Some(current) = &mut work {
                                assign_text(current, field, value);
                            }
                        } else {
                            text_field = Some((field, value));
                        }
                    }
                }
            },
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if work.is_some() || current_rom.is_some() {
        return Err(CatalogImportError::invalid(
            "DAT ended before the current work or ROM was closed",
        ));
    }
    if stats.work_entries != manifest.counts.work_entries {
        return Err(CatalogImportError::invalid(format!(
            "manifest declares {} DAT work entries, parsed {}",
            manifest.counts.work_entries, stats.work_entries
        )));
    }
    on_progress(stats)?;
    Ok(stats)
}

fn persist_work(
    work: &mut WorkBuilder,
    manifest: &CatalogPackageManifest,
    writer: &mut SqliteCatalogImportWriter,
    stats: &mut DatImportStats,
) -> Result<(), CatalogImportError> {
    if work.persisted {
        return Ok(());
    }
    if work.code.is_empty() || work.title.trim().is_empty() {
        return Err(CatalogImportError::invalid(format!(
            "DAT work {} is missing a code or title",
            work.release_key
        )));
    }
    let detail = CatalogWorkDetail {
        work: CatalogWork {
            code: work.code.clone(),
            source_code: manifest.source.id.clone(),
            title: work.title.clone(),
            title_english: String::new(),
            added_date: String::new(),
            release_date: work.release_date.clone(),
            updated_date: update_date_from_release_key(&work.release_key),
            age_rating: String::new(),
            release_type: String::new(),
            main_image_urls: Vec::new(),
            thumbnail_urls: Vec::new(),
            circles: named_values(&work.circle),
            categories: category_values(&work.categories),
            tags: named_values(&work.tags),
            synthetic: false,
        },
        sample_image_urls: Vec::new(),
        file_formats: category_values(&work.file_types),
        supported_languages: category_values(&work.languages),
        miscellanies: category_values(&work.miscellanies),
        roms: Vec::new(),
        related_works: Vec::new(),
        rating: None,
        descriptions: Default::default(),
    };
    if writer
        .ensure_work(detail)
        .map_err(CatalogImportError::persistence)?
    {
        stats.unique_works += 1;
        writer
            .apply_dat_metadata(&work.code, &work.site, &string_values(&work.drm, true))
            .map_err(CatalogImportError::persistence)?;
    }
    work.persisted = true;
    Ok(())
}

fn parse_rom<B: BufRead>(
    reader: &Reader<B>,
    element: &BytesStart<'_>,
    work: &WorkBuilder,
    includes_contents: bool,
) -> Result<CatalogRom, CatalogImportError> {
    Ok(CatalogRom {
        name: required_attribute(reader, element, b"name", "ROM name")?,
        size: required_attribute(reader, element, b"size", "ROM size")?,
        crc: attribute(reader, element, b"crc")?.unwrap_or_default(),
        md5: attribute(reader, element, b"md5")?.unwrap_or_default(),
        sha1: attribute(reader, element, b"sha1")?.unwrap_or_default(),
        sha256: attribute(reader, element, b"sha256")?.unwrap_or_default(),
        file_count: includes_contents.then_some(0),
        update_date: update_date_from_release_key(&work.release_key),
        version: work.version.clone(),
    })
}

fn import_file<B: BufRead>(
    reader: &Reader<B>,
    element: &BytesStart<'_>,
    current_rom: Option<&(String, usize)>,
    writer: &mut SqliteCatalogImportWriter,
    stats: &mut DatImportStats,
) -> Result<(), CatalogImportError> {
    let (work_code, rom_position) =
        current_rom.ok_or_else(|| CatalogImportError::invalid("file appears outside a ROM"))?;
    let path = required_attribute(reader, element, b"name", "file name")?;
    let entry = CatalogRomEntry {
        entry_index: stats.files,
        extension: Path::new(&path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase(),
        path,
        is_directory: false,
        size: Some(required_attribute(reader, element, b"size", "file size")?),
        crc32: attribute(reader, element, b"crc")?.unwrap_or_default(),
        md5: attribute(reader, element, b"md5")?.unwrap_or_default(),
        sha1: attribute(reader, element, b"sha1")?.unwrap_or_default(),
        sha256: attribute(reader, element, b"sha256")?.unwrap_or_default(),
        hash_status: "complete".to_owned(),
    };
    writer
        .insert_rom_file(work_code, *rom_position, &entry)
        .map_err(CatalogImportError::persistence)?;
    stats.files += 1;
    Ok(())
}

fn attribute<B: BufRead>(
    reader: &Reader<B>,
    element: &BytesStart<'_>,
    name: &[u8],
) -> Result<Option<String>, CatalogImportError> {
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(CatalogImportError::invalid)?;
        if attribute.key.as_ref() == name {
            return attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                .map(|value| Some(value.into_owned()))
                .map_err(CatalogImportError::invalid);
        }
    }
    Ok(None)
}

fn required_attribute<B: BufRead>(
    reader: &Reader<B>,
    element: &BytesStart<'_>,
    name: &[u8],
    label: &str,
) -> Result<String, CatalogImportError> {
    attribute(reader, element, name)?
        .ok_or_else(|| CatalogImportError::invalid(format!("DAT {label} is missing")))
}

fn text_field_for(name: &[u8]) -> Option<TextField> {
    match name {
        b"title" => Some(TextField::Title),
        b"site" => Some(TextField::Site),
        b"circle" => Some(TextField::Circle),
        b"release_date" => Some(TextField::ReleaseDate),
        b"version" => Some(TextField::Version),
        b"categories" => Some(TextField::Categories),
        b"tags" => Some(TextField::Tags),
        b"filetypes" => Some(TextField::FileTypes),
        b"miscellanies" => Some(TextField::Miscellanies),
        b"languages" => Some(TextField::Languages),
        b"drm" => Some(TextField::Drm),
        _ => None,
    }
}

fn assign_text(work: &mut WorkBuilder, field: TextField, value: String) {
    match field {
        TextField::Title => work.title = value,
        TextField::Site => work.site = value,
        TextField::Circle => work.circle = value,
        TextField::ReleaseDate => work.release_date = value,
        TextField::Version => work.version = value,
        TextField::Categories => work.categories = value,
        TextField::Tags => work.tags = value,
        TextField::FileTypes => work.file_types = value,
        TextField::Miscellanies => work.miscellanies = value,
        TextField::Languages => work.languages = value,
        TextField::Drm => work.drm = value,
    }
}

fn base_work_code(release_key: &str) -> String {
    let upper = release_key.trim().to_ascii_uppercase();
    for prefix in ["RJ", "BJ", "VJ"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            let digits = rest
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>();
            if !digits.is_empty() {
                return format!("{prefix}{digits}");
            }
        }
    }
    release_key
        .split_once('-')
        .map_or(release_key, |(base, _)| base)
        .trim()
        .to_owned()
}

fn update_date_from_release_key(release_key: &str) -> String {
    let bytes = release_key.as_bytes();
    if bytes.len() < 10 {
        return String::new();
    }
    for index in 0..=bytes.len() - 10 {
        let value = &bytes[index..index + 10];
        if value.len() == 10
            && value[4] == b'-'
            && value[7] == b'-'
            && value
                .iter()
                .enumerate()
                .all(|(position, byte)| position == 4 || position == 7 || byte.is_ascii_digit())
            && let Ok(candidate) = std::str::from_utf8(value)
        {
            return candidate.to_owned();
        }
    }
    String::new()
}

fn named_values(value: &str) -> Vec<NamedReference> {
    string_values(value, false)
        .into_iter()
        .map(|name| NamedReference {
            name,
            name_english: String::new(),
        })
        .collect()
}

fn category_values(value: &str) -> Vec<Category> {
    string_values(value, false)
        .into_iter()
        .map(|name| Category {
            code: stable_value_code(&name),
            name,
            name_english: String::new(),
        })
        .collect()
}

fn string_values(value: &str, omit_none: bool) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter(|value| !omit_none || !value.eq_ignore_ascii_case("none"))
        .map(str::to_owned)
        .collect()
}

fn stable_value_code(value: &str) -> String {
    let normalized = value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    let compact = normalized
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    if compact.is_empty() {
        value.to_owned()
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_base_codes_and_update_dates() {
        assert_eq!(base_work_code("RJ01326398-2026-07-30"), "RJ01326398");
        assert_eq!(base_work_code("BJ009272"), "BJ009272");
        assert_eq!(
            update_date_from_release_key("RJ01326398-2026-07-30-dup2"),
            "2026-07-30"
        );
    }
}
