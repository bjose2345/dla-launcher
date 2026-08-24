use std::{borrow::Cow, fs, path::Path};

use encoding_rs::{BIG5, EUC_KR, Encoding, GBK, SHIFT_JIS, UTF_16BE, UTF_16LE, WINDOWS_1252};
use serde::Serialize;

const VIDEO_EXTENSIONS: [&str; 8] = ["avi", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "webm"];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleFormat {
    WebVtt,
    SubRip,
    SubStationAlpha,
    Sami,
}

impl SubtitleFormat {
    fn from_extension(extension: &str) -> Option<Self> {
        match extension {
            "vtt" => Some(Self::WebVtt),
            "srt" => Some(Self::SubRip),
            "ass" | "ssa" => Some(Self::SubStationAlpha),
            "smi" => Some(Self::Sami),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::WebVtt => "VTT",
            Self::SubRip => "SRT",
            Self::SubStationAlpha => "ASS",
            Self::Sami => "SMI",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SidecarSubtitle {
    pub index: u32,
    pub relative_path: String,
    pub label: String,
    pub language: Option<String>,
    pub format: SubtitleFormat,
}

pub fn subtitle_label(video_stem: &str, subtitle_stem: &str, format: SubtitleFormat) -> String {
    let suffix = subtitle_stem
        .strip_prefix(video_stem)
        .filter(|rest| rest.is_empty() || starts_with_pairing_boundary(rest))
        .map(|rest| rest.trim_start_matches(['.', '_', '-', ' ']))
        .unwrap_or(subtitle_stem);
    if suffix.is_empty() {
        format.label().to_owned()
    } else {
        suffix.to_owned()
    }
}

pub fn find_sidecar_subtitles(video_path: &Path) -> Vec<SidecarSubtitle> {
    let Some(directory) = video_path.parent() else {
        return Vec::new();
    };
    let Some(video_stem) = video_path.file_stem().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    let video_count = entries
        .iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    VIDEO_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
                })
        })
        .count();

    let mut found = entries
        .into_iter()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())?
                .to_ascii_lowercase();
            let format = SubtitleFormat::from_extension(&extension)?;
            let stem = path.file_stem().and_then(|value| value.to_str())?;
            if video_count != 1 && !stems_are_paired(video_stem, stem) {
                return None;
            }
            let name = path.file_name().and_then(|value| value.to_str())?;
            let label = subtitle_label(video_stem, stem, format);
            Some(SidecarSubtitle {
                index: 0,
                relative_path: name.to_owned(),
                language: subtitle_language(&label),
                label,
                format,
            })
        })
        .collect::<Vec<_>>();
    found.sort_by(|left, right| {
        left.relative_path
            .to_ascii_lowercase()
            .cmp(&right.relative_path.to_ascii_lowercase())
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    for (index, subtitle) in found.iter_mut().enumerate() {
        subtitle.index = index as u32;
    }
    found
}

pub fn subtitle_to_web_vtt(path: &Path, format: SubtitleFormat) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let source = decode_subtitle(&bytes, path);
    let converted = match format {
        SubtitleFormat::WebVtt => normalize_web_vtt(&source),
        SubtitleFormat::SubRip => subrip_to_web_vtt(&source),
        SubtitleFormat::SubStationAlpha => ass_to_web_vtt(&source),
        SubtitleFormat::Sami => sami_to_web_vtt(&source),
    };
    if converted.trim() == "WEBVTT" {
        return Err("the subtitle contains no readable cues".to_owned());
    }
    Ok(converted)
}

pub fn to_web_vtt(source: &str) -> String {
    subrip_to_web_vtt(source)
}

fn stems_are_paired(video_stem: &str, subtitle_stem: &str) -> bool {
    subtitle_stem
        .strip_prefix(video_stem)
        .is_some_and(|rest| rest.is_empty() || starts_with_pairing_boundary(rest))
}

fn starts_with_pairing_boundary(value: &str) -> bool {
    value.starts_with(['.', '_', '-', ' '])
}

fn subtitle_language(label: &str) -> Option<String> {
    let normalized = label.to_ascii_lowercase().replace(['_', ' '], "-");
    let language = match normalized.as_str() {
        "en" | "eng" | "english" => "en",
        "ja" | "jpn" | "japanese" => "ja",
        "ko" | "kor" | "korean" => "ko",
        "chs" | "sc" | "zh-cn" | "zh-hans" => "zh-Hans",
        "cht" | "tc" | "zh-tw" | "zh-hant" => "zh-Hant",
        "de" | "deu" | "ger" | "german" => "de",
        "es" | "spa" | "spanish" => "es",
        "fr" | "fra" | "fre" | "french" => "fr",
        "it" | "ita" | "italian" => "it",
        "pt" | "por" | "portuguese" => "pt",
        "ru" | "rus" | "russian" => "ru",
        _ => return None,
    };
    Some(language.to_owned())
}

fn decode_subtitle<'a>(bytes: &'a [u8], path: &Path) -> Cow<'a, str> {
    if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(bytes);
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return UTF_16LE.decode_without_bom_handling(bytes).0;
    }
    if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return UTF_16BE.decode_without_bom_handling(bytes).0;
    }
    if let Ok(source) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(source);
    }

    let hint = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let encodings: [&'static Encoding; 5] = if hint.contains("jpn") || hint.contains("japanese") {
        [SHIFT_JIS, EUC_KR, GBK, BIG5, WINDOWS_1252]
    } else if hint.contains("cht") || hint.contains("big5") {
        [BIG5, GBK, EUC_KR, SHIFT_JIS, WINDOWS_1252]
    } else if hint.contains("chs") || hint.contains("gbk") {
        [GBK, BIG5, EUC_KR, SHIFT_JIS, WINDOWS_1252]
    } else if extension == "smi" || hint.contains("kor") || hint.contains("korean") {
        [EUC_KR, SHIFT_JIS, GBK, BIG5, WINDOWS_1252]
    } else {
        [WINDOWS_1252, SHIFT_JIS, EUC_KR, GBK, BIG5]
    };
    for encoding in encodings {
        let (decoded, had_errors) = encoding.decode_without_bom_handling(bytes);
        if !had_errors {
            return Cow::Owned(decoded.into_owned());
        }
    }
    WINDOWS_1252.decode_without_bom_handling(bytes).0
}

fn normalize_web_vtt(source: &str) -> String {
    let source = source
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if source.trim_start().starts_with("WEBVTT") {
        source
    } else {
        format!("WEBVTT\n\n{}", source.trim_start())
    }
}

fn subrip_to_web_vtt(source: &str) -> String {
    if source.trim_start().starts_with("WEBVTT") {
        return normalize_web_vtt(source);
    }
    let mut output = String::from("WEBVTT\n\n");
    for line in source.replace("\r\n", "\n").replace('\r', "\n").lines() {
        if line.contains("-->") {
            output.push_str(&line.replace(',', "."));
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    output
}

fn ass_to_web_vtt(source: &str) -> String {
    let mut output = String::from("WEBVTT\n\n");
    let mut in_events = false;
    let mut fields = Vec::<String>::new();
    for line in source.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_events = trimmed.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(value) = strip_prefix_ascii_case(trimmed, "format:") {
            fields = value
                .split(',')
                .map(|field| field.trim().to_ascii_lowercase())
                .collect();
            continue;
        }
        let Some(value) = strip_prefix_ascii_case(trimmed, "dialogue:") else {
            continue;
        };
        let field_count = fields.len().max(10);
        let values = value
            .splitn(field_count, ',')
            .map(str::trim)
            .collect::<Vec<_>>();
        let start_index = fields
            .iter()
            .position(|field| field == "start")
            .unwrap_or(1);
        let end_index = fields.iter().position(|field| field == "end").unwrap_or(2);
        let text_index = fields
            .iter()
            .position(|field| field == "text")
            .unwrap_or(field_count - 1);
        let (Some(start), Some(end), Some(text)) = (
            values
                .get(start_index)
                .and_then(|value| parse_ass_timestamp(value)),
            values
                .get(end_index)
                .and_then(|value| parse_ass_timestamp(value)),
            values.get(text_index),
        ) else {
            continue;
        };
        let text = clean_ass_text(text);
        if text.is_empty() || end <= start {
            continue;
        }
        push_cue(&mut output, start, end, &text);
    }
    output
}

fn sami_to_web_vtt(source: &str) -> String {
    let lower = source.to_ascii_lowercase();
    let starts = lower
        .match_indices("<sync")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut events = Vec::<(u64, String)>::new();
    for (index, start) in starts.iter().copied().enumerate() {
        let Some(tag_end_offset) = lower[start..].find('>') else {
            continue;
        };
        let tag_end = start + tag_end_offset;
        let Some(time) = parse_sami_start(&source[start..=tag_end]) else {
            continue;
        };
        let body_end = starts.get(index + 1).copied().unwrap_or(source.len());
        let text = clean_html_text(&source[tag_end + 1..body_end]);
        events.push((time, text));
    }

    let mut output = String::from("WEBVTT\n\n");
    for (index, (start, text)) in events.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let end = events
            .get(index + 1)
            .map(|(time, _)| *time)
            .filter(|time| time > start)
            .unwrap_or(start.saturating_add(4_000));
        push_cue(&mut output, *start, end, text);
    }
    output
}

fn parse_sami_start(tag: &str) -> Option<u64> {
    let lower = tag.to_ascii_lowercase();
    let start = lower.find("start")? + "start".len();
    let value = lower[start..].trim_start();
    let value = value.strip_prefix('=')?.trim_start();
    let value = value.trim_start_matches(['\'', '"']);
    let digits = value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits.parse().ok()
}

fn parse_ass_timestamp(value: &str) -> Option<u64> {
    let mut parts = value.split(':');
    let hours = parts.next()?.trim().parse::<u64>().ok()?;
    let minutes = parts.next()?.trim().parse::<u64>().ok()?;
    let seconds = parts.next()?.trim().parse::<f64>().ok()?;
    if parts.next().is_some() || !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(hours * 3_600_000 + minutes * 60_000 + (seconds * 1_000.0).round() as u64)
}

fn clean_ass_text(value: &str) -> String {
    let mut output = String::new();
    let mut in_override = false;
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '{' => in_override = true,
            '}' if in_override => in_override = false,
            '\\' if !in_override => match chars.peek().copied() {
                Some('N' | 'n') => {
                    chars.next();
                    output.push('\n');
                }
                Some('h') => {
                    chars.next();
                    output.push(' ');
                }
                _ => output.push(character),
            },
            _ if !in_override => output.push(character),
            _ => {}
        }
    }
    clean_text_lines(&decode_html_entities(&output))
}

fn clean_html_text(value: &str) -> String {
    let mut output = String::new();
    let mut remaining = value;
    while let Some(open) = remaining.find('<') {
        output.push_str(&remaining[..open]);
        let Some(close) = remaining[open..].find('>') else {
            output.push_str(&remaining[open..]);
            remaining = "";
            break;
        };
        let tag = remaining[open + 1..open + close]
            .trim()
            .to_ascii_lowercase();
        if tag.starts_with("br") || tag.starts_with("/p") {
            output.push('\n');
        }
        remaining = &remaining[open + close + 1..];
    }
    output.push_str(remaining);
    clean_text_lines(&decode_html_entities(&output))
}

fn decode_html_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(index) = remaining.find('&') {
        output.push_str(&remaining[..index]);
        let entity = &remaining[index..];
        let Some(end) = entity.find(';').filter(|end| *end <= 12) else {
            output.push('&');
            remaining = &entity[1..];
            continue;
        };
        let name = &entity[1..end];
        let decoded = match name.to_ascii_lowercase().as_str() {
            "nbsp" => Some(' '),
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|value| u32::from_str_radix(value, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|value| value.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(character) = decoded {
            output.push(character);
        } else {
            output.push_str(&entity[..=end]);
        }
        remaining = &entity[end + 1..];
    }
    output.push_str(remaining);
    output
}

fn clean_text_lines(value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_cue(output: &mut String, start: u64, end: u64, text: &str) {
    output.push_str(&format_vtt_timestamp(start));
    output.push_str(" --> ");
    output.push_str(&format_vtt_timestamp(end));
    output.push('\n');
    output.push_str(text);
    output.push_str("\n\n");
}

fn format_vtt_timestamp(milliseconds: u64) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds % 3_600_000 / 60_000;
    let seconds = milliseconds % 60_000 / 1_000;
    let milliseconds = milliseconds % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}.{milliseconds:03}")
}

fn strip_prefix_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn srt_timestamps_become_web_vtt_timestamps() {
        let converted = to_web_vtt("1\n00:00:01,000 --> 00:00:04,500\nHello\n");
        assert!(converted.starts_with("WEBVTT\n\n"));
        assert!(converted.contains("00:00:01.000 --> 00:00:04.500"));
        assert!(converted.contains("Hello"));
    }

    #[test]
    fn ass_dialogue_keeps_timing_and_plain_text() {
        let converted = ass_to_web_vtt(
            "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.25,0:00:04.50,Default,,0,0,0,,{\\i1}Hello{\\i0}\\NWorld, again\n",
        );
        assert!(converted.contains("00:00:01.250 --> 00:00:04.500"));
        assert!(converted.contains("Hello\nWorld, again"));
        assert!(!converted.contains("\\i1"));
    }

    #[test]
    fn sami_sync_events_become_bounded_cues() {
        let converted = sami_to_web_vtt(
            "<SAMI><BODY><SYNC Start=1000><P Class=ENCC>Hello<br>there<SYNC Start=2500><P Class=ENCC>&nbsp;<SYNC Start=4000><P>Next</BODY></SAMI>",
        );
        assert!(converted.contains("00:00:01.000 --> 00:00:02.500"));
        assert!(converted.contains("Hello\nthere"));
        assert!(converted.contains("00:00:04.000 --> 00:00:08.000"));
    }

    #[test]
    fn a_single_video_accepts_language_named_sidecars() {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("movie.mp4"), b"video").expect("video");
        fs::write(directory.path().join("ENG.smi"), b"subtitle").expect("subtitle");
        fs::write(directory.path().join("CHT.srt"), b"subtitle").expect("subtitle");

        let tracks = find_sidecar_subtitles(&directory.path().join("movie.mp4"));

        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].label, "CHT");
        assert_eq!(tracks[0].language.as_deref(), Some("zh-Hant"));
        assert_eq!(tracks[1].label, "ENG");
        assert_eq!(tracks[1].language.as_deref(), Some("en"));
    }

    #[test]
    fn multiple_videos_only_accept_stem_paired_sidecars() {
        let directory = tempdir().expect("temporary directory");
        fs::write(directory.path().join("ep1.mp4"), b"video").expect("video");
        fs::write(directory.path().join("ep2.mp4"), b"video").expect("video");
        fs::write(directory.path().join("ep1.en.srt"), b"subtitle").expect("subtitle");
        fs::write(directory.path().join("ep10.srt"), b"subtitle").expect("subtitle");
        fs::write(directory.path().join("ENG.smi"), b"subtitle").expect("subtitle");

        let tracks = find_sidecar_subtitles(&directory.path().join("ep1.mp4"));

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].label, "en");
    }

    #[test]
    fn legacy_korean_sami_is_decoded_before_conversion() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("KOR.smi");
        let source = "<SAMI><SYNC Start=1000><P>한국어 자막<SYNC Start=2000><P>&nbsp;</SAMI>";
        let (encoded, _, _) = EUC_KR.encode(source);
        fs::write(&path, encoded).expect("subtitle");

        let converted = subtitle_to_web_vtt(&path, SubtitleFormat::Sami).expect("conversion");

        assert!(converted.contains("한국어 자막"));
    }
}
