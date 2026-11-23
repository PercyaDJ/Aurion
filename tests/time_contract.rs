//! Time behaviour contract, written against the chrono implementation before
//! the move to jiff (chrono is unmaintained, RUSTSEC-2026-0543) and kept as is
//! afterwards: only the small `lib` module below may change with the date
//! library (plus the `.clone()` a jiff date needs, it is not `Copy`), never
//! the cases or the expected values.
//!
//! What a camera depends on: settings files already on SD cards and USB keys
//! (time range "HH:MM:SS"), dates typed in the UI, file and folder names on
//! the key, and the night schedule (range, timer, resume, wake-up).
//! Daylight saving changes are covered in `time_dst.rs` (own process, TZ set).

use aurion::core::config::{AppConfig, TimeRangeConfig};
use aurion::core::layout::parse_image_timestamp;
use aurion::core::models::TimeRange;
use aurion::core::night::{next_occurrence, NightMarker, ResumePlan};
use aurion::core::orchestrator::{before_night, keeps_raw, wake_before, BeforeNight, NightWindow, WindowState};
use aurion::core::models::OutputFormat;
use aurion::core::validate::parse_local_datetime;
use aurion::web::gallery::{assign_images_to_sessions, parse_session_start};

/// The only part that depends on the date library.
mod lib {
    use aurion::core::time::{local_earliest, local_tz, CivilDateTime, Time, ZonedDateTime};

    pub type Local_ = ZonedDateTime;

    /// Local date/time on the machine running the tests (TZ-independent use:
    /// every assertion compares instants built the same way).
    pub fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Local_ {
        let dt = jiff::civil::date(y as i16, mo as i8, d as i8).at(h as i8, mi as i8, 0, 0);
        local_earliest(dt, &local_tz()).unwrap()
    }
    pub fn epoch(t: &Local_) -> i64 {
        t.timestamp().as_second()
    }
    pub fn plus_secs(t: &Local_, s: i64) -> Local_ {
        aurion::core::time::plus_secs(t, s)
    }
    pub fn time(h: u32, m: u32) -> Time {
        aurion::core::time::hm(h as i8, m as i8)
    }
    pub fn ymdhms(dt: &CivilDateTime) -> String {
        dt.strftime("%Y-%m-%d %H:%M:%S").to_string()
    }
}
use lib::*;

fn range(sh: u32, sm: u32, eh: u32, em: u32) -> TimeRange {
    TimeRange { start: time(sh, sm), end: time(eh, em) }
}

// ─── Settings files already on cameras ─────────────────────

#[test]
fn time_range_settings_keep_their_format() {
    let json = r#"{"start":"21:00:00","end":"06:00:00","duration_hours":null}"#;
    let r: TimeRangeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(serde_json::to_string(&r).unwrap(), json);

    // Seconds are kept; the UI always sends ":00", older files may differ
    let r: TimeRangeConfig = serde_json::from_str(r#"{"start":"21:30:15","end":"05:45:00"}"#).unwrap();
    let out = serde_json::to_value(&r).unwrap();
    assert_eq!(out["start"], "21:30:15");
    assert_eq!(out["end"], "05:45:00");

    // Invalid hours are refused
    for bad in ["25:00:00", "21:60:00", "", "soir", "21h00"] {
        let json = format!(r#"{{"start":"{bad}","end":"06:00:00"}}"#);
        assert!(serde_json::from_str::<TimeRangeConfig>(&json).is_err(), "{bad} accepted");
    }
}

#[test]
fn hour_without_seconds_is_accepted() {
    // "HH:MM" (hand-edited file): read as HH:MM:00
    let r: TimeRangeConfig = serde_json::from_str(r#"{"start":"21:00","end":"06:00"}"#).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap()["start"], "21:00:00");
}

#[test]
fn default_config_round_trips() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config/default.json")).unwrap();
    let cfg: AppConfig = serde_json::from_str(&text).unwrap();
    let out = serde_json::to_value(&cfg).unwrap();
    assert_eq!(out["time_range"]["start"], "21:00:00");
    assert_eq!(out["time_range"]["end"], "06:00:00");
    let again: AppConfig = serde_json::from_value(out.clone()).unwrap();
    assert_eq!(serde_json::to_value(&again).unwrap(), out);
}

// ─── Dates typed in the UI ─────────────────────────────────

#[test]
fn ui_dates_are_parsed_like_before() {
    let ok = [
        ("2026-01-15 21:05", "2026-01-15 21:05:00"),
        ("2026-01-15 21:05:30", "2026-01-15 21:05:30"),
        ("2026-01-15T21:05", "2026-01-15 21:05:00"),
        ("2026-01-15T21:05:30", "2026-01-15 21:05:30"),
        ("  2026-01-15 21:05  ", "2026-01-15 21:05:00"),
        ("2024-02-29 00:00", "2024-02-29 00:00:00"),
    ];
    for (input, want) in ok {
        let got = parse_local_datetime(input).unwrap_or_else(|| panic!("{input:?} refused"));
        assert_eq!(ymdhms(&got), want, "{input:?}");
    }
    let refused = [
        "", "2026-01-15", "21:05", "2026-02-30 10:00", "2025-02-29 10:00", "2026-13-01 10:00",
        "2026-01-15 24:00", "2026-01-15 21:05Z", "2026-01-15 21:05:30+01:00", "15/01/2026 21:05", "demain",
    ];
    for input in refused {
        assert!(parse_local_datetime(input).is_none(), "{input:?} accepted");
    }
}

#[test]
fn ui_dates_with_short_fields() {
    // Single-digit month, day or hour: accepted by the parser in place
    for (input, want) in [("2026-1-5 3:04", "2026-01-05 03:04:00"), ("2026-01-15 9:05", "2026-01-15 09:05:00")] {
        let got = parse_local_datetime(input).map(|d| ymdhms(&d));
        assert_eq!(got.as_deref(), Some(want), "{input:?}");
    }
}

// ─── Names on the USB key ──────────────────────────────────

#[test]
fn image_names_give_their_time() {
    let cases = [
        ("aurora_20260115_210530_00001.jpg", Some("2026-01-15 21:05:30")),
        ("aurora_20260115_210530_00001_AURORA.dng", Some("2026-01-15 21:05:30")),
        ("aurora_20260115_210530.jpg", Some("2026-01-15 21:05:30")),
        ("aurora_20261315_210530_00001.jpg", None),
        ("aurora_2026011_210530_00001.jpg", None),
        ("photo_20260115_210530_00001.jpg", None),
        ("aurora_", None),
    ];
    for (name, want) in cases {
        assert_eq!(parse_image_timestamp(name).map(|d| ymdhms(&d)).as_deref(), want, "{name}");
    }
}

#[test]
fn session_names_give_their_start() {
    let cases = [
        ("2026-01-15_21-05", Some("2026-01-15 21:05:00")),
        ("2026-01-15_21-05_reprise", Some("2026-01-15 21:05:00")),
        ("2026-01-15_25-05", None),
        ("2026-01-15", None),
        ("darks", None),
    ];
    for (name, want) in cases {
        assert_eq!(parse_session_start(name).map(|d| ymdhms(&d)).as_deref(), want, "{name}");
    }
}

#[test]
fn images_go_to_their_session() {
    let sessions = vec!["2026-01-15_21-00".to_string(), "2026-01-16_21-00".to_string()];
    let images: Vec<String> = [
        "aurora_20260115_205800_00001.jpg", // 2 min before: start margin
        "aurora_20260115_230000_00002.jpg",
        "aurora_20260116_050000_00003.jpg",
        "aurora_20260116_140000_00004.jpg", // > 16 h after: orphan
        "aurora_20260116_210100_00005.jpg",
        "aurora_20260114_120000_00006.jpg", // before any session
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let m = assign_images_to_sessions(&sessions, &images);
    assert_eq!(m["2026-01-15_21-00"], vec![images[0].clone(), images[1].clone(), images[2].clone()]);
    assert_eq!(m["2026-01-16_21-00"], vec![images[4].clone()]);
}

// ─── Night schedule ────────────────────────────────────────

#[test]
fn time_range_boundaries() {
    let r = range(21, 0, 6, 0);
    assert!(r.contains(time(21, 0)));
    assert!(r.contains(time(6, 0)));
    assert!(r.contains(time(0, 0)));
    assert!(!r.contains(time(6, 1)));
    assert!(!r.contains(time(20, 59)));
    let day = range(9, 0, 17, 0);
    assert!(day.contains(time(9, 0)) && day.contains(time(17, 0)) && !day.contains(time(17, 1)));
    let same = range(22, 0, 22, 0);
    assert!(same.contains(time(22, 0)) && !same.contains(time(22, 1)));
}

#[test]
fn next_start_and_wake_up() {
    let t = time(21, 0);
    assert_eq!(epoch(&next_occurrence(local(2026, 1, 15, 14, 0), t)), epoch(&local(2026, 1, 15, 21, 0)));
    assert_eq!(epoch(&next_occurrence(local(2026, 1, 15, 21, 0), t)), epoch(&local(2026, 1, 16, 21, 0)));
    assert_eq!(epoch(&next_occurrence(local(2026, 12, 31, 22, 0), t)), epoch(&local(2027, 1, 1, 21, 0)));
    assert_eq!(epoch(&wake_before(local(2026, 1, 15, 7, 0), t)), epoch(&local(2026, 1, 15, 20, 50)));
    // 5 min before the start: 10 min before today's start, already past (never
    // used to sleep: before_night only sleeps when the start is > 2 h away)
    assert_eq!(epoch(&wake_before(local(2026, 1, 15, 20, 55), t)), epoch(&local(2026, 1, 15, 20, 50)));
}

#[test]
fn before_the_night() {
    let r = range(21, 0, 6, 0);
    assert_eq!(before_night(local(2026, 1, 15, 22, 0), &r, true), BeforeNight::Start);
    assert_eq!(before_night(local(2026, 1, 15, 19, 30), &r, true), BeforeNight::Wait, "starts in 1 h 30");
    assert_eq!(before_night(local(2026, 1, 15, 10, 0), &r, false), BeforeNight::Wait, "cannot wake");
    match before_night(local(2026, 1, 15, 10, 0), &r, true) {
        BeforeNight::Sleep { wake, hours } => {
            assert_eq!(epoch(&wake), epoch(&local(2026, 1, 15, 20, 50)));
            assert_eq!(hours, 11);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn night_window_timer_and_range() {
    let start = local(2026, 1, 15, 21, 0);
    let mut w = NightWindow::new(Some(2.5), range(21, 0, 6, 0), start.clone());
    assert_eq!(w.check(plus_secs(&start, 9_000 - 1)), WindowState::Capture);
    assert_eq!(w.check(plus_secs(&start, 9_000)), WindowState::Stop);

    let mut w = NightWindow::new(None, range(21, 0, 6, 0), local(2026, 1, 15, 18, 0));
    assert_eq!(w.check(local(2026, 1, 15, 20, 59)), WindowState::Wait);
    assert_eq!(w.check(local(2026, 1, 15, 21, 0)), WindowState::Capture);
    assert_eq!(w.check(local(2026, 1, 16, 6, 0)), WindowState::Capture);
    assert_eq!(w.check(local(2026, 1, 16, 6, 1)), WindowState::Stop);
}

#[test]
fn raw_kept_after_an_aurora() {
    let now = local(2026, 1, 15, 23, 0);
    let hold = aurion::core::orchestrator::AURORA_RAW_HOLD_SECS;
    assert!(keeps_raw(OutputFormat::JpgAuroraRaw, Some(plus_secs(&now, -hold)), now.clone()));
    assert!(!keeps_raw(OutputFormat::JpgAuroraRaw, Some(plus_secs(&now, -hold - 1)), now.clone()));
    assert!(!keeps_raw(OutputFormat::JpgAuroraRaw, None, now.clone()));
    assert!(keeps_raw(OutputFormat::RawDng, None, now));
}

#[test]
fn interrupted_night_resumes_like_before() {
    let r = range(21, 0, 6, 0);
    // Range night launched at 14:00: resumes until 06:00 the next morning
    let m = NightMarker::new(local(2026, 1, 15, 14, 0), None);
    assert_eq!(m.started_epoch_ms, epoch(&local(2026, 1, 15, 14, 0)) * 1000);
    assert_eq!(m.resume_plan(local(2026, 1, 16, 5, 59), &r), Some(ResumePlan::Range));
    assert_eq!(m.resume_plan(local(2026, 1, 16, 6, 0), &r), None);
    // Timer night: what is left, capped by the planned duration
    let m = NightMarker::new(local(2026, 1, 15, 21, 0), Some(3.0));
    assert_eq!(m.end_epoch_ms, Some((epoch(&local(2026, 1, 15, 21, 0)) + 3 * 3600) * 1000));
    assert_eq!(m.resume_plan(local(2026, 1, 15, 22, 0), &r), Some(ResumePlan::Timer(2.0)));
    assert_eq!(m.resume_plan(local(2026, 1, 16, 0, 0), &r), None);
}
