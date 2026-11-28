//! Siril script written at the end of a night with RAW files:
//! `sessions/<night>/calibrer.ssf` converts the DNGs of the night and
//! calibrates them with the master dark of the same ISO, exposure and
//! sensor temperature (`darks/master/`, see `core::master_dark`).
//!
//! Every command and option of the script was checked in the source code of
//! Siril (gitlab.com/free-astro/siril), tag 1.4.0 and commit 53828ab of
//! 27 November 2026 (version 1.5.0 in development):
//! - `src/core/command_list.h`: `requires min_version`, `cd directory`,
//!   `convert basename [-out=]`, `calibrate sequencename [-dark=filename]
//!   [-cc=dark [siglo sighi]] [-cfa]`;
//! - `src/core/command.c` (`process_convert`): `-out=` is a path relative to
//!   the current directory, created when missing; the current directory does
//!   not change;
//! - `src/core/command_line_processor.c`: lines starting with `#` are
//!   comments; a script runs in the current directory of Siril (it is
//!   restored at the end), **not** in the folder of the script: the user
//!   sets it to the night folder first, or runs
//!   `siril-cli -d <night folder> -s calibrer.ssf` (`-d` and `-s`:
//!   `src/main-cli.c`).
//!
//! Not run with real files in Siril yet: to be validated.

use std::path::Path;

use crate::core::master_dark::{base_name, shutter_label};

/// Script file name in the night folder.
pub const SCRIPT_NAME: &str = "calibrer.ssf";
/// Siril sequence (and file prefix) of the converted lights.
const SEQUENCE: &str = "brut";

/// Master dark chosen for the night.
#[derive(Debug, Clone, PartialEq)]
pub enum DarkChoice {
    /// Same ISO, exposure and temperature band.
    Exact(String),
    /// Same ISO and exposure, closest temperature band available.
    Closest(String),
    /// None yet: the expected name of the master to make.
    Missing(String),
}

impl DarkChoice {
    pub fn file(&self) -> &str {
        match self {
            DarkChoice::Exact(f) | DarkChoice::Closest(f) | DarkChoice::Missing(f) => f,
        }
    }
}

/// Temperature of a master dark file name (`..._T35.fits` → 35).
fn temp_of(name: &str) -> Option<i64> {
    name.strip_suffix(".fits")?.rsplit_once("_T")?.1.parse().ok()
}

/// Pick the master dark of `available` (file names of `darks/master/`) for
/// a night at `iso`, `shutter_us` and `temp_c`.
pub fn choose_master(available: &[String], iso: u32, shutter_us: u64, temp_c: Option<f64>) -> DarkChoice {
    let expected = format!("master_dark_{}.fits", base_name(iso, shutter_us, temp_c));
    if available.contains(&expected) {
        return DarkChoice::Exact(expected);
    }
    let prefix = format!("master_dark_ISO{}_{}_T", iso, shutter_label(shutter_us));
    let target = temp_c.unwrap_or(f64::NAN);
    let closest = available
        .iter()
        .filter(|n| n.starts_with(&prefix))
        .filter_map(|n| temp_of(n).map(|t| (n, t)))
        .min_by(|a, b| (a.1 as f64 - target).abs().total_cmp(&(b.1 as f64 - target).abs()).then(a.0.cmp(b.0)));
    match (closest, temp_c) {
        (Some((n, _)), Some(_)) => DarkChoice::Closest(n.clone()),
        _ => DarkChoice::Missing(expected),
    }
}

/// Text of the script for `night`.
pub fn script(night: &str, iso: u32, shutter_us: u64, temp_c: Option<f64>, dark: &DarkChoice, version: &str) -> String {
    let temp = temp_c.map(|t| format!("{:.1} °C", t)).unwrap_or_else(|| "inconnue".into());
    let note = match dark {
        DarkChoice::Exact(_) => "master dark aux mêmes ISO, pose et température (tranche de 5 °C)".to_string(),
        DarkChoice::Closest(_) => "pas de master dark à cette température : le plus proche aux mêmes ISO et pose".to_string(),
        DarkChoice::Missing(_) => {
            "ce master dark n'existe pas encore : faire la série de darks proposée par Aurion (mêmes ISO et pose, objectif bouché), puis lancer ce script".to_string()
        }
    };
    let dark_path = format!("../../../darks/master/{}", dark.file());
    format!(
        "# Aurion {version} : calibration des RAW de la nuit {night}\n\
         # Réglages moyens de la nuit : ISO {iso}, pose {pose}, température {temp}.\n\
         # Master dark : {file}\n\
         # ({note}).\n\
         #\n\
         # Lancement : le script s'exécute dans le dossier courant de Siril, pas\n\
         # dans le dossier du script. Choisir ce dossier de nuit comme dossier\n\
         # de travail dans Siril, puis lancer le script ; ou en ligne de commande :\n\
         #   siril-cli -d <dossier de cette nuit> -s {script}\n\
         # Résultat : dossier siril/, séquence pp_{seq} (images calibrées, CFA).\n\
         # Si la pose ou l'ISO ont changé pendant la nuit, ce dark ne vaut que pour\n\
         # les images prises à ces réglages.\n\
         #\n\
         # Syntaxe vérifiée dans le code source de Siril (gitlab.com/free-astro/siril,\n\
         # version 1.4.0) : src/core/command_list.h (requires, cd, convert,\n\
         # calibrate), src/core/command.c (process_convert, option -out=),\n\
         # src/main-cli.c (options -d et -s).\n\
         requires 1.4.0\n\
         cd {raw}\n\
         convert {seq} -out=../siril\n\
         cd ../siril\n\
         calibrate {seq} -dark={dark_path} -cc=dark -cfa\n",
        pose = shutter_label(shutter_us),
        file = dark.file(),
        script = SCRIPT_NAME,
        seq = SEQUENCE,
        raw = crate::core::layout::RAW_DIR,
    )
}

/// Write `calibrer.ssf` in `night_dir` (`<key>/sessions/<night>`) when the
/// night has RAW files. Returns the master dark chosen, None without RAW.
pub fn write_for_night(night_dir: &Path, iso: u32, shutter_us: u64, temp_c: Option<f64>) -> std::io::Result<Option<DarkChoice>> {
    let raw_dir = night_dir.join(crate::core::layout::RAW_DIR);
    let has_raw = std::fs::read_dir(&raw_dir)
        .map(|it| it.flatten().any(|e| crate::core::layout::is_raw(&e.file_name().to_string_lossy())))
        .unwrap_or(false);
    if !has_raw {
        return Ok(None);
    }
    let night = night_dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let master_dir = night_dir.parent().and_then(Path::parent).map(|root| root.join("darks").join("master"));
    let available: Vec<String> = master_dir
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|it| it.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
        .unwrap_or_default();
    let choice = choose_master(&available, iso, shutter_us, temp_c);
    let text = script(&night, iso, shutter_us, temp_c, &choice, env!("CARGO_PKG_VERSION"));
    crate::core::config::write_atomic(&night_dir.join(SCRIPT_NAME), text.as_bytes(), 0o644)?;
    Ok(Some(choice))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_master_dark_of_the_night() {
        let files: Vec<String> = ["master_dark_ISO800_5.0s_T35.fits", "master_dark_ISO800_5.0s_T20.fits", "master_dark_ISO1600_5.0s_T35.fits", "hot_pixels_ISO800_5.0s_T35.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(choose_master(&files, 800, 5_000_000, Some(36.0)), DarkChoice::Exact("master_dark_ISO800_5.0s_T35.fits".into()));
        assert_eq!(choose_master(&files, 800, 5_000_000, Some(26.0)), DarkChoice::Closest("master_dark_ISO800_5.0s_T20.fits".into()));
        assert_eq!(choose_master(&files, 800, 8_000_000, Some(36.0)), DarkChoice::Missing("master_dark_ISO800_8.0s_T35.fits".into()));
        assert_eq!(choose_master(&files, 800, 5_000_000, None), DarkChoice::Missing("master_dark_ISO800_5.0s_Tinconnue.fits".into()));
        assert_eq!(temp_of("master_dark_ISO800_5.0s_T-5.fits"), Some(-5));
    }

    #[test]
    fn script_uses_only_checked_commands() {
        let s = script("2026-11-28_21-00", 800, 5_000_000, Some(36.2), &DarkChoice::Exact("master_dark_ISO800_5.0s_T35.fits".into()), "1.17.0");
        let commands: Vec<&str> = s.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).collect();
        assert_eq!(
            commands,
            [
                "requires 1.4.0",
                "cd RAW",
                "convert brut -out=../siril",
                "cd ../siril",
                "calibrate brut -dark=../../../darks/master/master_dark_ISO800_5.0s_T35.fits -cc=dark -cfa",
            ]
        );
        assert!(s.contains("ISO 800, pose 5.0s, température 36.2 °C"));
        assert!(!s.contains('\u{2014}'));
    }

    #[test]
    fn written_only_for_nights_with_raw() {
        let key = tempfile::tempdir().unwrap();
        let night = key.path().join("sessions").join("2026-11-28_21-00");
        std::fs::create_dir_all(night.join("JPG")).unwrap();
        assert_eq!(write_for_night(&night, 800, 5_000_000, Some(34.0)).unwrap(), None);
        assert!(!night.join(SCRIPT_NAME).exists());
        std::fs::create_dir_all(night.join("RAW")).unwrap();
        std::fs::write(night.join("RAW").join("aurora_20261128_210000_00000.dng"), b"x").unwrap();
        std::fs::create_dir_all(key.path().join("darks").join("master")).unwrap();
        std::fs::write(key.path().join("darks/master/master_dark_ISO800_5.0s_T35.fits"), b"x").unwrap();
        let choice = write_for_night(&night, 800, 5_000_000, Some(34.0)).unwrap();
        assert_eq!(choice, Some(DarkChoice::Exact("master_dark_ISO800_5.0s_T35.fits".into())));
        let text = std::fs::read_to_string(night.join(SCRIPT_NAME)).unwrap();
        assert!(text.contains("-dark=../../../darks/master/master_dark_ISO800_5.0s_T35.fits"));
        // The relative path really leads from siril/ to the master dark
        std::fs::create_dir_all(night.join("siril")).unwrap();
        assert!(night.join("siril").join("../../../darks/master/master_dark_ISO800_5.0s_T35.fits").exists());
    }
}
