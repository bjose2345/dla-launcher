use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::UNIX_EPOCH,
};

use dla_application::media::{AudioWaveform, AudioWaveformReader, MediaError};
use dla_domain::installation::RelativePath;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use symphonia::core::{
    audio::sample::Sample,
    codecs::audio::AudioDecoderOptions,
    errors::Error as SymphoniaError,
    formats::{FormatOptions, TrackType, probe::Hint},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

const CACHE_VERSION: u32 = 1;
const ANALYSIS_WINDOW_FRAMES: usize = 1_024;
static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct DesktopAudioWaveformReader {
    cache_directory: PathBuf,
}

impl DesktopAudioWaveformReader {
    pub fn new(cache_directory: PathBuf) -> Self {
        Self { cache_directory }
    }
}

impl AudioWaveformReader for DesktopAudioWaveformReader {
    fn read_waveform(
        &self,
        root_path: &str,
        relative_path: &RelativePath,
        bucket_count: u32,
    ) -> Result<AudioWaveform, MediaError> {
        let path = resolve_media_path(root_path, relative_path)?;
        let metadata = path.metadata().map_err(MediaError::waveform)?;
        let fingerprint = SourceFingerprint {
            size_bytes: metadata.len(),
            modified_ns: metadata.modified().ok().and_then(|modified| {
                modified
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|duration| duration.as_nanos())
            }),
        };
        let cache_path =
            self.cache_directory
                .join(cache_key(root_path, relative_path.as_str(), bucket_count));
        if let Some(waveform) = read_cached(&cache_path, &fingerprint, bucket_count) {
            return Ok(waveform);
        }

        let waveform = decode_waveform(&path, bucket_count)?;
        let cached = CachedWaveform {
            version: CACHE_VERSION,
            bucket_count,
            fingerprint,
            waveform: waveform.clone(),
        };
        if let Err(error) = write_cached(&self.cache_directory, &cache_path, &cached) {
            log::warn!("could not cache audio waveform: {error}");
        }
        Ok(waveform)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceFingerprint {
    size_bytes: u64,
    modified_ns: Option<u128>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct CachedWaveform {
    version: u32,
    bucket_count: u32,
    fingerprint: SourceFingerprint,
    waveform: AudioWaveform,
}

fn resolve_media_path(
    root_path: &str,
    relative_path: &RelativePath,
) -> Result<PathBuf, MediaError> {
    let root = Path::new(root_path)
        .canonicalize()
        .map_err(MediaError::waveform)?;
    let attempted = root.join(relative_path.as_str());
    let target = attempted
        .canonicalize()
        .inspect_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                log::warn!(
                    "media asset is indexed but missing on disk: {}",
                    attempted.display()
                );
            }
        })
        .map_err(MediaError::waveform)?;
    if !target.starts_with(&root) || !target.is_file() {
        return Err(MediaError::waveform(
            "waveform source is outside the installation root",
        ));
    }
    Ok(target)
}

fn cache_key(root_path: &str, relative_path: &str, bucket_count: u32) -> String {
    let mut digest = Sha256::new();
    digest.update(CACHE_VERSION.to_le_bytes());
    digest.update(root_path.as_bytes());
    digest.update([0]);
    digest.update(relative_path.as_bytes());
    digest.update(bucket_count.to_le_bytes());
    format!("{}.json", hex::encode(digest.finalize()))
}

fn read_cached(
    path: &Path,
    fingerprint: &SourceFingerprint,
    bucket_count: u32,
) -> Option<AudioWaveform> {
    let cached = serde_json::from_reader::<_, CachedWaveform>(File::open(path).ok()?).ok()?;
    (cached.version == CACHE_VERSION
        && cached.bucket_count == bucket_count
        && cached.fingerprint == *fingerprint
        && cached.waveform.peaks.len() == bucket_count as usize)
        .then_some(cached.waveform)
}

fn write_cached(
    directory: &Path,
    path: &Path,
    cached: &CachedWaveform,
) -> Result<(), std::io::Error> {
    fs::create_dir_all(directory)?;
    let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}-{sequence}.tmp", std::process::id()));
    let file = File::create(&temporary)?;
    serde_json::to_writer(file, cached).map_err(std::io::Error::other)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}

fn decode_waveform(path: &Path, bucket_count: u32) -> Result<AudioWaveform, MediaError> {
    let file = Box::new(File::open(path).map_err(MediaError::waveform)?);
    let stream = MediaSourceStream::new(file, Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(MediaError::waveform)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| MediaError::waveform("the file contains no decodable audio track"))?;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| MediaError::waveform("the audio track has no codec parameters"))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())
        .map_err(MediaError::waveform)?;
    let track_id = track.id;
    let mut samples = Vec::<f32>::new();
    let mut windows = Vec::<f64>::new();
    let mut window_power = 0.0_f64;
    let mut window_frames = 0_usize;
    let mut total_frames = 0_u64;
    let mut sample_rate = None;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(MediaError::waveform(error)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(MediaError::waveform(error)),
        };
        sample_rate.get_or_insert(audio.spec().rate());
        let channels = audio.spec().channels().count();
        if channels == 0 {
            continue;
        }
        samples.resize(audio.samples_interleaved(), f32::MID);
        audio.copy_to_slice_interleaved(&mut samples);
        for frame in samples.chunks_exact(channels) {
            let power = frame
                .iter()
                .map(|sample| f64::from(*sample) * f64::from(*sample))
                .sum::<f64>()
                / channels as f64;
            window_power += power;
            window_frames += 1;
            total_frames += 1;
            if window_frames == ANALYSIS_WINDOW_FRAMES {
                windows.push((window_power / window_frames as f64).sqrt());
                window_power = 0.0;
                window_frames = 0;
            }
        }
    }
    if window_frames > 0 {
        windows.push((window_power / window_frames as f64).sqrt());
    }
    let sample_rate = sample_rate
        .filter(|rate| *rate > 0)
        .ok_or_else(|| MediaError::waveform("the audio decoder produced no samples"))?;
    let duration_ms = total_frames
        .saturating_mul(1_000)
        .checked_div(u64::from(sample_rate))
        .unwrap_or(0);
    Ok(AudioWaveform {
        peaks: reduce_windows(&windows, bucket_count as usize),
        duration_ms,
    })
}

fn reduce_windows(windows: &[f64], bucket_count: usize) -> Vec<f32> {
    if windows.is_empty() {
        return vec![0.0; bucket_count];
    }
    let maximum = windows.iter().copied().fold(0.0_f64, f64::max);
    (0..bucket_count)
        .map(|bucket| {
            let start = bucket * windows.len() / bucket_count;
            let end = (((bucket + 1) * windows.len()).div_ceil(bucket_count))
                .max(start + 1)
                .min(windows.len());
            let value = windows[start.min(windows.len() - 1)..end]
                .iter()
                .copied()
                .fold(0.0_f64, f64::max);
            if maximum <= f64::EPSILON {
                0.0
            } else {
                (value / maximum).powf(0.65).clamp(0.0, 1.0) as f32
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn decodes_and_caches_a_pcm_waveform() {
        let directory = tempdir().expect("temporary directory");
        let media_root = directory.path().join("media");
        let cache_root = directory.path().join("cache");
        fs::create_dir_all(&media_root).expect("media root");
        fs::write(media_root.join("tone.wav"), wave_fixture()).expect("wave fixture");
        let reader = DesktopAudioWaveformReader::new(cache_root.clone());
        let relative = RelativePath::parse("tone.wav").expect("relative path");

        let waveform = reader
            .read_waveform(media_root.to_str().expect("utf-8 root"), &relative, 32)
            .expect("waveform");

        assert_eq!(waveform.peaks.len(), 32);
        assert!((990..=1_010).contains(&waveform.duration_ms));
        assert!(waveform.peaks.iter().any(|peak| *peak > 0.9));
        assert_eq!(
            fs::read_dir(cache_root).expect("cache directory").count(),
            1
        );
    }

    #[test]
    fn discards_a_stale_cache_when_the_source_changes() {
        let directory = tempdir().expect("temporary directory");
        let media_root = directory.path().join("media");
        let cache_root = directory.path().join("cache");
        fs::create_dir_all(&media_root).expect("media root");
        let media_path = media_root.join("tone.wav");
        fs::write(&media_path, wave_fixture()).expect("wave fixture");
        let reader = DesktopAudioWaveformReader::new(cache_root.clone());
        let relative = RelativePath::parse("tone.wav").expect("relative path");
        let root = media_root.to_str().expect("utf-8 root");
        reader
            .read_waveform(root, &relative, 32)
            .expect("initial waveform");
        let cache_path = fs::read_dir(&cache_root)
            .expect("cache directory")
            .next()
            .expect("cache entry")
            .expect("cache entry")
            .path();
        let mut cached = serde_json::from_reader::<_, CachedWaveform>(
            File::open(&cache_path).expect("cached waveform"),
        )
        .expect("cached waveform json");
        cached.waveform.duration_ms = 1;
        cached.fingerprint.size_bytes = 0;
        serde_json::to_writer(File::create(&cache_path).expect("replace cache"), &cached)
            .expect("replace cache json");

        let refreshed = reader
            .read_waveform(root, &relative, 32)
            .expect("refreshed waveform");

        assert!((990..=1_010).contains(&refreshed.duration_ms));
    }

    #[test]
    #[ignore = "requires DLA_MANUAL_AUDIO_FIXTURE"]
    fn decodes_the_manually_selected_audio_fixture() {
        let path = std::env::var_os("DLA_MANUAL_AUDIO_FIXTURE").expect("manual fixture path");
        let waveform = decode_waveform(Path::new(&path), 256).expect("manual waveform");
        assert_eq!(waveform.peaks.len(), 256);
        assert!(waveform.duration_ms > 0);
        assert!(waveform.peaks.iter().any(|peak| *peak > 0.0));
    }

    fn wave_fixture() -> Vec<u8> {
        let sample_rate = 8_000_u32;
        let samples = (0..sample_rate)
            .map(|index| {
                let phase = index as f32 / sample_rate as f32 * 440.0 * std::f32::consts::TAU;
                (phase.sin() * f32::from(i16::MAX) * 0.7) as i16
            })
            .collect::<Vec<_>>();
        let data_bytes = (samples.len() * size_of::<i16>()) as u32;
        let mut bytes = Vec::with_capacity(44 + data_bytes as usize);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_bytes.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}
