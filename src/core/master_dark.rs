//! Master dark and hot pixel map, computed from a series of dark DNGs.
//!
//! - **Master dark**: median, pixel by pixel, of the N raw frames (CFA data,
//!   before any demosaicing). The files are read by bands of rows
//!   ([`BAND_ROWS`]): memory is `N × BAND_ROWS × width × 2` bytes for the
//!   band plus the master itself (24 MB for 4056×3040), never the N files.
//! - **Hot pixel map**: pixels of the master above `median + k × MAD`, where
//!   the median and the MAD (median absolute deviation, not scaled) are taken
//!   over the whole master; the MAD is floored at 1 ADU so that a perfectly
//!   flat master does not flag every pixel above the median.
//!
//! The original DNGs are only read. Outputs, in `darks/master/`:
//! - `master_dark_ISO<iso>_<t>s_T<temp>.fits`: 16-bit FITS (see `core::fits`)
//!   in raw sensor units (black level included, as in the DNG), rows stored
//!   bottom-up with `ROWORDER = 'BOTTOM-UP'` and `BAYERPAT` taken from the
//!   top row of the DNG. This is the convention of Siril when it opens a raw
//!   file through LibRaw (`readraw_in_cfa`, src/io/image_formats_libraries.c,
//!   checked in November 2026), so the master lines up with lights that Siril
//!   converts from the DNGs. To be validated in Siril with real files.
//! - `hot_pixels_ISO<iso>_<t>s_T<temp>.txt`: documented text format, see
//!   [`write_hot_pixel_map`]. It is not the Siril "bad pixel map" format.
//!
//! `<temp>` is the sensor temperature rounded to 5 °C (`T35`, `T-5`), or
//! `Tinconnue` when no temperature is known.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::dng::{DngError, DngReader};
use crate::core::fits::{Card, FitsU16Writer};

/// Rows read from every file at a time.
pub const BAND_ROWS: u32 = 32;
/// Default `k` of the hot pixel threshold (`median + k × MAD`).
pub const DEFAULT_HOT_K: f64 = 6.0;
/// Hot pixels listed at most (a dark taken with the cap off would list
/// millions); the count in the header stays exact.
pub const MAX_LISTED_HOT_PIXELS: usize = 200_000;

/// Settings of the dark series.
#[derive(Debug, Clone)]
pub struct MasterDarkParams {
    pub iso: u32,
    pub shutter_us: u64,
    /// Sensor temperature (°C), mean of the series.
    pub temp_c: Option<f64>,
    /// Where `temp_c` comes from (`"capteur"` or `"processeur"`).
    pub temp_source: Option<&'static str>,
    pub hot_k: f64,
    /// Start of the series (FITS `DATE-OBS`, UTC).
    pub date_obs: crate::core::time::Timestamp,
    pub instrument: String,
}

/// What was written.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MasterDarkResult {
    pub fits: PathBuf,
    pub hot_pixels_file: PathBuf,
    pub frames: usize,
    pub width: u32,
    pub height: u32,
    pub median: u16,
    pub mad: u16,
    pub threshold: f64,
    pub hot_pixels: u64,
}

/// `5.0s`, the exposure as written in the dark file names.
pub fn shutter_label(shutter_us: u64) -> String {
    format!("{:.1}s", shutter_us as f64 / 1e6)
}

/// Temperature rounded to 5 °C, as used in the file names.
pub fn temp_label(temp_c: Option<f64>) -> String {
    match temp_c.filter(|t| t.is_finite()) {
        Some(t) => format!("T{}", ((t / 5.0).round() * 5.0) as i64),
        None => "Tinconnue".into(),
    }
}

/// Base name shared by the master and the hot pixel map.
pub fn base_name(iso: u32, shutter_us: u64, temp_c: Option<f64>) -> String {
    format!("ISO{}_{}_{}", iso, shutter_label(shutter_us), temp_label(temp_c))
}

fn median_of(v: &mut [u16]) -> u16 {
    let n = v.len();
    let mid = n / 2;
    let (_, &mut hi, _) = v.select_nth_unstable(mid);
    if n % 2 == 1 {
        hi
    } else {
        let lo = *v[..mid].iter().max().expect("n >= 2");
        ((lo as u32 + hi as u32).div_ceil(2)) as u16
    }
}

/// Median of a 16-bit histogram (lower median).
fn histogram_median(hist: &[u64], total: u64) -> u16 {
    let target = total.div_ceil(2);
    let mut acc = 0;
    for (v, &c) in hist.iter().enumerate() {
        acc += c;
        if acc >= target {
            return v as u16;
        }
    }
    0
}

/// Pixel-wise median of the DNGs (all of the same size). `progress` gets
/// `(rows done, rows total)` after each band.
pub fn median_stack(inputs: &[PathBuf], mut progress: impl FnMut(u32, u32)) -> Result<(crate::core::dng::DngInfo, Vec<u16>), String> {
    if inputs.is_empty() {
        return Err("aucun dark".into());
    }
    let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut readers = Vec::with_capacity(inputs.len());
    for p in inputs {
        let r = DngReader::open(p).map_err(|e: DngError| format!("{} : {}", name(p), e))?;
        readers.push(r);
    }
    let info = readers[0].info().clone();
    for (r, p) in readers.iter().zip(inputs) {
        let i = r.info();
        if (i.width, i.height, &i.cfa_pattern) != (info.width, info.height, &info.cfa_pattern) {
            return Err(format!("{} : taille ou matrice de Bayer différente du premier dark", name(p)));
        }
    }
    let (w, h) = (info.width as usize, info.height);
    let n = readers.len();
    let mut master = vec![0u16; w * h as usize];
    let mut bands: Vec<Vec<u16>> = (0..n).map(|_| vec![0u16; w * BAND_ROWS as usize]).collect();
    let mut y0 = 0;
    while y0 < h {
        let rows = BAND_ROWS.min(h - y0);
        for ((r, band), p) in readers.iter_mut().zip(bands.iter_mut()).zip(inputs) {
            r.read_rows(y0, rows, band).map_err(|e| format!("{} : {}", name(p), e))?;
        }
        median_band(&bands, &mut master[y0 as usize * w..(y0 + rows) as usize * w]);
        y0 += rows;
        progress(y0, h);
    }
    Ok((info, master))
}

/// Pixel-wise median of the same band read from every file: `out[i]` is
/// the median of `bands[k][i]` over `k` (`out` may be shorter than a band).
pub fn median_band(bands: &[Vec<u16>], out: &mut [u16]) {
    let mut values = vec![0u16; bands.len()];
    for (i, px) in out.iter_mut().enumerate() {
        for (v, band) in values.iter_mut().zip(bands) {
            *v = band[i];
        }
        *px = median_of(&mut values);
    }
}

/// Global median, MAD (floored at 1) and hot pixel threshold of a master.
pub fn hot_threshold(master: &[u16], k: f64) -> (u16, u16, f64) {
    let mut hist = vec![0u64; 65536];
    for &v in master {
        hist[v as usize] += 1;
    }
    let total = master.len() as u64;
    let median = histogram_median(&hist, total);
    let mut dev = vec![0u64; 65536];
    for (v, &c) in hist.iter().enumerate() {
        if c > 0 {
            dev[(v as i32 - median as i32).unsigned_abs() as usize] += c;
        }
    }
    let mad = histogram_median(&dev, total).max(1);
    (median, mad, median as f64 + k * mad as f64)
}

/// Hot pixel map, plain text:
///
/// ```text
/// # Aurion hot pixel map
/// # ... lines starting with '#' are comments ...
/// x y value
/// 1203 77 1874
/// ```
///
/// `x` is the column and `y` the row of the pixel in the DNG, from the top
/// left corner (0, 0), in CFA coordinates (one value per photosite, before
/// demosaicing). In the FITS master, stored bottom-up, the same pixel is in
/// stored row `height - 1 - y`. `value` is the master dark value in ADU.
pub fn write_hot_pixel_map(out: &mut impl Write, master: &[u16], width: u32, height: u32, k: f64, source: &str) -> std::io::Result<u64> {
    let (median, mad, threshold) = hot_threshold(master, k);
    let hot: Vec<(usize, u16)> = master.iter().copied().enumerate().filter(|&(_, v)| v as f64 > threshold).collect();
    writeln!(out, "# Aurion hot pixel map")?;
    writeln!(out, "# source: {}", source)?;
    writeln!(out, "# size: {} x {}", width, height)?;
    writeln!(out, "# rule: value > median + k * MAD (MAD not scaled, at least 1 ADU)")?;
    writeln!(out, "# median: {} ADU, MAD: {} ADU, k: {}, threshold: {:.1} ADU", median, mad, k, threshold)?;
    writeln!(out, "# hot pixels: {}", hot.len())?;
    if hot.len() > MAX_LISTED_HOT_PIXELS {
        writeln!(out, "# list truncated to the {} first pixels (row order)", MAX_LISTED_HOT_PIXELS)?;
    }
    writeln!(out, "# x = column, y = row from the top left corner of the DNG (CFA coordinates)")?;
    writeln!(out, "# in the FITS master (rows stored bottom-up) the row is height - 1 - y")?;
    writeln!(out, "x y value")?;
    for &(i, v) in hot.iter().take(MAX_LISTED_HOT_PIXELS) {
        writeln!(out, "{} {} {}", i % width as usize, i / width as usize, v)?;
    }
    Ok(hot.len() as u64)
}

/// Write `data` through `f` to `path` atomically (temporary file + rename).
fn write_file(path: &Path, f: impl FnOnce(&mut std::io::BufWriter<std::fs::File>) -> Result<(), String>) -> Result<(), String> {
    let tmp = path.with_extension("part");
    let res = (|| {
        let file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let mut w = std::io::BufWriter::new(file);
        f(&mut w)?;
        let file = w.into_inner().map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// Compute and write the master dark and the hot pixel map of a series.
/// Blocking (minutes of CPU on a Pi): run it with `spawn_blocking`.
pub fn build(inputs: &[PathBuf], out_dir: &Path, p: &MasterDarkParams, progress: impl FnMut(u32, u32)) -> Result<MasterDarkResult, String> {
    let (info, master) = median_stack(inputs, progress)?;
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let base = base_name(p.iso, p.shutter_us, p.temp_c);
    let fits_path = out_dir.join(format!("master_dark_{}.fits", base));
    let hot_path = out_dir.join(format!("hot_pixels_{}.txt", base));
    let (w, h) = (info.width, info.height);

    let mut cards = vec![
        Card::str("IMAGETYP", "Master Dark", "Type of image"),
        Card::float("EXPTIME", p.shutter_us as f64 / 1e6, "[s] Exposure time of each dark"),
        Card::float("ISOSPEED", p.iso as f64, "ISO setting (analogue gain x 100)"),
        Card::str("DATE-OBS", &crate::core::fits::date_obs(p.date_obs), "UTC start of the dark series"),
        Card::str("INSTRUME", &p.instrument, "Camera"),
        Card::int("STACKCNT", inputs.len() as i64, "Number of darks (pixel-wise median)"),
        crate::core::fits::row_order_bottom_up(),
    ];
    if let Some(t) = p.temp_c {
        cards.push(Card::float("CCD-TEMP", (t * 10.0).round() / 10.0, "[degC] mean temperature of the series"));
    }
    if let Some(bayer) = info.bayer_pattern() {
        cards.push(Card::str("BAYERPAT", &bayer, "Bayer pattern of the top row of the sensor"));
    }
    cards.push(Card::comment("Raw sensor values (ADU), black level included, as in the DNG files."));
    cards.push(Card::comment(&format!(
        "DNG black level {:.1}, white level {}, {}.",
        info.black_level_mean(),
        info.white_level.map(|v| v.to_string()).unwrap_or_else(|| "unknown".into()),
        match p.temp_source {
            Some(src) => format!("temperature: {}", src),
            None => "temperature unknown".to_string(),
        }
    )));
    cards.push(Card::comment("Made by Aurion from the files of darks/ (originals untouched)."));

    write_file(&fits_path, |out| {
        let mut fw = FitsU16Writer::new(out, w, h, &cards).map_err(|e| e.to_string())?;
        for y in (0..h as usize).rev() {
            fw.write_row(&master[y * w as usize..(y + 1) * w as usize]).map_err(|e| e.to_string())?;
        }
        fw.finish().map(|_| ()).map_err(|e| e.to_string())
    })?;
    let source = fits_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut hot_pixels = 0;
    write_file(&hot_path, |out| {
        hot_pixels = write_hot_pixel_map(out, &master, w, h, p.hot_k, &source).map_err(|e| e.to_string())?;
        Ok(())
    })?;
    let (median, mad, threshold) = hot_threshold(&master, p.hot_k);
    Ok(MasterDarkResult { fits: fits_path, hot_pixels_file: hot_path, frames: inputs.len(), width: w, height: h, median, mad, threshold, hot_pixels })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::denoise::XorShift;
    use crate::core::dng::{test_dng_bytes, TestDngOptions};

    #[test]
    fn names() {
        assert_eq!(base_name(800, 5_000_000, Some(36.0)), "ISO800_5.0s_T35");
        assert_eq!(base_name(1600, 8_000_000, Some(38.0)), "ISO1600_8.0s_T40");
        assert_eq!(temp_label(Some(-3.0)), "T-5");
        assert_eq!(temp_label(Some(-2.0)), "T0");
        assert_eq!(temp_label(None), "Tinconnue");
        assert!(crate::core::validate::is_safe_name(&format!("master_dark_{}.fits", base_name(800, 5_000_000, Some(-12.0)))));
    }

    #[test]
    fn median_helpers() {
        assert_eq!(median_of(&mut [5, 1, 3]), 3);
        assert_eq!(median_of(&mut [4, 1, 3, 2]), 3, "(2 + 3) / 2 rounded up");
        assert_eq!(median_of(&mut [9]), 9);
        let (m, mad, t) = hot_threshold(&[10, 10, 10, 10, 50], 6.0);
        assert_eq!((m, mad), (10, 1), "flat master: MAD floored at 1");
        assert_eq!(t, 16.0);
    }

    /// Series of synthetic darks: black level + noise, a few hot pixels at
    /// fixed positions, one cosmic ray on a single frame.
    fn write_series(dir: &Path, n: usize, w: u32, h: u32) -> (Vec<PathBuf>, Vec<(u32, u32)>) {
        let hot = vec![(3, 4), (40, 30), (63, 47)];
        let mut paths = Vec::new();
        for k in 0..n {
            let mut rng = XorShift::new(1 + k as u64);
            let mut px: Vec<u16> = (0..w * h).map(|_| (256.0 + 2.0 * rng.gaussian()).round() as u16).collect();
            for &(x, y) in &hot {
                px[(y * w + x) as usize] = 1500;
            }
            if k == 1 {
                px[(10 * w + 10) as usize] = 4095; // cosmic ray on one frame
            }
            let p = dir.join(format!("dark_{:02}.dng", k));
            std::fs::write(&p, test_dng_bytes(w, h, &px, &TestDngOptions { rows_per_strip: 3, ..Default::default() })).unwrap();
            paths.push(p);
        }
        (paths, hot)
    }

    #[test]
    fn master_dark_and_hot_pixels_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let (w, h) = (64u32, 48u32);
        let (paths, hot) = write_series(dir.path(), 5, w, h);
        let before: Vec<Vec<u8>> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let params = MasterDarkParams {
            iso: 800,
            shutter_us: 5_000_000,
            temp_c: Some(36.2),
            temp_source: Some("capteur"),
            hot_k: DEFAULT_HOT_K,
            date_obs: crate::core::time::Timestamp::from_second(1_772_000_000).unwrap(),
            instrument: "Raspberry Pi HQ Camera (IMX477)".into(),
        };
        let mut calls = Vec::new();
        let out = dir.path().join("master");
        let r = build(&paths, &out, &params, |d, t| calls.push((d, t))).unwrap();
        assert_eq!(calls.last(), Some(&(h, h)));
        assert_eq!(calls.len(), h.div_ceil(BAND_ROWS) as usize);
        assert_eq!(r.fits, out.join("master_dark_ISO800_5.0s_T35.fits"));
        assert_eq!(r.hot_pixels_file, out.join("hot_pixels_ISO800_5.0s_T35.txt"));
        assert_eq!(r.median, 256);
        assert_eq!(r.hot_pixels, 3, "the cosmic ray of one frame is not a hot pixel");
        // Originals untouched
        for (p, b) in paths.iter().zip(&before) {
            assert_eq!(&std::fs::read(p).unwrap(), b);
        }

        // FITS: header, size, bottom-up rows
        let fits = std::fs::read(&r.fits).unwrap();
        assert_eq!(fits.len() % 2880, 0);
        let header = String::from_utf8_lossy(&fits[..2880]).to_string();
        for key in ["EXPTIME =", "ISOSPEED=", "CCD-TEMP=                 36.2", "DATE-OBS= '2026-02-25T06:13:20'",
                    "BAYERPAT= 'RGGB    '", "INSTRUME= 'Raspberry Pi HQ Camera (IMX477)'", "ROWORDER= 'BOTTOM-UP'", "STACKCNT="] {
            assert!(header.contains(key), "missing {} in header", key);
        }
        let data = &fits[2880..];
        let at = |sx: u32, sy: u32| {
            let i = 2 * (sy * w + sx) as usize;
            (i16::from_be_bytes([data[i], data[i + 1]]) as i32 + 32768) as u16
        };
        for &(x, y) in &hot {
            assert_eq!(at(x, h - 1 - y), 1500, "hot pixel ({}, {}) in the bottom-up FITS", x, y);
        }
        assert!(at(10, h - 1 - 10) < 300, "median removes the cosmic ray");

        // Hot pixel map
        let text = std::fs::read_to_string(&r.hot_pixels_file).unwrap();
        let listed: Vec<(u32, u32)> = text
            .lines()
            .filter(|l| !l.starts_with('#') && *l != "x y value")
            .map(|l| {
                let v: Vec<u32> = l.split(' ').map(|s| s.parse().unwrap()).collect();
                assert_eq!(v[2], 1500);
                (v[0], v[1])
            })
            .collect();
        assert_eq!(listed, hot);
        assert!(text.contains("# source: master_dark_ISO800_5.0s_T35.fits"));
    }

    #[test]
    fn refuses_mixed_or_broken_series() {
        let dir = tempfile::tempdir().unwrap();
        let (mut paths, _) = write_series(dir.path(), 2, 64, 48);
        let other = dir.path().join("other.dng");
        std::fs::write(&other, test_dng_bytes(16, 16, &[0; 256], &TestDngOptions::default())).unwrap();
        paths.push(other);
        assert!(median_stack(&paths, |_, _| {}).unwrap_err().contains("other.dng"));
        let broken = dir.path().join("broken.dng");
        std::fs::write(&broken, b"DNG").unwrap();
        assert!(median_stack(&[broken], |_, _| {}).unwrap_err().contains("broken.dng"));
        assert!(median_stack(&[], |_, _| {}).is_err());
    }
}
