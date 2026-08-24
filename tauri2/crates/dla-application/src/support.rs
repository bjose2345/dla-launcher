use serde::{Deserialize, Serialize};

pub const SUPPORT_SCHEMA_VERSION: u32 = 1;
pub const SUPPORT_LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;
pub const SUPPORT_LOG_FILE_COUNT: usize = 5;
pub const SUPPORT_FAULT_FILE_COUNT: usize = 3;
pub const SUPPORT_BUNDLE_BYTES: u64 = 15 * 1024 * 1024;
pub const SUPPORT_FAULT_TEXT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SupportFaultKind {
    RustPanic,
    FrontendRender,
    FrontendError,
    UnhandledRejection,
    StartupFailure,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendFaultReport {
    pub kind: SupportFaultKind,
    pub message: String,
    #[serde(default)]
    pub stack: String,
    #[serde(default)]
    pub component_stack: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportFaultRecord {
    pub schema_version: u32,
    pub kind: SupportFaultKind,
    pub occurred_at: String,
    pub run_id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stack: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub component_stack: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportFaultSummary {
    pub kind: SupportFaultKind,
    pub occurred_at: String,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportStatus {
    pub schema_version: u32,
    pub previous_shutdown_unclean: bool,
    pub previous_run_id: String,
    pub last_fault: Option<SupportFaultSummary>,
    pub retained_log_files: usize,
    pub retained_fault_files: usize,
    pub estimated_bundle_bytes: u64,
    pub max_bundle_bytes: u64,
    pub summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundleEntry {
    pub name: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportBundleManifest {
    pub schema_version: u32,
    pub report_id: String,
    pub created_at: String,
    pub app_version: String,
    pub build_id: String,
    pub platform: String,
    pub entries: Vec<SupportBundleEntry>,
    pub excluded: Vec<String>,
    pub redaction: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SupportSaveOutcome {
    Saved,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportSaveResult {
    pub outcome: SupportSaveOutcome,
    pub file_name: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionRoot {
    pub value: String,
    pub replacement: &'static str,
}

pub fn redact_text(value: &str, roots: &[RedactionRoot]) -> String {
    let mut redacted = value.replace('\0', "");
    let mut sorted = roots
        .iter()
        .filter(|root| !root.value.is_empty())
        .collect::<Vec<_>>();
    sorted.sort_by_key(|root| std::cmp::Reverse(root.value.len()));
    for root in sorted {
        redacted = redacted.replace(&root.value, root.replacement);
        let slash_value = root.value.replace('\\', "/");
        if slash_value != root.value {
            redacted = redacted.replace(&slash_value, root.replacement);
        }
    }
    redacted
        .split_inclusive(char::is_whitespace)
        .map(redact_absolute_token)
        .collect()
}

pub fn bounded_text(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}\n<truncated>", &value[..end])
}

pub fn single_line(value: &str, max_bytes: usize) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    bounded_text(&normalized, max_bytes)
}

fn redact_absolute_token(token: &str) -> String {
    let body_length = token.trim_end_matches(char::is_whitespace).len();
    let body = &token[..body_length];
    let whitespace = &token[body_length..];
    let leading_length = body
        .chars()
        .take_while(|character| matches!(character, '(' | '[' | '{' | '<' | '\'' | '"'))
        .map(char::len_utf8)
        .sum::<usize>();
    let trailing_length = body
        .chars()
        .rev()
        .take_while(|character| matches!(character, ')' | ']' | '}' | '>' | '\'' | '"' | ',' | ';'))
        .map(char::len_utf8)
        .sum::<usize>();
    if leading_length + trailing_length >= body.len() {
        return token.to_owned();
    }
    let core_end = body.len() - trailing_length;
    let core = &body[leading_length..core_end];
    let is_file_url = core.starts_with("file:///");
    let is_unix_path = core.starts_with('/') && !core.starts_with("//");
    let bytes = core.as_bytes();
    let is_windows_path = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    if !is_file_url && !is_unix_path && !is_windows_path {
        return token.to_owned();
    }
    format!(
        "{}<path>{}{whitespace}",
        &body[..leading_length],
        &body[core_end..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_known_roots_before_generic_paths() {
        let roots = [RedactionRoot {
            value: "/home/alice/.local/share/dla".to_owned(),
            replacement: "<app-data>",
        }];
        assert_eq!(
            redact_text(
                "failed at /home/alice/.local/share/dla/catalog.sqlite and /mnt/private/game.exe",
                &roots,
            ),
            "failed at <app-data>/catalog.sqlite and <path>"
        );
    }

    #[test]
    fn redacts_file_urls_and_windows_paths_without_touching_web_urls() {
        assert_eq!(
            redact_text(
                "(file:///home/alice/app.js:4) C:\\Users\\Alice\\game.exe https://example.com/help",
                &[],
            ),
            "(<path>) <path> https://example.com/help"
        );
    }

    #[test]
    fn bounds_utf8_without_splitting_a_character() {
        assert_eq!(bounded_text("123✨456", 5), "123\n<truncated>");
    }

    #[test]
    fn normalizes_fault_messages_to_one_line() {
        assert_eq!(
            single_line("failed\n  after\tlaunch", 100),
            "failed after launch"
        );
    }
}
