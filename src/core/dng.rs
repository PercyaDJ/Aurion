//! Minimal DNG (TIFF) reader for the files written by `rpicam-still --raw`.
//!
//! Layout written by rpicam-apps (`image/dng.cpp`, checked in November 2026):
//! - IFD0: small 8-bit RGB thumbnail (width/16 × height/16), with a
//!   `SubIFDs` tag (330) pointing to the raw image and an EXIF IFD;
//! - the raw IFD (`NewSubfileType` 0): `PhotometricInterpretation` 32803
//!   (CFA), `BitsPerSample` 16 (12-bit values from the HQ camera stored in
//!   16-bit samples), `SamplesPerPixel` 1, no compression, one scanline at a
//!   time through libtiff (`TIFFWriteScanline`), so the image is split in
//!   strips (`StripOffsets` / `StripByteCounts`, `RowsPerStrip` chosen by
//!   libtiff);
//! - `WhiteLevel` = 2^bits − 1, `BlackLevelRepeatDim` 2×2 and four
//!   `BlackLevel` values, `CFARepeatPatternDim` 2×2 and `CFAPattern` with
//!   0 = red, 1 = green, 2 = blue.
//!
//! The reader does not depend on these choices beyond what TIFF requires: it
//! walks IFD0, its chain and its SubIFDs, takes the first CFA image, accepts
//! any numeric TIFF type for every tag, either byte order, and reads rows on
//! demand with `seek` ([`DngReader::read_rows`]): a 24 MB file is never
//! loaded at once. Compressed, tiled or packed images are refused with a
//! clear error (the Pi 5 compressed raw format is decompressed by
//! rpicam-apps before it writes the DNG).
//!
//! To be validated on the hardware: a real DNG from the HQ camera on a Pi 4
//! and on a Pi 5 (the self-test reads it, see `web::selftest`).

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// TIFF/EP `PhotometricInterpretation` for colour filter array data.
pub const PHOTOMETRIC_CFA: u32 = 32803;

const TAG_NEW_SUBFILE_TYPE: u16 = 254;
const TAG_WIDTH: u16 = 256;
const TAG_HEIGHT: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_PLANAR_CONFIG: u16 = 284;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_SUB_IFDS: u16 = 330;
const TAG_CFA_REPEAT_PATTERN_DIM: u16 = 33421;
const TAG_CFA_PATTERN: u16 = 33422;
const TAG_BLACK_LEVEL_REPEAT_DIM: u16 = 50713;
const TAG_BLACK_LEVEL: u16 = 50714;
const TAG_WHITE_LEVEL: u16 = 50717;

/// Upper bounds against corrupt files (a 4056×3040 frame needs far less).
const MAX_IFDS: usize = 32;
const MAX_VALUES: u32 = 1 << 20;
const MAX_SIDE: u32 = 1 << 16;

#[derive(Debug, thiserror::Error)]
pub enum DngError {
    #[error("lecture du DNG impossible : {0}")]
    Io(#[from] std::io::Error),
    #[error("DNG invalide : {0}")]
    Invalid(String),
    #[error("DNG non pris en charge : {0}")]
    Unsupported(String),
}

fn invalid(msg: impl Into<String>) -> DngError {
    DngError::Invalid(msg.into())
}

/// Description of the CFA image of a DNG.
#[derive(Debug, Clone, PartialEq)]
pub struct DngInfo {
    pub width: u32,
    pub height: u32,
    pub bits_per_sample: u16,
    /// Black level per position of the `black_level_repeat` pattern
    /// (row-major), in the units of the samples.
    pub black_level: Vec<f64>,
    pub black_level_repeat: (u16, u16),
    pub white_level: Option<u32>,
    /// Colour of each cell of the `cfa_repeat` pattern (row-major),
    /// 0 = red, 1 = green, 2 = blue.
    pub cfa_pattern: Vec<u8>,
    pub cfa_repeat: (u16, u16),
    little_endian: bool,
    rows_per_strip: u32,
    strip_offsets: Vec<u64>,
    strip_byte_counts: Vec<u64>,
}

impl DngInfo {
    /// Bayer pattern as a string (`"RGGB"`, `"BGGR"`...) seen from the top
    /// row of the image, None when it is not a 2×2 RGB pattern.
    pub fn bayer_pattern(&self) -> Option<String> {
        if self.cfa_repeat != (2, 2) || self.cfa_pattern.len() != 4 {
            return None;
        }
        self.cfa_pattern
            .iter()
            .map(|c| match c {
                0 => Some('R'),
                1 => Some('G'),
                2 => Some('B'),
                _ => None,
            })
            .collect()
    }

    /// Mean of the black levels (0 when the tag is absent).
    pub fn black_level_mean(&self) -> f64 {
        if self.black_level.is_empty() {
            0.0
        } else {
            self.black_level.iter().sum::<f64>() / self.black_level.len() as f64
        }
    }

    fn row_bytes(&self) -> u64 {
        self.width as u64 * (self.bits_per_sample as u64 / 8)
    }
}

/// Reader of the CFA image of a DNG, row band by row band.
pub struct DngReader<R> {
    reader: R,
    info: DngInfo,
    row_buf: Vec<u8>,
}

impl DngReader<BufReader<File>> {
    pub fn open(path: &Path) -> Result<Self, DngError> {
        Self::new(BufReader::new(File::open(path)?))
    }
}

impl<R: Read + Seek> DngReader<R> {
    /// Parse the TIFF structure (only the directories are read).
    pub fn new(mut reader: R) -> Result<Self, DngError> {
        let info = parse_structure(&mut reader)?;
        let row_buf = vec![0; info.row_bytes() as usize];
        Ok(Self { reader, info, row_buf })
    }

    pub fn info(&self) -> &DngInfo {
        &self.info
    }

    /// Read `rows` rows starting at `y0` into `out` (`rows × width` samples).
    pub fn read_rows(&mut self, y0: u32, rows: u32, out: &mut [u16]) -> Result<(), DngError> {
        let (w, h) = (self.info.width as usize, self.info.height);
        if y0.checked_add(rows).is_none_or(|end| end > h) {
            return Err(invalid(format!("lignes {}..{} hors de l'image ({} lignes)", y0, y0 as u64 + rows as u64, h)));
        }
        if out.len() < rows as usize * w {
            return Err(invalid("tampon trop petit"));
        }
        let row_bytes = self.info.row_bytes();
        let rps = self.info.rows_per_strip;
        for r in 0..rows {
            let y = y0 + r;
            let strip = (y / rps) as usize;
            let within = (y % rps) as u64 * row_bytes;
            let (Some(&start), Some(&count)) = (self.info.strip_offsets.get(strip), self.info.strip_byte_counts.get(strip)) else {
                return Err(invalid(format!("bande {} absente", strip)));
            };
            if within + row_bytes > count {
                return Err(invalid(format!("bande {} trop courte ({} octets)", strip, count)));
            }
            self.reader.seek(SeekFrom::Start(start + within))?;
            self.reader.read_exact(&mut self.row_buf)?;
            let dst = &mut out[r as usize * w..(r as usize + 1) * w];
            if self.info.bits_per_sample == 8 {
                for (d, &b) in dst.iter_mut().zip(&self.row_buf) {
                    *d = b as u16;
                }
            } else {
                for (d, b) in dst.iter_mut().zip(self.row_buf.chunks_exact(2)) {
                    let pair = [b[0], b[1]];
                    *d = if self.info.little_endian { u16::from_le_bytes(pair) } else { u16::from_be_bytes(pair) };
                }
            }
        }
        Ok(())
    }
}

/// One IFD entry, values decoded as numbers.
struct Entry {
    tag: u16,
    values: Vec<f64>,
}

struct Ifd {
    entries: Vec<Entry>,
    next: u32,
}

impl Ifd {
    fn get(&self, tag: u16) -> Option<&[f64]> {
        self.entries.iter().find(|e| e.tag == tag).map(|e| e.values.as_slice())
    }
    fn first(&self, tag: u16) -> Option<f64> {
        self.get(tag).and_then(|v| v.first().copied())
    }
}

struct Tiff<'a, R> {
    r: &'a mut R,
    le: bool,
}

impl<R: Read + Seek> Tiff<'_, R> {
    fn u16(&mut self) -> Result<u16, DngError> {
        let mut b = [0; 2];
        self.r.read_exact(&mut b)?;
        Ok(if self.le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }
    fn u32(&mut self) -> Result<u32, DngError> {
        let mut b = [0; 4];
        self.r.read_exact(&mut b)?;
        Ok(if self.le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }

    fn read_ifd(&mut self, offset: u32) -> Result<Ifd, DngError> {
        self.r.seek(SeekFrom::Start(offset as u64))?;
        let n = self.u16()?;
        let mut raw = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let tag = self.u16()?;
            let typ = self.u16()?;
            let count = self.u32()?;
            let mut field = [0u8; 4];
            self.r.read_exact(&mut field)?;
            raw.push((tag, typ, count, field));
        }
        let next = self.u32()?;
        let mut entries = Vec::with_capacity(raw.len());
        for (tag, typ, count, field) in raw {
            if let Some(values) = self.decode(typ, count, field)? {
                entries.push(Entry { tag, values });
            }
        }
        Ok(Ifd { entries, next })
    }

    /// Values of one entry, None for types that are not numbers.
    fn decode(&mut self, typ: u16, count: u32, field: [u8; 4]) -> Result<Option<Vec<f64>>, DngError> {
        let size: u32 = match typ {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => return Ok(None),
        };
        if typ == 2 || count > MAX_VALUES {
            return Ok(None); // ASCII, or absurd count: not needed here
        }
        let total = size as u64 * count as u64;
        let bytes = if total <= 4 {
            field[..total as usize].to_vec()
        } else {
            let off = if self.le { u32::from_le_bytes(field) } else { u32::from_be_bytes(field) };
            self.r.seek(SeekFrom::Start(off as u64))?;
            let mut b = vec![0; total as usize];
            self.r.read_exact(&mut b)?;
            b
        };
        let le = self.le;
        let u16_at = |c: &[u8]| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) };
        let u32_at = |c: &[u8]| {
            let a = [c[0], c[1], c[2], c[3]];
            if le { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) }
        };
        let values = bytes
            .chunks_exact(size as usize)
            .map(|c| match typ {
                1 | 7 => c[0] as f64,
                6 => c[0] as i8 as f64,
                3 => u16_at(c) as f64,
                8 => u16_at(c) as i16 as f64,
                4 | 13 => u32_at(c) as f64,
                9 => u32_at(c) as i32 as f64,
                11 => f32::from_bits(u32_at(c)) as f64,
                5 => ratio(u32_at(c) as f64, u32_at(&c[4..]) as f64),
                10 => ratio(u32_at(c) as i32 as f64, u32_at(&c[4..]) as i32 as f64),
                _ => {
                    let a = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]];
                    if le { f64::from_le_bytes(a) } else { f64::from_be_bytes(a) }
                }
            })
            .collect();
        Ok(Some(values))
    }
}

fn ratio(n: f64, d: f64) -> f64 {
    if d == 0.0 { 0.0 } else { n / d }
}

fn parse_structure<R: Read + Seek>(r: &mut R) -> Result<DngInfo, DngError> {
    r.seek(SeekFrom::Start(0))?;
    let mut head = [0u8; 4];
    r.read_exact(&mut head)?;
    let le = match &head {
        b"II*\0" => true,
        b"MM\0*" => false,
        b"II+\0" | b"MM\0+" => return Err(DngError::Unsupported("BigTIFF".into())),
        _ => return Err(invalid("en-tête TIFF absent")),
    };
    let mut t = Tiff { r, le };
    let first = t.u32()?;

    // Breadth-first walk: IFD0, its SubIFDs, then the next IFDs of the chain
    let mut queue = std::collections::VecDeque::from([first]);
    let mut seen = Vec::new();
    while let Some(off) = queue.pop_front() {
        if off == 0 || seen.contains(&off) {
            continue;
        }
        if seen.len() >= MAX_IFDS {
            break;
        }
        seen.push(off);
        let ifd = t.read_ifd(off)?;
        let is_cfa = ifd.first(TAG_PHOTOMETRIC) == Some(PHOTOMETRIC_CFA as f64);
        let is_main = ifd.first(TAG_NEW_SUBFILE_TYPE).unwrap_or(0.0) as u32 & 1 == 0;
        if is_cfa && is_main {
            return cfa_info(&ifd, le);
        }
        if let Some(subs) = ifd.get(TAG_SUB_IFDS) {
            queue.extend(subs.iter().map(|&s| s as u32));
        }
        queue.push_back(ifd.next);
    }
    Err(invalid("aucune image CFA (PhotometricInterpretation 32803) trouvée"))
}

fn cfa_info(ifd: &Ifd, little_endian: bool) -> Result<DngInfo, DngError> {
    let dim = |tag: u16, name: &str| -> Result<u32, DngError> {
        let v = ifd.first(tag).ok_or_else(|| invalid(format!("{} absent", name)))?;
        if v < 1.0 || v > MAX_SIDE as f64 {
            return Err(invalid(format!("{} = {}", name, v)));
        }
        Ok(v as u32)
    };
    let width = dim(TAG_WIDTH, "ImageWidth")?;
    let height = dim(TAG_HEIGHT, "ImageLength")?;
    let bits = ifd.first(TAG_BITS_PER_SAMPLE).unwrap_or(1.0) as u16;
    if bits != 8 && bits != 16 {
        return Err(DngError::Unsupported(format!("{} bits par échantillon (8 ou 16 attendus)", bits)));
    }
    let compression = ifd.first(TAG_COMPRESSION).unwrap_or(1.0) as u32;
    if compression != 1 {
        return Err(DngError::Unsupported(format!("compression {}", compression)));
    }
    if ifd.get(TAG_TILE_WIDTH).is_some() {
        return Err(DngError::Unsupported("image en tuiles".into()));
    }
    let spp = ifd.first(TAG_SAMPLES_PER_PIXEL).unwrap_or(1.0) as u32;
    let planar = ifd.first(TAG_PLANAR_CONFIG).unwrap_or(1.0) as u32;
    if spp != 1 || planar != 1 {
        return Err(DngError::Unsupported(format!("{} échantillons par pixel, configuration {}", spp, planar)));
    }
    let to_u64 = |v: &[f64]| v.iter().map(|&x| x as u64).collect::<Vec<_>>();
    let strip_offsets = to_u64(ifd.get(TAG_STRIP_OFFSETS).ok_or_else(|| invalid("StripOffsets absent"))?);
    let strip_byte_counts = to_u64(ifd.get(TAG_STRIP_BYTE_COUNTS).ok_or_else(|| invalid("StripByteCounts absent"))?);
    let rows_per_strip = (ifd.first(TAG_ROWS_PER_STRIP).unwrap_or(height as f64).min(height as f64) as u32).max(1);
    let strips = height.div_ceil(rows_per_strip) as usize;
    if strip_offsets.len() < strips || strip_byte_counts.len() < strips {
        return Err(invalid(format!("{} bandes attendues, {} décrites", strips, strip_offsets.len())));
    }
    let pair = |tag: u16| ifd.get(tag).filter(|v| v.len() >= 2).map(|v| (v[0] as u16, v[1] as u16));
    let cfa_repeat = pair(TAG_CFA_REPEAT_PATTERN_DIM).unwrap_or((2, 2));
    let cfa_pattern = ifd.get(TAG_CFA_PATTERN).map(|v| v.iter().map(|&c| c as u8).collect()).unwrap_or_default();
    Ok(DngInfo {
        width,
        height,
        bits_per_sample: bits,
        black_level: ifd.get(TAG_BLACK_LEVEL).map(|v| v.to_vec()).unwrap_or_default(),
        black_level_repeat: pair(TAG_BLACK_LEVEL_REPEAT_DIM).unwrap_or((1, 1)),
        white_level: ifd.first(TAG_WHITE_LEVEL).map(|v| v as u32),
        cfa_pattern,
        cfa_repeat,
        little_endian,
        rows_per_strip,
        strip_offsets,
        strip_byte_counts,
    })
}

// ─── Test writer ────────────────────────────────────────────

/// Options of [`write_test_dng`].
#[derive(Debug, Clone)]
pub struct TestDngOptions {
    /// 0 = red, 1 = green, 2 = blue (rpicam-apps: RGGB = [0, 1, 1, 2]).
    pub cfa_pattern: [u8; 4],
    pub black_level: [f64; 4],
    pub white_level: u32,
    pub rows_per_strip: u32,
    pub big_endian: bool,
}

impl Default for TestDngOptions {
    fn default() -> Self {
        // Values of a 12-bit HQ camera frame: black 4096/65536 × 4096 = 256
        Self { cfa_pattern: [0, 1, 1, 2], black_level: [256.0; 4], white_level: 4095, rows_per_strip: 1, big_endian: false }
    }
}

/// Write a small DNG with the layout of rpicam-apps (thumbnail in IFD0, CFA
/// image in a SubIFD, BlackLevel as RATIONAL). Used by the tests and by the
/// PC mock camera; not meant to produce files for other software.
pub fn write_test_dng<W: Write>(out: &mut W, width: u32, height: u32, pixels: &[u16], opt: &TestDngOptions) -> std::io::Result<()> {
    assert_eq!(pixels.len(), (width * height) as usize, "pixel count");
    let be = opt.big_endian;
    let p16 = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let p32 = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };

    let (tw, th) = ((width / 16).max(1), (height / 16).max(1));
    let rps = opt.rows_per_strip.clamp(1, height);
    let strips = height.div_ceil(rps);

    // File: header | thumbnail pixels | raw pixels | IFD0 | raw IFD | extra values
    let thumb_off = 8u32;
    let thumb_len = tw * th * 3;
    let raw_off = thumb_off + thumb_len;
    let raw_len = width * height * 2;
    let ifd0_entries = 11u32;
    let ifd0_off = raw_off + raw_len;
    let ifd0_len = 2 + ifd0_entries * 12 + 4;
    let raw_entries = 15u32;
    let raw_ifd_off = ifd0_off + ifd0_len;
    let raw_ifd_len = 2 + raw_entries * 12 + 4;
    let mut extra: Vec<u8> = Vec::new();
    let extra_base = raw_ifd_off + raw_ifd_len;

    // (tag, type, count, inline bytes or bytes stored after the IFDs)
    type Field = (u16, u16, u32, Vec<u8>);
    let place = |extra: &mut Vec<u8>, f: Field| -> [u8; 12] {
        let (tag, typ, count, data) = f;
        let mut e = [0u8; 12];
        e[..2].copy_from_slice(&p16(tag));
        e[2..4].copy_from_slice(&p16(typ));
        e[4..8].copy_from_slice(&p32(count));
        if data.len() <= 4 {
            e[8..8 + data.len()].copy_from_slice(&data);
        } else {
            if extra.len() % 2 == 1 {
                extra.push(0);
            }
            e[8..12].copy_from_slice(&p32(extra_base + extra.len() as u32));
            extra.extend_from_slice(&data);
        }
        e
    };
    let short = |v: u16| p16(v).to_vec();
    let long = |v: u32| p32(v).to_vec();
    let shorts = |v: &[u16]| v.iter().flat_map(|&x| p16(x)).collect::<Vec<u8>>();
    let longs = |v: &[u32]| v.iter().flat_map(|&x| p32(x)).collect::<Vec<u8>>();

    let ifd0: Vec<Field> = vec![
        (TAG_NEW_SUBFILE_TYPE, 4, 1, long(1)),
        (TAG_WIDTH, 4, 1, long(tw)),
        (TAG_HEIGHT, 4, 1, long(th)),
        (TAG_BITS_PER_SAMPLE, 3, 3, shorts(&[8, 8, 8])),
        (TAG_COMPRESSION, 3, 1, short(1)),
        (TAG_PHOTOMETRIC, 3, 1, short(2)),
        (TAG_STRIP_OFFSETS, 4, 1, long(thumb_off)),
        (TAG_SAMPLES_PER_PIXEL, 3, 1, short(3)),
        (TAG_ROWS_PER_STRIP, 4, 1, long(th)),
        (TAG_STRIP_BYTE_COUNTS, 4, 1, long(thumb_len)),
        (TAG_SUB_IFDS, 4, 1, long(raw_ifd_off)),
    ];
    let offsets: Vec<u32> = (0..strips).map(|s| raw_off + s * rps * width * 2).collect();
    let counts: Vec<u32> = (0..strips).map(|s| (rps.min(height - s * rps)) * width * 2).collect();
    let rational = |v: f64| {
        let (n, d) = ((v * 1000.0).round() as u32, 1000u32);
        [p32(n), p32(d)].concat()
    };
    let raw: Vec<Field> = vec![
        (TAG_NEW_SUBFILE_TYPE, 4, 1, long(0)),
        (TAG_WIDTH, 4, 1, long(width)),
        (TAG_HEIGHT, 4, 1, long(height)),
        (TAG_BITS_PER_SAMPLE, 3, 1, short(16)),
        (TAG_COMPRESSION, 3, 1, short(1)),
        (TAG_PHOTOMETRIC, 3, 1, short(PHOTOMETRIC_CFA as u16)),
        (TAG_STRIP_OFFSETS, 4, strips, longs(&offsets)),
        (TAG_SAMPLES_PER_PIXEL, 3, 1, short(1)),
        (TAG_ROWS_PER_STRIP, 4, 1, long(rps)),
        (TAG_STRIP_BYTE_COUNTS, 4, strips, longs(&counts)),
        (TAG_CFA_REPEAT_PATTERN_DIM, 3, 2, shorts(&[2, 2])),
        (TAG_CFA_PATTERN, 1, 4, opt.cfa_pattern.to_vec()),
        (TAG_BLACK_LEVEL_REPEAT_DIM, 3, 2, shorts(&[2, 2])),
        (TAG_BLACK_LEVEL, 5, 4, opt.black_level.iter().flat_map(|&b| rational(b)).collect()),
        (TAG_WHITE_LEVEL, 4, 1, long(opt.white_level)),
    ];
    debug_assert_eq!(ifd0.len() as u32, ifd0_entries);
    debug_assert_eq!(raw.len() as u32, raw_entries);

    let mut ifd_bytes = Vec::new();
    for (fields, next) in [(ifd0, 0u32), (raw, 0u32)] {
        ifd_bytes.extend_from_slice(&p16(fields.len() as u16));
        for f in fields {
            let e = place(&mut extra, f);
            ifd_bytes.extend_from_slice(&e);
        }
        ifd_bytes.extend_from_slice(&p32(next));
    }

    out.write_all(if be { b"MM\0*" } else { b"II*\0" })?;
    out.write_all(&p32(ifd0_off))?;
    // Greyscale thumbnail, like rpicam-apps
    let mut thumb = Vec::with_capacity(thumb_len as usize);
    for y in 0..th {
        for x in 0..tw {
            let v = pixels[((y * 16).min(height - 1) * width + (x * 16).min(width - 1)) as usize];
            let g = (v as u32 * 255 / opt.white_level.max(1)).min(255) as u8;
            thumb.extend_from_slice(&[g, g, g]);
        }
    }
    out.write_all(&thumb)?;
    let mut raw_bytes = Vec::with_capacity(raw_len as usize);
    for &v in pixels {
        raw_bytes.extend_from_slice(&p16(v));
    }
    out.write_all(&raw_bytes)?;
    out.write_all(&ifd_bytes)?;
    out.write_all(&extra)?;
    Ok(())
}

/// [`write_test_dng`] into a byte vector.
pub fn test_dng_bytes(width: u32, height: u32, pixels: &[u16], opt: &TestDngOptions) -> Vec<u8> {
    let mut v = Vec::new();
    write_test_dng(&mut v, width, height, pixels, opt).expect("write to memory");
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn ramp(w: u32, h: u32) -> Vec<u16> {
        (0..w * h).map(|i| (i * 7 % 4096) as u16).collect()
    }

    #[test]
    fn reads_back_the_rpicam_layout() {
        let (w, h) = (64, 48);
        let px = ramp(w, h);
        for big_endian in [false, true] {
            for rps in [1, 5, 48] {
                let opt = TestDngOptions { rows_per_strip: rps, big_endian, ..Default::default() };
                let mut r = DngReader::new(Cursor::new(test_dng_bytes(w, h, &px, &opt))).unwrap();
                let info = r.info().clone();
                assert_eq!((info.width, info.height, info.bits_per_sample), (w, h, 16));
                assert_eq!(info.white_level, Some(4095));
                assert_eq!(info.black_level, vec![256.0; 4]);
                assert_eq!(info.black_level_repeat, (2, 2));
                assert_eq!(info.bayer_pattern().as_deref(), Some("RGGB"));
                let mut all = vec![0u16; (w * h) as usize];
                r.read_rows(0, h, &mut all).unwrap();
                assert_eq!(all, px, "rps {} be {}", rps, big_endian);
                // A band in the middle, crossing strip boundaries
                let mut band = vec![0u16; (w * 7) as usize];
                r.read_rows(13, 7, &mut band).unwrap();
                assert_eq!(band, px[(13 * w) as usize..(20 * w) as usize]);
            }
        }
    }

    #[test]
    fn other_bayer_orders() {
        let px = vec![0u16; 16 * 16];
        for (pat, name) in [([2, 1, 1, 0], "BGGR"), ([1, 0, 2, 1], "GRBG"), ([1, 2, 0, 1], "GBRG")] {
            let opt = TestDngOptions { cfa_pattern: pat, ..Default::default() };
            let r = DngReader::new(Cursor::new(test_dng_bytes(16, 16, &px, &opt))).unwrap();
            assert_eq!(r.info().bayer_pattern().as_deref(), Some(name));
        }
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        assert!(matches!(DngReader::new(Cursor::new(b"DNG".to_vec())), Err(DngError::Io(_))));
        assert!(matches!(DngReader::new(Cursor::new(b"GIF89a-not-a-tiff".to_vec())), Err(DngError::Invalid(_))));
        // A TIFF without CFA image (an ordinary RGB TIFF)
        let mut rgb_only = test_dng_bytes(32, 32, &vec![0; 1024], &TestDngOptions::default());
        // Point IFD0's SubIFDs entry to nothing: only the RGB thumbnail stays
        let ifd0_off = u32::from_le_bytes(rgb_only[4..8].try_into().unwrap()) as usize;
        let last_entry = ifd0_off + 2 + 10 * 12;
        rgb_only[last_entry + 8..last_entry + 12].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(DngReader::new(Cursor::new(rgb_only)), Err(DngError::Invalid(_))));
        // Truncated pixel data
        let full = test_dng_bytes(32, 32, &vec![1; 1024], &TestDngOptions::default());
        let mut r = DngReader::new(Cursor::new(full)).unwrap();
        let mut out = vec![0u16; 32 * 4];
        assert!(r.read_rows(30, 4, &mut out).is_err(), "rows past the end");
        assert!(r.read_rows(0, 4, &mut out[..10]).is_err(), "buffer too small");
    }
}
