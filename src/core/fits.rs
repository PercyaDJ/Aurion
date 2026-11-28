//! FITS writer for 16-bit unsigned images (FITS Standard 4.0).
//!
//! FITS has no unsigned 16-bit type: values are stored as signed 16-bit
//! big-endian integers (`BITPIX = 16`) with `BZERO = 32768` and
//! `BSCALE = 1`, so that `physical = stored + 32768` covers 0..65535
//! (FITS 4.0, section 5.2.5). The header is made of 80-character ASCII
//! cards in 2880-byte blocks, padded with spaces; the data is padded with
//! zeros to a multiple of 2880 bytes.
//!
//! Rows are written in the order given by the caller: FITS itself does not
//! fix the orientation. [`row_order_bottom_up`] documents the convention
//! chosen for the master darks (that of Siril, see `core::master_dark`).
//!
//! Also meant for the scientific network (lot 7): any keyword can be added.

use std::io::Write;

pub const BLOCK: usize = 2880;
pub const CARD: usize = 80;

/// Value of a header card.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

/// One header keyword.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub key: String,
    pub value: Option<Value>,
    pub comment: String,
}

impl Card {
    pub fn new(key: &str, value: Value, comment: &str) -> Self {
        Self { key: key.to_string(), value: Some(value), comment: comment.to_string() }
    }
    pub fn str(key: &str, v: &str, comment: &str) -> Self {
        Self::new(key, Value::Str(v.to_string()), comment)
    }
    pub fn int(key: &str, v: i64, comment: &str) -> Self {
        Self::new(key, Value::Int(v), comment)
    }
    pub fn float(key: &str, v: f64, comment: &str) -> Self {
        Self::new(key, Value::Float(v), comment)
    }
    pub fn comment(text: &str) -> Self {
        Self { key: "COMMENT".into(), value: None, comment: text.to_string() }
    }
}

/// `ROWORDER` value for files whose first stored row is the bottom row of
/// the picture (keyword read by Siril).
pub fn row_order_bottom_up() -> Card {
    Card::str("ROWORDER", "BOTTOM-UP", "Order of the rows in image array")
}

#[derive(Debug, thiserror::Error)]
pub enum FitsError {
    #[error("mot-clé FITS invalide : {0}")]
    BadKeyword(String),
    #[error("valeur FITS invalide pour {0}")]
    BadValue(String),
    #[error("taille d'image invalide")]
    BadSize,
    #[error("ligne de {got} pixels au lieu de {expected}")]
    BadRow { got: usize, expected: usize },
    #[error("{0} lignes manquantes")]
    MissingRows(u64),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Keywords that the writer sets itself.
const RESERVED: [&str; 8] = ["SIMPLE", "BITPIX", "NAXIS", "NAXIS1", "NAXIS2", "BZERO", "BSCALE", "END"];

fn printable(s: &str) -> String {
    s.chars().map(|c| if (' '..='~').contains(&c) { c } else { '?' }).collect()
}

fn format_float(v: f64) -> String {
    let a = v.abs();
    if a == 0.0 || (1e-4..1e15).contains(&a) {
        let s = format!("{:.6}", v);
        let s = s.trim_end_matches('0');
        if s.ends_with('.') { format!("{}0", s) } else { s.to_string() }
    } else {
        // Rust writes "1.5E-7": a valid FITS real with an exponent
        format!("{:.9E}", v)
    }
}

/// Format one 80-character card (fixed format of FITS 4.0, section 4.2).
pub fn format_card(card: &Card) -> Result<String, FitsError> {
    let key = card.key.as_str();
    let key_ok = !key.is_empty() && key.len() <= 8 && key.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !key_ok {
        return Err(FitsError::BadKeyword(key.to_string()));
    }
    let mut line = format!("{:<8}", key);
    match &card.value {
        None => {
            // Commentary card: free text from column 9
            line.push_str(&printable(&card.comment));
        }
        Some(value) => {
            line.push_str("= ");
            let v = match value {
                Value::Bool(b) => format!("{:>20}", if *b { "T" } else { "F" }),
                Value::Int(i) => format!("{:>20}", i),
                Value::Float(f) if f.is_finite() => format!("{:>20}", format_float(*f)),
                Value::Float(_) => return Err(FitsError::BadValue(key.to_string())),
                Value::Str(s) => {
                    let inner = printable(s).replace('\'', "''");
                    if inner.len() > 68 {
                        return Err(FitsError::BadValue(key.to_string()));
                    }
                    format!("'{:<8}'", inner)
                }
            };
            line.push_str(&v);
            if !card.comment.is_empty() {
                line.push_str(" / ");
                line.push_str(&printable(&card.comment));
            }
        }
    }
    line.truncate(CARD);
    Ok(format!("{:<80}", line))
}

/// Streaming writer: header first, then one row at a time.
pub struct FitsU16Writer<W: Write> {
    out: W,
    width: usize,
    rows_left: u64,
    data_bytes: u64,
    buf: Vec<u8>,
}

impl<W: Write> FitsU16Writer<W> {
    /// Write the primary header of a `width × height` 16-bit image.
    pub fn new(mut out: W, width: u32, height: u32, cards: &[Card]) -> Result<Self, FitsError> {
        if width == 0 || height == 0 {
            return Err(FitsError::BadSize);
        }
        let mut header = String::new();
        let mandatory = [
            Card::new("SIMPLE", Value::Bool(true), "conforms to FITS standard"),
            Card::int("BITPIX", 16, "16-bit signed integers"),
            Card::int("NAXIS", 2, "number of axes"),
            Card::int("NAXIS1", width as i64, "image width"),
            Card::int("NAXIS2", height as i64, "image height"),
            Card::int("BZERO", 32768, "physical = stored + BZERO (unsigned 16-bit)"),
            Card::int("BSCALE", 1, ""),
        ];
        for c in mandatory.iter() {
            header.push_str(&format_card(c)?);
        }
        for c in cards {
            if RESERVED.contains(&c.key.as_str()) {
                return Err(FitsError::BadKeyword(c.key.clone()));
            }
            header.push_str(&format_card(c)?);
        }
        header.push_str(&format!("{:<80}", "END"));
        while !header.len().is_multiple_of(BLOCK) {
            header.push(' ');
        }
        out.write_all(header.as_bytes())?;
        Ok(Self { out, width: width as usize, rows_left: height as u64, data_bytes: 0, buf: Vec::with_capacity(width as usize * 2) })
    }

    /// Append the next stored row (`width` unsigned values).
    pub fn write_row(&mut self, row: &[u16]) -> Result<(), FitsError> {
        if row.len() != self.width {
            return Err(FitsError::BadRow { got: row.len(), expected: self.width });
        }
        if self.rows_left == 0 {
            return Err(FitsError::BadSize);
        }
        self.buf.clear();
        for &v in row {
            // stored = physical − 32768, i.e. the top bit flipped
            self.buf.extend_from_slice(&(v ^ 0x8000).to_be_bytes());
        }
        self.out.write_all(&self.buf)?;
        self.rows_left -= 1;
        self.data_bytes += self.buf.len() as u64;
        Ok(())
    }

    /// Pad the data unit and return the underlying writer.
    pub fn finish(mut self) -> Result<W, FitsError> {
        if self.rows_left > 0 {
            return Err(FitsError::MissingRows(self.rows_left));
        }
        let pad = (BLOCK as u64 - self.data_bytes % BLOCK as u64) % BLOCK as u64;
        self.out.write_all(&vec![0u8; pad as usize])?;
        self.out.flush()?;
        Ok(self.out)
    }
}

/// UTC date for `DATE-OBS` (`YYYY-MM-DDThh:mm:ss`).
pub fn date_obs(t: crate::core::time::Timestamp) -> String {
    t.to_zoned_datetime(crate::core::time::TimeZone::UTC).strftime("%Y-%m-%dT%H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards_of(bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for chunk in bytes.chunks(CARD) {
            let s = String::from_utf8(chunk.to_vec()).unwrap();
            let end = s.starts_with("END ");
            out.push(s);
            if end {
                break;
            }
        }
        out
    }

    #[test]
    fn header_and_data_follow_the_standard() {
        let cards = [
            Card::float("EXPTIME", 5.0, "[s] exposure"),
            Card::float("CCD-TEMP", -12.5, "[degC]"),
            Card::str("BAYERPAT", "RGGB", ""),
            Card::str("INSTRUME", "it's Aurion", "quote doubled"),
            Card::comment("free text"),
        ];
        let mut w = FitsU16Writer::new(Vec::new(), 3, 2, &cards).unwrap();
        w.write_row(&[0, 32768, 65535]).unwrap();
        w.write_row(&[1, 2, 3]).unwrap();
        let bytes = w.finish().unwrap();
        assert_eq!(bytes.len(), 2 * BLOCK, "one header block, one data block");
        let header = cards_of(&bytes[..BLOCK]);
        assert!(header.iter().all(|c| c.len() == 80 && c.is_ascii()));
        assert_eq!(header[0], format!("{:<80}", format!("SIMPLE  = {:>20} / conforms to FITS standard", "T")));
        assert!(header[1].starts_with("BITPIX  =                   16"));
        assert!(header[3].starts_with("NAXIS1  =                    3"));
        assert!(header[5].starts_with("BZERO   =                32768"));
        assert!(header.iter().any(|c| c.starts_with("EXPTIME =                  5.0 /")));
        assert!(header.iter().any(|c| c.starts_with("CCD-TEMP=                -12.5 /")));
        assert!(header.iter().any(|c| c.starts_with("BAYERPAT= 'RGGB    '")));
        assert!(header.iter().any(|c| c.starts_with("INSTRUME= 'it''s Aurion'")));
        assert!(header.iter().any(|c| c.starts_with("COMMENT free text")));
        assert_eq!(header.last().unwrap().trim_end(), "END");
        assert!(bytes[header.len() * 80..BLOCK].iter().all(|&b| b == b' '));
        // Data: big-endian, value − 32768
        let data = &bytes[BLOCK..];
        let read = |i: usize| i16::from_be_bytes([data[2 * i], data[2 * i + 1]]) as i32 + 32768;
        assert_eq!((0..6).map(read).collect::<Vec<_>>(), vec![0, 32768, 65535, 1, 2, 3]);
        assert!(data[12..].iter().all(|&b| b == 0));
    }

    #[test]
    fn exact_block_needs_no_padding() {
        // 1440 values = 2880 bytes
        let mut w = FitsU16Writer::new(Vec::new(), 1440, 1, &[]).unwrap();
        w.write_row(&[7; 1440]).unwrap();
        assert_eq!(w.finish().unwrap().len(), 2 * BLOCK);
    }

    #[test]
    fn refuses_invalid_input() {
        assert!(FitsU16Writer::new(Vec::new(), 0, 1, &[]).is_err());
        assert!(FitsU16Writer::new(Vec::new(), 1, 1, &[Card::int("toolong-key", 1, "")]).is_err());
        assert!(FitsU16Writer::new(Vec::new(), 1, 1, &[Card::int("bitpix", 1, "")]).is_err(), "lower case");
        assert!(FitsU16Writer::new(Vec::new(), 1, 1, &[Card::int("NAXIS", 3, "")]).is_err(), "reserved");
        assert!(FitsU16Writer::new(Vec::new(), 1, 1, &[Card::float("X", f64::NAN, "")]).is_err());
        let mut w = FitsU16Writer::new(Vec::new(), 2, 2, &[]).unwrap();
        assert!(w.write_row(&[1]).is_err());
        w.write_row(&[1, 2]).unwrap();
        assert!(matches!(w.finish(), Err(FitsError::MissingRows(1))));
    }

    #[test]
    fn float_and_date_formatting() {
        assert_eq!(format_float(8.0), "8.0");
        assert_eq!(format_float(0.0), "0.0");
        assert_eq!(format_float(1.25), "1.25");
        assert_eq!(format_float(1.5e-7), "1.500000000E-7");
        let t = crate::core::time::Timestamp::from_second(1_772_000_000).unwrap();
        assert_eq!(date_obs(t), "2026-02-25T06:13:20");
        let long = "x".repeat(100);
        let card = format_card(&Card { key: "COMMENT".into(), value: None, comment: long }).unwrap();
        assert_eq!(card.len(), 80);
    }
}
