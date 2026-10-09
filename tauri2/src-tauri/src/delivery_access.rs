use dla_connector::AccessToken;
use serde::Deserialize;
use zeroize::Zeroizing;

pub struct EmbeddedAccess {
    pub hostname: String,
    pub token: AccessToken,
}

#[derive(Deserialize)]
struct Fields {
    hostname: String,
    client_id: String,
    client_secret: String,
}

/// The launcher's downloads token embedded by `build.rs`, if this build has
/// one.
pub fn embedded() -> Option<EmbeddedAccess> {
    unmask(option_env!("DLA_LAUNCHER_ACCESS_BLOB")?)
}

fn unmask(blob: &str) -> Option<EmbeddedAccess> {
    let bytes = decode_hex(blob)?;
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return None;
    }
    let (key, masked) = bytes.split_at(bytes.len() / 2);
    let plain = Zeroizing::new(
        masked
            .iter()
            .zip(key)
            .map(|(byte, mask)| byte ^ mask)
            .collect::<Vec<u8>>(),
    );
    let fields: Fields = serde_json::from_slice(&plain).ok()?;
    Some(EmbeddedAccess {
        hostname: fields.hostname,
        token: AccessToken::new(fields.client_id, fields.client_secret),
    })
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(plain: &[u8], key: &[u8]) -> String {
        let masked: Vec<u8> = plain
            .iter()
            .zip(key)
            .map(|(byte, mask)| byte ^ mask)
            .collect();
        key.iter()
            .chain(&masked)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    #[test]
    fn unmasks_what_the_build_script_embeds() {
        let plain =
            br#"{"hostname":"rx.example.com","client_id":"abc.access","client_secret":"cfast_x"}"#;
        let key: Vec<u8> = (0..plain.len())
            .map(|index| (index * 37 + 11) as u8)
            .collect();
        let access = unmask(&mask(plain, &key)).unwrap();
        assert_eq!(access.hostname, "rx.example.com");
        assert_eq!(format!("{:?}", access.token), "AccessToken(<hidden>)");
    }

    #[test]
    fn rejects_damaged_blobs() {
        assert!(unmask("").is_none());
        assert!(unmask("abc").is_none());
        assert!(unmask("zz").is_none());
        assert!(unmask(&mask(b"not json", &[1; 8])).is_none());
    }
}
