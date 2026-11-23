//! Daylight saving changes, in the Europe/Paris time zone (2026: clocks go
//! forward on 29 March at 02:00, back on 25 October at 03:00). Written against
//! the chrono implementation before the move to jiff, kept as is afterwards:
//! only the `lib` module may change. Expected instants are Unix seconds (UTC),
//! independent of any date library.
//!
//! One test only: the time zone is set for the whole process before any date
//! is computed, so no other test can see it change.

use aurion::core::models::TimeRange;
use aurion::core::night::{next_occurrence, NightMarker, ResumePlan};
use aurion::core::orchestrator::{before_night, wake_before, BeforeNight, NightWindow, WindowState};
use aurion::core::validate::{parse_local_epoch, LocalTimeError};

/// The only part that depends on the date library.
mod lib {
    use chrono::{Local, NaiveTime, TimeZone};

    pub type Local_ = chrono::DateTime<Local>;

    /// Local wall time (a time repeated in October: what chrono `earliest()` gives).
    pub fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Local_ {
        Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest().unwrap()
    }
    pub fn epoch(t: &Local_) -> i64 {
        t.timestamp()
    }
    pub fn time(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }
}
use lib::*;

#[test]
fn daylight_saving_changes_in_paris() {
    std::env::set_var("TZ", "Europe/Paris");
    let night = TimeRange { start: time(21, 0), end: time(6, 0) };

    // The evening start falls on the right instant on both change days
    assert_eq!(epoch(&next_occurrence(local(2026, 3, 28, 22, 0), time(21, 0))), 1_774_810_800, "21:00 CEST, 29/03");
    assert_eq!(epoch(&next_occurrence(local(2026, 10, 24, 22, 0), time(21, 0))), 1_792_958_400, "21:00 CET, 25/10");

    // A time skipped in March (02:30 does not exist on 29/03): the next one
    // that exists, the following day
    assert_eq!(epoch(&next_occurrence(local(2026, 3, 28, 22, 0), time(2, 30))), 1_774_830_600, "02:30, 30/03");
    // A time repeated in October: the code asks for its first occurrence, but
    // chrono 0.4.43 `earliest()` returns the later one (02:30 CET, 01:30 UTC)
    assert_eq!(epoch(&next_occurrence(local(2026, 10, 24, 22, 0), time(2, 30))), 1_792_891_800, "02:30 CET, 25/10");

    // Wake-up 10 min of real time before the start
    assert_eq!(epoch(&wake_before(local(2026, 3, 29, 7, 0), time(21, 0))), 1_774_810_200, "20:50 CEST");

    // Hours before the night counted in real time: 01:00 CET → 21:00 CEST is
    // 20 h on the wall clock but 19 h of real time
    let short = TimeRange { start: time(21, 0), end: time(0, 30) };
    match before_night(local(2026, 3, 29, 1, 0), &short, true) {
        BeforeNight::Sleep { wake, hours } => {
            assert_eq!(epoch(&wake), 1_774_810_200);
            assert_eq!(hours, 19);
        }
        other => panic!("{other:?}"),
    }

    // Timer: 3 h of real time from 01:00 CET (00:00 UTC) end at 05:00 CEST
    let start = local(2026, 3, 29, 1, 0);
    assert_eq!(epoch(&start), 1_774_742_400);
    let mut w = NightWindow::new(Some(3.0), night.clone(), start);
    assert_eq!(w.check(local(2026, 3, 29, 4, 59)), WindowState::Capture);
    assert_eq!(w.check(local(2026, 3, 29, 5, 0)), WindowState::Stop);

    // Range night across the October change: still capturing in the repeated
    // hour, stops after 06:00 CET
    let mut w = NightWindow::new(None, night.clone(), local(2026, 10, 24, 20, 0));
    assert_eq!(w.check(local(2026, 10, 24, 21, 0)), WindowState::Capture);
    assert_eq!(w.check(local(2026, 10, 25, 2, 30)), WindowState::Capture);
    assert_eq!(w.check(local(2026, 10, 25, 6, 1)), WindowState::Stop);

    // Interrupted range night: resumes until 06:00 CET (05:00 UTC)
    let m = NightMarker::new(local(2026, 10, 24, 14, 0), None);
    assert_eq!(m.resume_plan(local(2026, 10, 25, 5, 59), &night), Some(ResumePlan::Range));
    assert_eq!(m.resume_plan(local(2026, 10, 25, 6, 0), &night), None);
    assert_eq!(epoch(&local(2026, 10, 25, 6, 0)), 1_792_904_400);

    // Date typed in the UI (time setting without a phone clock)
    assert_eq!(parse_local_epoch("2026-03-29 02:30"), Err(LocalTimeError::Nonexistent));
    assert_eq!(parse_local_epoch("2026-03-29 03:30"), Ok(1_774_747_800));
    assert_eq!(parse_local_epoch("2026-10-25 02:30"), Ok(1_792_891_800), "chrono: the later of the two");
    assert_eq!(parse_local_epoch("2026-10-25"), Err(LocalTimeError::Format));
}
