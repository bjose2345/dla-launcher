const MAX_NAME_BYTES: usize = 200;
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A file name that is safe on Linux and Windows. An archive whose catalog
/// name changes under this is not downloaded, because multipart volumes are
/// found by their exact names.
pub fn local_name(remote: &str) -> String {
    let replaced: String = remote
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect();
    let mut name = replaced
        .trim_start()
        .trim_end_matches(['.', ' '])
        .to_owned();
    if name.is_empty() {
        name = "download".to_owned();
    }
    let device = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    if RESERVED_NAMES.contains(&device.as_str()) {
        name.insert(0, '_');
    }
    if name.len() > MAX_NAME_BYTES {
        let extension = name
            .rfind('.')
            .filter(|&dot| dot > 0 && name.len() - dot <= 16)
            .map(|dot| name[dot..].to_owned())
            .unwrap_or_default();
        let mut end = MAX_NAME_BYTES - extension.len();
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name = format!("{}{extension}", name[..end].trim_end_matches(['.', ' ']));
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_names_are_safe_on_both_systems() {
        assert_eq!(local_name("RJ108375__作品.zip"), "RJ108375__作品.zip");
        assert_eq!(local_name("a/b\\c:d?.zip"), "a_b_c_d_.zip");
        assert_eq!(local_name("con.txt"), "_con.txt");
        assert_eq!(local_name("  name. . "), "name");
        assert_eq!(local_name(".."), "download");
        assert_eq!(local_name("tab\there"), "tab_here");
        let long = format!("{}.zip", "作".repeat(100));
        let safe = local_name(&long);
        assert!(safe.len() <= MAX_NAME_BYTES && safe.ends_with(".zip"));
    }
}
