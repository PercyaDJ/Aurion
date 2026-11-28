//! Metadata written by `rpicam-still --metadata <file> --metadata-format json`.
//!
//! What rpicam-apps really writes (checked in `output/output.cpp`,
//! `write_metadata`, raspberrypi/rpicam-apps, November 2026): one object per
//! capture, one line per libcamera control, `"Name": value`, where the value
//! is `ControlValue::toString()` and is only quoted when it contains a `/`.
//! So the file is *not* always valid JSON: a `Size` prints as `4056x3040`
//! without quotes, a float can print as `inf` or `nan`. The parser therefore
//! tries real JSON first, then falls back to a line scan.
//!
//! Control names (libcamera `src/libcamera/control_ids_core.yaml`):
//! - `ExposureTime`: int32, microseconds actually applied by the sensor;
//! - `AnalogueGain`: float, gain applied in the sensor;
//! - `DigitalGain`: float, gain applied afterwards by the ISP (not in the DNG);
//! - `SensorTemperature`: float, °C, only present when the sensor has a
//!   thermal sensor. For the IMX477, the Raspberry Pi helper reads register
//!   0x013a and clamps it to -20..80 °C (`cam_helper_imx477.cpp`).
//!
//! To be validated on the hardware: presence of each key on the Raspberry Pi
//! OS build of rpicam-apps, and the value of `SensorTemperature` against a
//! thermometer.

use serde::{Deserialize, Serialize};

/// Values actually applied by the camera for one capture. Every field is
/// optional: an older rpicam-apps, a missing file or a sensor without
/// thermal sensor simply leaves it empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RpicamMetadata {
    pub exposure_time_us: Option<u64>,
    pub analogue_gain: Option<f64>,
    pub digital_gain: Option<f64>,
    pub sensor_temp_c: Option<f64>,
}

impl RpicamMetadata {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Parse the metadata file. Never fails: unknown or broken content gives
/// empty fields.
pub fn parse(text: &str) -> RpicamMetadata {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        // A timelapse writes an array of objects: the last one is the latest
        let obj = match &v {
            serde_json::Value::Array(a) => a.iter().rev().find(|o| o.is_object()),
            o if o.is_object() => Some(o),
            _ => None,
        };
        if let Some(obj) = obj {
            let num = |k: &str| obj.get(k).and_then(json_number);
            return build(num("ExposureTime"), num("AnalogueGain"), num("DigitalGain"), num("SensorTemperature"));
        }
    }
    // Fallback: `"Key": value` lines, the last occurrence wins
    let (mut exp, mut ag, mut dg, mut temp) = (None, None, None, None);
    for line in text.lines() {
        let Some((key, value)) = split_line(line) else { continue };
        let slot = match key {
            "ExposureTime" => &mut exp,
            "AnalogueGain" => &mut ag,
            "DigitalGain" => &mut dg,
            "SensorTemperature" => &mut temp,
            _ => continue,
        };
        if let Some(x) = parse_number(value) {
            *slot = Some(x);
        }
    }
    build(exp, ag, dg, temp)
}

/// Read and parse a metadata file (empty result when it is missing).
pub fn read(path: &std::path::Path) -> RpicamMetadata {
    std::fs::read_to_string(path).map(|t| parse(&t)).unwrap_or_default()
}

fn build(exp: Option<f64>, ag: Option<f64>, dg: Option<f64>, temp: Option<f64>) -> RpicamMetadata {
    RpicamMetadata {
        exposure_time_us: exp.filter(|v| *v >= 0.0 && *v < 1e12).map(|v| v.round() as u64),
        analogue_gain: ag.filter(|v| *v > 0.0),
        digital_gain: dg.filter(|v| *v > 0.0),
        // Outside any physical range: a parsing accident, not a temperature
        sensor_temp_c: temp.filter(|v| (-60.0..=150.0).contains(v)),
    }
}

fn json_number(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => parse_number(s),
        _ => None,
    }
}

fn split_line(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix('"')?;
    let (key, rest) = rest.split_once('"')?;
    let value = rest.trim_start().strip_prefix(':')?;
    Some((key, value))
}

fn parse_number(raw: &str) -> Option<f64> {
    let v = raw.trim().trim_end_matches(',').trim().trim_matches('"').trim();
    v.parse::<f64>().ok().filter(|x| x.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape produced by `write_metadata` (rpicam-apps): unquoted `Size`,
    /// so not valid JSON.
    const RPICAM_LIKE: &str = "{\n    \"SensorBlackLevels\": [ 4096, 4096, 4096, 4096 ],\n    \"ScalerCrop\": \"(0, 0)/4056x3040\",\n    \"DigitalGain\": 1.000000,\n    \"ExposureTime\": 7999987,\n    \"SensorTemperature\": 34.000000,\n    \"AnalogueGain\": 8.000000,\n    \"FrameDuration\": 8100000,\n    \"SomeSize\": 4056x3040,\n    \"Lux\": inf\n}";

    #[test]
    fn rpicam_output_that_is_not_valid_json() {
        assert!(serde_json::from_str::<serde_json::Value>(RPICAM_LIKE).is_err());
        let m = parse(RPICAM_LIKE);
        assert_eq!(m.exposure_time_us, Some(7_999_987));
        assert_eq!(m.analogue_gain, Some(8.0));
        assert_eq!(m.digital_gain, Some(1.0));
        assert_eq!(m.sensor_temp_c, Some(34.0));
    }

    #[test]
    fn valid_json_object_and_array() {
        let m = parse(r#"{"ExposureTime": 1000, "AnalogueGain": 2.5, "DigitalGain": 1.2}"#);
        assert_eq!(m.exposure_time_us, Some(1000));
        assert_eq!(m.analogue_gain, Some(2.5));
        assert_eq!(m.digital_gain, Some(1.2));
        assert_eq!(m.sensor_temp_c, None, "no thermal sensor: no temperature");
        let m = parse(r#"[{"ExposureTime": 1}, {"ExposureTime": 2, "SensorTemperature": "21.5"}]"#);
        assert_eq!(m.exposure_time_us, Some(2), "latest frame of a timelapse");
        assert_eq!(m.sensor_temp_c, Some(21.5));
    }

    #[test]
    fn garbage_gives_empty_fields() {
        assert!(parse("").is_empty());
        assert!(parse("not metadata at all").is_empty());
        assert!(parse("{\"SensorTemperature\": nan,\n\"AnalogueGain\": -3}").is_empty());
        assert!(parse("{\"SensorTemperature\": 9999}").is_empty());
        assert!(read(std::path::Path::new("/nonexistent/aurion.json")).is_empty());
    }
}
