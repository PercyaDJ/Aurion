//! Pure input validation helpers shared by the web layer and the config.
//!
//! Every value coming from the network (file names, session names, preset
//! names, Wi-Fi credentials, dates) goes through one of these functions
//! before touching the filesystem or a privileged command.

/// Image extensions served and managed by the gallery.
pub const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "dng", "raw"];

/// Maximum length accepted for any user-supplied name.
const MAX_NAME_LEN: usize = 128;

/// A "safe name" only contains `[A-Za-z0-9._-]`, does not start with a dot
/// and is not empty. It can never escape a directory (no `/`, `\`, `..`).
pub fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

/// Lower-cased extension of a file name, if any.
pub fn extension_lower(name: &str) -> Option<String> {
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

/// True for a safe file name with an image extension.
pub fn is_safe_image_name(name: &str) -> bool {
    is_safe_name(name)
        && extension_lower(name)
            .map(|e| IMAGE_EXTENSIONS.contains(&e.as_str()))
            .unwrap_or(false)
}

/// Preset names: letters, digits, space, `_` and `-`, 1 to 40 characters.
/// Spaces are allowed for readability; the file name is derived with
/// [`preset_file_stem`].
pub fn is_valid_preset_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.chars().count() <= 40
        && trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '_' || c == '-')
}

/// File stem used to store a preset on disk (spaces become `_`).
pub fn preset_file_stem(name: &str) -> String {
    name.trim().replace(' ', "_")
}

/// Wi-Fi SSID: 1 to 32 bytes, printable characters only (no control chars),
/// no leading/trailing space. Prevents injection into hostapd/wpa configs.
pub fn validate_ssid(ssid: &str) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("Le SSID doit faire entre 1 et 32 octets".into());
    }
    if ssid.chars().any(|c| c.is_control()) {
        return Err("Le SSID contient des caractères interdits".into());
    }
    if ssid.trim() != ssid {
        return Err("Le SSID ne doit pas commencer ou finir par un espace".into());
    }
    Ok(())
}

/// WPA2 passphrase: printable ASCII (0x20..=0x7e), `min_len` to 63 characters.
pub fn validate_wpa_passphrase(pass: &str, min_len: usize) -> Result<(), String> {
    let len = pass.chars().count();
    if len < min_len || len > 63 {
        return Err(format!(
            "Le mot de passe Wi-Fi doit faire entre {} et 63 caractères",
            min_len
        ));
    }
    if !pass.bytes().all(|b| (0x20..=0x7e).contains(&b)) {
        return Err("Le mot de passe Wi-Fi ne doit contenir que des caractères ASCII imprimables".into());
    }
    Ok(())
}

/// 2.4 GHz channels usable in Europe.
pub fn validate_channel(channel: u32) -> Result<(), String> {
    if (1..=13).contains(&channel) {
        Ok(())
    } else {
        Err("Le canal Wi-Fi doit être entre 1 et 13".into())
    }
}

/// Parse a date/time sent by the UI.
/// Accepts `YYYY-MM-DD HH:MM[:SS]` or `YYYY-MM-DDTHH:MM[:SS]` (local time).
pub fn parse_local_datetime(input: &str) -> Option<chrono::NaiveDateTime> {
    let s = input.trim().replace('T', " ");
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
        .iter()
        .find_map(|fmt| chrono::NaiveDateTime::parse_from_str(&s, fmt).ok())
}

/// IANA time zone name such as `Europe/Paris` or `UTC`.
/// Only the syntax is checked here; the privileged helper checks that the
/// zone exists in `/usr/share/zoneinfo`.
pub fn is_valid_timezone_name(tz: &str) -> bool {
    !tz.is_empty()
        && tz.len() <= 64
        && !tz.starts_with('/')
        && !tz.contains("..")
        && tz
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'_' || b == b'-' || b == b'+')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_names() {
        assert!(is_safe_name("aurora_20260305_213500_00001.jpg"));
        assert!(is_safe_name("2026-03-05_21-30"));
        assert!(!is_safe_name(""));
        assert!(!is_safe_name(".."));
        assert!(!is_safe_name("../etc/passwd"));
        assert!(!is_safe_name("a/b.jpg"));
        assert!(!is_safe_name("a\\b.jpg"));
        assert!(!is_safe_name(".hidden"));
        assert!(!is_safe_name("x..jpg"));
        assert!(!is_safe_name("évil.jpg"));
        assert!(!is_safe_name("a b.jpg"));
        assert!(!is_safe_name("a\nb.jpg"));
        assert!(!is_safe_name("a\"b.jpg"));
        assert!(!is_safe_name(&"a".repeat(200)));
    }

    #[test]
    fn safe_image_names() {
        assert!(is_safe_image_name("x.JPG"));
        assert!(is_safe_image_name("x.dng"));
        assert!(!is_safe_image_name("x.txt"));
        assert!(!is_safe_image_name("event.jsonl"));
        assert!(!is_safe_image_name("noext"));
    }

    #[test]
    fn preset_names() {
        assert!(is_valid_preset_name("Nuit Laponie"));
        assert!(is_valid_preset_name("Test_1-b"));
        assert!(!is_valid_preset_name(""));
        assert!(!is_valid_preset_name("   "));
        assert!(!is_valid_preset_name("../../aurion"));
        assert!(!is_valid_preset_name("a/b"));
        assert!(!is_valid_preset_name("a.b"));
        assert!(!is_valid_preset_name(&"x".repeat(41)));
        assert_eq!(preset_file_stem(" Nuit Laponie "), "Nuit_Laponie");
    }

    #[test]
    fn ssid_validation() {
        assert!(validate_ssid("Aurion").is_ok());
        assert!(validate_ssid("Mon Wifi 5G").is_ok());
        assert!(validate_ssid("").is_err());
        assert!(validate_ssid(&"a".repeat(33)).is_err());
        assert!(validate_ssid("evil\nwpa=0").is_err());
        assert!(validate_ssid(" lead").is_err());
    }

    #[test]
    fn passphrase_validation() {
        assert!(validate_wpa_passphrase("aurora2024", 10).is_ok());
        assert!(validate_wpa_passphrase("short", 10).is_err());
        assert!(validate_wpa_passphrase(&"a".repeat(64), 8).is_err());
        assert!(validate_wpa_passphrase("line\nbreak12", 8).is_err());
        assert!(validate_wpa_passphrase("accentué123", 8).is_err());
        assert!(validate_wpa_passphrase("12345678", 8).is_ok());
    }

    #[test]
    fn channel_validation() {
        assert!(validate_channel(1).is_ok());
        assert!(validate_channel(13).is_ok());
        assert!(validate_channel(0).is_err());
        assert!(validate_channel(14).is_err());
    }

    #[test]
    fn datetime_parsing() {
        assert!(parse_local_datetime("2026-02-22 10:00").is_some());
        assert!(parse_local_datetime("2026-02-22T10:00:30").is_some());
        assert!(parse_local_datetime("now; rm -rf /").is_none());
        assert!(parse_local_datetime("2026-13-40 10:00").is_none());
    }

    #[test]
    fn timezone_names() {
        assert!(is_valid_timezone_name("Europe/Paris"));
        assert!(is_valid_timezone_name("America/Argentina/Buenos_Aires"));
        assert!(is_valid_timezone_name("UTC"));
        assert!(is_valid_timezone_name("Etc/GMT+1"));
        assert!(!is_valid_timezone_name("../../etc/shadow"));
        assert!(!is_valid_timezone_name("/etc/passwd"));
        assert!(!is_valid_timezone_name("Europe/Paris; reboot"));
        assert!(!is_valid_timezone_name(""));
    }
}
