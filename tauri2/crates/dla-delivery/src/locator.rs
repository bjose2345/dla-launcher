use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

use dla_application::delivery::{DeliveryControl, DeliveryError, DeliveryLocator, ReleaseLookup};
use dla_domain::delivery::DeliveryProblem;
use serde::{Deserialize, Serialize};

use crate::{DeliveryClient, client::Entry};

/// The archive root in every subscriber's view of SFTPGo.
pub const ARCHIVE_ROOT: &str = "/dlsite";
pub const MANIFEST_NAME: &str = "_manifest.json";
const MAX_MANIFEST_BYTES: u64 = 32 << 20;

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    files: BTreeMap<String, ManifestFile>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ManifestFile {
    size: Option<u64>,
    hashes: Option<Hashes>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Hashes {
    sha256: Option<String>,
}

/// One month's archives by SHA-256, remembered with the manifest's size and
/// date so an unchanged manifest is not read again.
#[derive(Clone, Default, Deserialize, Serialize)]
struct Month {
    size: u64,
    modified: String,
    files: BTreeMap<String, (String, u64)>,
}

/// Finds archives by SHA-256 in the archive's per-month `_manifest.json`
/// files: first in the months of the work's release and update, then in
/// every month, newest first.
pub struct ManifestLocator {
    client: Arc<DeliveryClient>,
    cache: Option<PathBuf>,
}

impl ManifestLocator {
    pub fn new(client: Arc<DeliveryClient>, cache: Option<PathBuf>) -> Self {
        Self { client, cache }
    }

    fn load_cache(&self) -> BTreeMap<String, Month> {
        self.cache
            .as_ref()
            .and_then(|path| fs::read(path).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    fn save_cache(&self, months: &BTreeMap<String, Month>) {
        let Some(path) = &self.cache else {
            return;
        };
        if let (Some(parent), Ok(data)) = (path.parent(), serde_json::to_vec(months)) {
            let temporary = path.with_extension("json.new");
            if fs::create_dir_all(parent).is_ok() && fs::write(&temporary, data).is_ok() {
                fs::rename(&temporary, path).ok();
            }
        }
    }

    /// One month's archives, from the cache while its manifest is unchanged.
    fn month(
        &self,
        folder: &str,
        cache: &mut BTreeMap<String, Month>,
    ) -> Result<Option<Month>, DeliveryError> {
        let entries = match self.client.list(&format!("{ARCHIVE_ROOT}/{folder}")) {
            Ok(entries) => entries,
            Err(DeliveryError::Problem {
                problem: DeliveryProblem::NotInArchive,
                ..
            }) => return Ok(None),
            Err(error) => return Err(error),
        };
        let Some(manifest) = entries
            .iter()
            .find(|entry| !entry.directory && entry.name == MANIFEST_NAME)
        else {
            return Ok(None);
        };
        if let Some(known) = cache.get(folder)
            && known.size == manifest.size
            && known.modified == manifest.modified
        {
            return Ok(Some(known.clone()));
        }
        let data = self.client.read_small(
            &format!("{ARCHIVE_ROOT}/{folder}/{MANIFEST_NAME}"),
            MAX_MANIFEST_BYTES,
        )?;
        let Ok(parsed) = serde_json::from_slice::<Manifest>(&data) else {
            return Ok(None);
        };
        let month = Month {
            size: manifest.size,
            modified: manifest.modified.clone(),
            files: parsed
                .files
                .into_iter()
                .filter_map(|(name, file)| {
                    let sha256 = file.hashes?.sha256?.trim().to_ascii_lowercase();
                    Some((sha256, (name, file.size.unwrap_or(0))))
                })
                .collect(),
        };
        cache.insert(folder.to_owned(), month.clone());
        Ok(Some(month))
    }
}

/// `YYYY-MM` of a `YYYY-MM-DD…` date, if it has that shape.
fn month_of(date: &str) -> Option<String> {
    let month = date.trim().get(..7)?;
    let valid = month.bytes().enumerate().all(|(index, byte)| match index {
        4 => byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    valid.then(|| month.to_owned())
}

impl DeliveryLocator for ManifestLocator {
    fn locate(
        &self,
        lookup: &ReleaseLookup<'_>,
        control: &dyn DeliveryControl,
    ) -> Result<Vec<String>, DeliveryError> {
        let mut found: Vec<Option<String>> = vec![None; lookup.archives.len()];
        let mut cache = self.load_cache();
        let mut searched: Vec<String> = Vec::new();
        let mut search = |folder: &str,
                          found: &mut Vec<Option<String>>,
                          cache: &mut BTreeMap<String, Month>|
         -> Result<(), DeliveryError> {
            if searched.iter().any(|done| done == folder) {
                return Ok(());
            }
            searched.push(folder.to_owned());
            control.check()?;
            if let Some(month) = self.month(folder, cache)? {
                for (slot, archive) in found.iter_mut().zip(lookup.archives) {
                    if let Some((name, size)) = month.files.get(&archive.sha256)
                        && (*size == 0 || *size == archive.size)
                    {
                        slot.get_or_insert_with(|| format!("{ARCHIVE_ROOT}/{folder}/{name}"));
                    }
                }
            }
            Ok(())
        };
        let hinted: Vec<String> = [lookup.update_date, lookup.release_date]
            .into_iter()
            .filter_map(month_of)
            .collect();
        for folder in &hinted {
            search(folder, &mut found, &mut cache)?;
        }
        if found.iter().any(Option::is_none) {
            let mut folders: Vec<Entry> = self
                .client
                .list(ARCHIVE_ROOT)?
                .into_iter()
                .filter(|entry| entry.directory)
                .collect();
            folders.sort_by(|left, right| right.name.cmp(&left.name));
            for folder in folders {
                search(&folder.name, &mut found, &mut cache)?;
                if found.iter().all(Option::is_some) {
                    break;
                }
            }
        }
        self.save_cache(&cache);
        found
            .into_iter()
            .zip(lookup.archives)
            .map(|(path, archive)| {
                path.ok_or_else(|| {
                    DeliveryError::problem(
                        DeliveryProblem::NotInArchive,
                        format!("{} is not in the DLA archive", archive.file_name),
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_come_from_dates() {
        assert_eq!(month_of("2025-11-07").as_deref(), Some("2025-11"));
        assert_eq!(month_of("2025-11").as_deref(), Some("2025-11"));
        assert_eq!(month_of(""), None);
        assert_eq!(month_of("11/07/2025"), None);
    }
}
