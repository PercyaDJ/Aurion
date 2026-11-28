//! Sun and Moon from the place of the camera, and the gates they put on the
//! aurora detection (step C of the detection plan, audit annex A 2.3).
//!
//! Pure Rust, no dependency, low precision (a few hundredths of a degree for
//! the Sun, a few tenths for the Moon): plenty for "is the Sun below -12°"
//! and "is the Moon up, how full".
//!
//! Sources:
//! - Sun: low-precision formulas of *The Astronomical Almanac* (section C),
//!   also published by the U.S. Naval Observatory as "Approximate Solar
//!   Coordinates" (stated accuracy about 1 arcminute within two centuries of
//!   2000). Checked in the tests against Meeus, *Astronomical Algorithms*
//!   (2nd ed.), example 25.a.
//! - Moon: low-precision series of *The Astronomical Almanac* (section D,
//!   "low-precision formulae for geocentric coordinates of the Moon", stated
//!   accuracy 0.3° in longitude, 0.2° in latitude). Checked in the tests
//!   against Meeus example 47.a. The page number of the printed source is
//!   to be confirmed (the coefficients are validated by the test only).
//! - Sidereal time: Meeus formula 12.4 (terms in T² and T³ neglected: less
//!   than 0.01° over this century).
//! - Illuminated fraction: Meeus chapter 48 (formulas 48.2 and 48.3).
//!
//! Reference values of the tests: the Go package `github.com/soniakeys/meeus`
//! (v3), files `solar/solar_test.go`, `moonposition/moonposition_test.go` and
//! `moonillum/moonillum_test.go`, which reproduce the examples of the book.

use serde::Serialize;

use crate::core::time::{Timestamp, ZonedDateTime};

/// Detection is suspended while the Sun is above this altitude (bright
/// civil twilight: -6° is the standard limit of civil twilight).
pub const SUN_SUSPEND_DEG: f64 = -6.0;
/// Between [`SUN_SUSPEND_DEG`] and this altitude (nautical twilight, -12°
/// is its standard limit) the thresholds are raised by [`TWILIGHT_FACTOR`].
pub const TWILIGHT_GATE_DEG: f64 = -12.0;
/// Thresholds multiplied by this factor in nautical twilight (to validate
/// on real nights).
pub const TWILIGHT_FACTOR: f64 = 2.0;
/// Moon above the horizon: thresholds multiplied by 1 + this × illuminated
/// fraction (×1.5 at full Moon; to validate on real nights).
pub const MOON_GAIN: f64 = 0.5;

/// Place of the camera (degrees, north and east positive).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub latitude: f64,
    pub longitude: f64,
}

fn sin_d(x: f64) -> f64 {
    x.to_radians().sin()
}
fn cos_d(x: f64) -> f64 {
    x.to_radians().cos()
}
fn norm360(x: f64) -> f64 {
    x.rem_euclid(360.0)
}

/// Julian date of an instant (UTC; the 69 s between TT and UTC in 2026 are
/// below the precision of these formulas).
pub fn julian_date(t: Timestamp) -> f64 {
    t.as_millisecond() as f64 / 86_400_000.0 + 2_440_587.5
}

/// Equatorial coordinates (degrees) and distance.
#[derive(Debug, Clone, Copy)]
pub struct Equatorial {
    pub ra: f64,
    pub dec: f64,
}

/// Ecliptic to equatorial coordinates, obliquity `eps`.
fn to_equatorial(lon: f64, lat: f64, eps: f64) -> Equatorial {
    let l = cos_d(lat) * cos_d(lon);
    let m = cos_d(eps) * cos_d(lat) * sin_d(lon) - sin_d(eps) * sin_d(lat);
    let n = sin_d(eps) * cos_d(lat) * sin_d(lon) + cos_d(eps) * sin_d(lat);
    Equatorial { ra: norm360(m.atan2(l).to_degrees()), dec: n.clamp(-1.0, 1.0).asin().to_degrees() }
}

fn obliquity(jd: f64) -> f64 {
    23.439 - 0.000_000_4 * (jd - 2_451_545.0)
}

/// Apparent ecliptic longitude of the Sun (degrees), latitude 0.
pub fn sun_longitude(jd: f64) -> f64 {
    let n = jd - 2_451_545.0;
    let l = 280.460 + 0.985_647_4 * n;
    let g = 357.528 + 0.985_600_3 * n;
    norm360(l + 1.915 * sin_d(g) + 0.020 * sin_d(2.0 * g))
}

pub fn sun_equatorial(jd: f64) -> Equatorial {
    to_equatorial(sun_longitude(jd), 0.0, obliquity(jd))
}

/// Geocentric ecliptic longitude, latitude and equatorial horizontal
/// parallax of the Moon (degrees).
pub fn moon_ecliptic(jd: f64) -> (f64, f64, f64) {
    let t = (jd - 2_451_545.0) / 36_525.0;
    let lon = 218.32 + 481_267.881 * t
        + 6.29 * sin_d(135.0 + 477_198.87 * t)
        - 1.27 * sin_d(259.3 - 413_335.36 * t)
        + 0.66 * sin_d(235.7 + 890_534.22 * t)
        + 0.21 * sin_d(269.9 + 954_397.74 * t)
        - 0.19 * sin_d(357.5 + 35_999.05 * t)
        - 0.11 * sin_d(186.5 + 966_404.03 * t);
    let lat = 5.13 * sin_d(93.3 + 483_202.02 * t)
        + 0.28 * sin_d(228.2 + 960_400.89 * t)
        - 0.28 * sin_d(318.3 + 6_003.15 * t)
        - 0.17 * sin_d(217.6 - 407_332.21 * t);
    let parallax = 0.9508
        + 0.0518 * cos_d(135.0 + 477_198.87 * t)
        + 0.0095 * cos_d(259.3 - 413_335.36 * t)
        + 0.0078 * cos_d(235.7 + 890_534.22 * t)
        + 0.0028 * cos_d(269.9 + 954_397.74 * t);
    (norm360(lon), lat, parallax)
}

/// Greenwich mean sidereal time (degrees).
pub fn gmst(jd: f64) -> f64 {
    norm360(280.460_618_37 + 360.985_647_366_29 * (jd - 2_451_545.0))
}

/// Altitude (degrees) of a body above the horizon of `place`.
fn altitude(eq: Equatorial, jd: f64, place: Place) -> f64 {
    let h = gmst(jd) + place.longitude - eq.ra;
    let s = sin_d(place.latitude) * sin_d(eq.dec) + cos_d(place.latitude) * cos_d(eq.dec) * cos_d(h);
    s.clamp(-1.0, 1.0).asin().to_degrees()
}

/// Mean Earth-Sun distance (km) and Earth equatorial radius (km).
const AU_KM: f64 = 149_597_870.7;
const EARTH_RADIUS_KM: f64 = 6_378.14;

/// Illuminated fraction of the Moon (0 new, 1 full), Meeus 48.2 and 48.3
/// with the Sun at its mean distance.
pub fn moon_illumination(jd: f64) -> f64 {
    let (lon, lat, parallax) = moon_ecliptic(jd);
    let sun = sun_equatorial(jd);
    let moon = to_equatorial(lon, lat, obliquity(jd));
    let cos_psi = sin_d(sun.dec) * sin_d(moon.dec) + cos_d(sun.dec) * cos_d(moon.dec) * cos_d(sun.ra - moon.ra);
    let psi = cos_psi.clamp(-1.0, 1.0).acos();
    let delta = EARTH_RADIUS_KM / sin_d(parallax);
    let i = (AU_KM * psi.sin()).atan2(delta - AU_KM * psi.cos());
    (1.0 + i.cos()) / 2.0
}

/// Sun and Moon seen from the camera at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SkyState {
    pub sun_alt_deg: f64,
    /// Topocentric (parallax removed: up to 1° for the Moon).
    pub moon_alt_deg: f64,
    pub moon_illumination: f64,
}

pub fn sky_state(t: Timestamp, place: Place) -> SkyState {
    let jd = julian_date(t);
    let sun_alt = altitude(sun_equatorial(jd), jd, place);
    let (lon, lat, parallax) = moon_ecliptic(jd);
    let geo = altitude(to_equatorial(lon, lat, obliquity(jd)), jd, place);
    SkyState {
        sun_alt_deg: sun_alt,
        moon_alt_deg: geo - parallax * cos_d(geo),
        moon_illumination: moon_illumination(jd),
    }
}

/// Gate on the detection for one sky state.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gate {
    /// The thresholds are multiplied by this factor (1 = no gate).
    pub factor: f64,
    pub suspended: bool,
    /// Why, in plain words (session.log, home screen); `None` = no gate.
    pub reason: Option<String>,
}

impl Gate {
    pub fn none() -> Self {
        Self { factor: 1.0, suspended: false, reason: None }
    }

    pub fn detection(&self) -> crate::core::detection::DetectionGate {
        crate::core::detection::DetectionGate { factor: self.factor, suspended: self.suspended }
    }
}

/// Twilight and Moon rules: suspended above [`SUN_SUSPEND_DEG`], thresholds
/// ×[`TWILIGHT_FACTOR`] above [`TWILIGHT_GATE_DEG`], ×(1 + [`MOON_GAIN`] ×
/// illuminated fraction) while the Moon is up. Factors multiply.
pub fn gate(s: &SkyState) -> Gate {
    let fmt = |x: f64| format!("{:.0}", x).replace("-0", "0");
    if s.sun_alt_deg >= SUN_SUSPEND_DEG {
        return Gate {
            factor: 1.0,
            suspended: true,
            reason: Some(format!("Soleil à {}° : ciel trop clair, détection suspendue", fmt(s.sun_alt_deg))),
        };
    }
    let mut factor = 1.0;
    let mut why = Vec::new();
    if s.sun_alt_deg >= TWILIGHT_GATE_DEG {
        factor *= TWILIGHT_FACTOR;
        why.push(format!("crépuscule (Soleil à {}°)", fmt(s.sun_alt_deg)));
    }
    if s.moon_alt_deg > 0.0 {
        factor *= 1.0 + MOON_GAIN * s.moon_illumination;
        why.push(format!(
            "Lune levée ({}°, éclairée à {} %)",
            fmt(s.moon_alt_deg),
            fmt(s.moon_illumination * 100.0)
        ));
    }
    if why.is_empty() {
        return Gate::none();
    }
    Gate {
        factor,
        suspended: false,
        reason: Some(format!("Seuils relevés ×{:.1} : {}", factor, why.join(", ")).replace('.', ",")),
    }
}

/// Dusk and dawn around the night that follows `from`: the first time the
/// Sun goes below `alt_deg`, and the next time it comes back above it,
/// searched over 36 h by steps of 2 min. `None` when the Sun never goes
/// below (summer at high latitude), or never comes back within the search.
/// A Sun already below at `from` gives `from` as dusk.
pub fn night_between(from: &ZonedDateTime, place: Place, alt_deg: f64) -> Option<(ZonedDateTime, ZonedDateTime)> {
    const STEP: i64 = 120;
    let alt = |t: &ZonedDateTime| sky_state(t.timestamp(), place).sun_alt_deg;
    let mut t = from.clone();
    let end = crate::core::time::plus_secs(from, 36 * 3600);
    let mut dusk = if alt(&t) < alt_deg { Some(t.clone()) } else { None };
    while t < end {
        let next = crate::core::time::plus_secs(&t, STEP);
        let (a, b) = (alt(&t), alt(&next));
        match dusk {
            None if a >= alt_deg && b < alt_deg => dusk = Some(next.clone()),
            Some(ref d) if a < alt_deg && b >= alt_deg => return Some((d.clone(), next)),
            _ => {}
        }
        t = next;
    }
    None
}

/// What the home screen tells about the coming night.
#[derive(Debug, Clone)]
pub struct NightSummary {
    /// Sun below [`TWILIGHT_GATE_DEG`]; `None` when it never goes that low.
    pub dark: Option<(ZonedDateTime, ZonedDateTime)>,
    /// Hours of that dark window with the Moon above the horizon.
    pub moon_up_hours: f64,
    /// Illuminated fraction in the middle of the dark window (or 12 h from now).
    pub moon_illumination: f64,
}

/// Dark window of the coming night, and the Moon during it (sampled every
/// 10 min).
pub fn night_summary(from: &ZonedDateTime, place: Place) -> NightSummary {
    let dark = night_between(from, place, TWILIGHT_GATE_DEG);
    let (a, b) = match &dark {
        Some((d, e)) => (d.clone(), e.clone()),
        None => (from.clone(), crate::core::time::plus_secs(from, 24 * 3600)),
    };
    let span = crate::core::time::secs_between(&a, &b).max(0);
    let mut up = 0;
    let mut t = 0;
    while t < span {
        if sky_state(crate::core::time::plus_secs(&a, t).timestamp(), place).moon_alt_deg > 0.0 {
            up += 600;
        }
        t += 600;
    }
    let mid = crate::core::time::plus_secs(&a, span / 2);
    NightSummary {
        moon_up_hours: up as f64 / 3600.0,
        moon_illumination: moon_illumination(julian_date(mid.timestamp())),
        dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::time::utc;

    fn jd(y: i16, m: i8, d: i8) -> f64 {
        julian_date(utc(jiff::civil::date(y, m, d).at(0, 0, 0, 0)))
    }

    #[test]
    fn julian_date_of_the_meeus_examples() {
        assert_eq!(jd(1992, 10, 13), 2_448_908.5, "Meeus 25.a");
        assert_eq!(jd(1992, 4, 12), 2_448_724.5, "Meeus 47.a");
    }

    #[test]
    fn sun_matches_meeus_example_25a() {
        // 1992 October 13, 0h TD: α = 13h13m31.4s, δ = -7°47'06"
        let sun = sun_equatorial(jd(1992, 10, 13));
        let ra = (13.0 + 13.0 / 60.0 + 31.4 / 3600.0) * 15.0;
        let dec = -(7.0 + 47.0 / 60.0 + 6.0 / 3600.0);
        assert!((sun.ra - ra).abs() < 0.01, "RA {} vs {}", sun.ra, ra);
        assert!((sun.dec - dec).abs() < 0.01, "dec {} vs {}", sun.dec, dec);
        // Apparent longitude 199°54'32" (low precision: no nutation)
        assert!((sun_longitude(jd(1992, 10, 13)) - (199.0 + 54.0 / 60.0 + 32.0 / 3600.0)).abs() < 0.02);
    }

    #[test]
    fn moon_matches_meeus_example_47a() {
        // 1992 April 12, 0h TD: λ = 133.162655°, β = -3.229126°, π = 0.991990°
        let (lon, lat, parallax) = moon_ecliptic(jd(1992, 4, 12));
        assert!((lon - 133.162655).abs() < 0.3, "λ {}", lon);
        assert!((lat - -3.229126).abs() < 0.2, "β {}", lat);
        assert!((parallax - 0.991990).abs() < 0.005, "π {}", parallax);
    }

    #[test]
    fn moon_illumination_matches_meeus_chapter_48() {
        // Same date: k = 0.6775 (reference of moonillum_test.go)
        let k = moon_illumination(jd(1992, 4, 12));
        assert!((k - 0.6775).abs() < 0.01, "k {}", k);
    }

    // Two well-known facts at Tromsø (69.65 N, 18.96 E): polar night at
    // the December solstice, midnight Sun at the June solstice.
    const TROMSO: Place = Place { latitude: 69.65, longitude: 18.96 };

    #[test]
    fn polar_night_and_midnight_sun_at_tromso() {
        for h in 0..24 {
            let s = sky_state(utc(jiff::civil::date(2026, 12, 21).at(h, 0, 0, 0)), TROMSO);
            assert!(s.sun_alt_deg < 0.0, "December, {} h UTC: {}", h, s.sun_alt_deg);
            let s = sky_state(utc(jiff::civil::date(2026, 6, 21).at(h, 0, 0, 0)), TROMSO);
            assert!(s.sun_alt_deg > 0.0, "June, {} h UTC: {}", h, s.sun_alt_deg);
        }
    }

    #[test]
    fn noon_altitude_follows_latitude_and_declination() {
        // At the upper transit, altitude = 90 - |latitude - declination|
        let place = Place { latitude: 45.19, longitude: 0.0 };
        let mut best = (-90.0_f64, 0.0_f64);
        for m in 0..24 * 60 {
            let t = utc(jiff::civil::date(2026, 3, 1).at(0, 0, 0, 0)) + jiff::SignedDuration::from_mins(m);
            let a = sky_state(t, place).sun_alt_deg;
            if a > best.0 {
                best = (a, sun_equatorial(julian_date(t)).dec);
            }
        }
        assert!((best.0 - (90.0 - (place.latitude - best.1).abs())).abs() < 0.02, "{:?}", best);
    }

    #[test]
    fn gates_follow_the_sun_then_the_moon() {
        let s = |sun, moon, k| SkyState { sun_alt_deg: sun, moon_alt_deg: moon, moon_illumination: k };
        let day = gate(&s(-3.0, -20.0, 0.0));
        assert!(day.suspended && day.reason.as_deref().unwrap().contains("-3°"), "{:?}", day);
        let nautical = gate(&s(-9.0, -20.0, 0.0));
        assert!(!nautical.suspended && nautical.factor == TWILIGHT_FACTOR);
        assert!(nautical.reason.unwrap().contains("crépuscule"));
        assert_eq!(gate(&s(-20.0, -20.0, 1.0)), Gate::none(), "night, Moon down");
        let full = gate(&s(-20.0, 30.0, 1.0));
        assert!((full.factor - 1.5).abs() < 1e-9 && full.reason.unwrap().contains("Lune levée (30°, éclairée à 100 %)"));
        let both = gate(&s(-10.0, 5.0, 0.5));
        assert!((both.factor - 2.0 * 1.25).abs() < 1e-9, "{:?}", both);
    }

    #[test]
    fn night_at_tromso_and_none_in_its_summer() {
        let tz = jiff::tz::TimeZone::UTC;
        let from = utc(jiff::civil::date(2026, 2, 15).at(12, 0, 0, 0)).to_zoned_datetime(tz.clone());
        let (dusk, dawn) = night_between(&from, TROMSO, TWILIGHT_GATE_DEG).expect("a dark night in February");
        assert!(dusk > from && dawn > dusk);
        let check = |t: &ZonedDateTime, before: bool| {
            let a = sky_state(crate::core::time::plus_secs(t, -120).timestamp(), TROMSO).sun_alt_deg;
            assert_eq!(a >= TWILIGHT_GATE_DEG, before, "{} {}", t, a);
            assert_eq!(sky_state(t.timestamp(), TROMSO).sun_alt_deg < TWILIGHT_GATE_DEG, before);
        };
        check(&dusk, true);
        check(&dawn, false);
        // Late May: the Sun never goes below -12° at Tromsø
        let may = utc(jiff::civil::date(2026, 5, 25).at(12, 0, 0, 0)).to_zoned_datetime(tz);
        assert!(night_between(&may, TROMSO, TWILIGHT_GATE_DEG).is_none());
    }
}
