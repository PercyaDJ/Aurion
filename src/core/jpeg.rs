//! Minimal JPEG / EXIF helpers (no external dependency).
//!
//! - [`exif_thumbnail`] returns the small JPEG that `rpicam-still` embeds in
//!   the EXIF block of every capture (320×240 by default). Analysing this
//!   thumbnail instead of decoding the 12 MP image divides the CPU work (and
//!   the energy drawn from the battery) of each frame by about 150.
//! - [`transplant_exif`] copies the EXIF block of the original capture into
//!   a re-encoded JPEG (after noise reduction), so exposure metadata stay in
//!   the photo.

/// Iterate over the marker segments before the image data:
/// yields `(marker, segment_start, segment_end)` where the range covers the
/// whole segment including the `FF xx` marker and length bytes.
fn segments(jpeg: &[u8]) -> impl Iterator<Item = (u8, usize, usize)> + '_ {
    let mut pos = if jpeg.len() >= 2 && jpeg[0] == 0xFF && jpeg[1] == 0xD8 { 2 } else { jpeg.len() };
    std::iter::from_fn(move || {
        // Skip fill bytes
        while pos + 1 < jpeg.len() && jpeg[pos] == 0xFF && jpeg[pos + 1] == 0xFF {
            pos += 1;
        }
        if pos + 4 > jpeg.len() || jpeg[pos] != 0xFF {
            return None;
        }
        let marker = jpeg[pos + 1];
        // Start of scan: compressed data follows, no more headers.
        if marker == 0xDA || marker == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            return None;
        }
        let start = pos;
        pos += 2 + len;
        Some((marker, start, pos))
    })
}

/// The TIFF block of the EXIF APP1 segment, if any.
fn exif_tiff(jpeg: &[u8]) -> Option<&[u8]> {
    segments(jpeg).find_map(|(marker, start, end)| {
        let payload = &jpeg[start + 4..end];
        (marker == 0xE1 && payload.starts_with(b"Exif\0\0")).then(|| &payload[6..])
    })
}

struct Tiff<'a> {
    data: &'a [u8],
    little: bool,
}

impl<'a> Tiff<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        let little = match data.get(..4)? {
            [b'I', b'I', 42, 0] => true,
            [b'M', b'M', 0, 42] => false,
            _ => return None,
        };
        Some(Self { data, little })
    }

    fn u16_at(&self, off: usize) -> Option<u16> {
        let b = self.data.get(off..off + 2)?;
        Some(if self.little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) })
    }

    fn u32_at(&self, off: usize) -> Option<u32> {
        let b = self.data.get(off..off + 4)?;
        let a = [b[0], b[1], b[2], b[3]];
        Some(if self.little { u32::from_le_bytes(a) } else { u32::from_be_bytes(a) })
    }

    /// `(entries_offset, count, next_ifd_offset)` of the IFD at `off`.
    fn ifd(&self, off: usize) -> Option<(usize, usize, u32)> {
        let count = self.u16_at(off)? as usize;
        let entries = off + 2;
        let next = self.u32_at(entries + count * 12)?;
        Some((entries, count, next))
    }

    /// Value of a LONG or SHORT tag with count 1 in an IFD.
    fn tag_value(&self, entries: usize, count: usize, tag: u16) -> Option<u32> {
        (0..count).find_map(|i| {
            let e = entries + i * 12;
            if self.u16_at(e)? != tag {
                return None;
            }
            match self.u16_at(e + 2)? {
                3 => self.u16_at(e + 8).map(u32::from), // SHORT
                4 => self.u32_at(e + 8),                // LONG
                _ => None,
            }
        })
    }
}

/// Embedded EXIF thumbnail (a complete JPEG), if the capture has one.
pub fn exif_thumbnail(jpeg: &[u8]) -> Option<&[u8]> {
    let tiff = Tiff::new(exif_tiff(jpeg)?)?;
    let (_, count0, ifd1) = tiff.ifd(tiff.u32_at(4)? as usize)?;
    let _ = count0;
    if ifd1 == 0 {
        return None;
    }
    let (entries, count, _) = tiff.ifd(ifd1 as usize)?;
    let offset = tiff.tag_value(entries, count, 0x0201)? as usize;
    let length = tiff.tag_value(entries, count, 0x0202)? as usize;
    let thumb = tiff.data.get(offset..offset.checked_add(length)?)?;
    (thumb.len() > 4 && thumb[0] == 0xFF && thumb[1] == 0xD8).then_some(thumb)
}

/// Copy the EXIF segment(s) of `original` into `encoded` (right after SOI).
/// Returns `encoded` unchanged if either image is not a JPEG or `original`
/// has no EXIF block.
pub fn transplant_exif(original: &[u8], encoded: Vec<u8>) -> Vec<u8> {
    if encoded.len() < 2 || encoded[0] != 0xFF || encoded[1] != 0xD8 {
        return encoded;
    }
    let exif: Vec<&[u8]> = segments(original)
        .filter(|(m, s, e)| *m == 0xE1 && original[s + 4..*e].starts_with(b"Exif\0\0"))
        .map(|(_, s, e)| &original[s..e])
        .collect();
    if exif.is_empty() {
        return encoded;
    }
    let mut out = Vec::with_capacity(encoded.len() + exif.iter().map(|s| s.len()).sum::<usize>());
    out.extend_from_slice(&encoded[..2]);
    for seg in exif {
        out.extend_from_slice(seg);
    }
    out.extend_from_slice(&encoded[2..]);
    out
}

/// Build an APP1 EXIF segment holding only a thumbnail (tests and mocks).
pub fn build_exif_with_thumbnail(thumb: &[u8], little_endian: bool) -> Vec<u8> {
    let w16 = |v: u16| if little_endian { v.to_le_bytes() } else { v.to_be_bytes() };
    let w32 = |v: u32| if little_endian { v.to_le_bytes() } else { v.to_be_bytes() };
    let mut tiff = Vec::new();
    tiff.extend_from_slice(if little_endian { b"II" } else { b"MM" });
    tiff.extend_from_slice(&w16(42));
    tiff.extend_from_slice(&w32(8)); // IFD0 at 8
    // IFD0: no entry, next = IFD1 at 14
    tiff.extend_from_slice(&w16(0));
    tiff.extend_from_slice(&w32(14));
    // IFD1 at 14: 2 entries (2 + 24 + 4 bytes) → thumbnail at 44
    let thumb_offset = 14 + 2 + 2 * 12 + 4;
    tiff.extend_from_slice(&w16(2));
    for (tag, value) in [(0x0201u16, thumb_offset as u32), (0x0202u16, thumb.len() as u32)] {
        tiff.extend_from_slice(&w16(tag));
        tiff.extend_from_slice(&w16(4)); // LONG
        tiff.extend_from_slice(&w32(1));
        tiff.extend_from_slice(&w32(value));
    }
    tiff.extend_from_slice(&w32(0));
    tiff.extend_from_slice(thumb);

    let mut seg = vec![0xFF, 0xE1];
    let len = (2 + 6 + tiff.len()) as u16;
    seg.extend_from_slice(&len.to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(&tiff);
    seg
}

/// Insert an APP1 segment right after the SOI of `jpeg`.
pub fn insert_segment(jpeg: &[u8], segment: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(jpeg.len() + segment.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(segment);
    out.extend_from_slice(&jpeg[2..]);
    out
}

/// Decode the pixels used for analysis (exposure metering, detection,
/// gallery thumbnail): the EXIF thumbnail when present, otherwise the full
/// image reduced to at most `max_width` pixels wide.
/// Returns `(rgb, width, height, used_embedded_thumbnail)`.
pub fn decode_for_analysis(jpeg: &[u8], max_width: u32) -> Option<(Vec<u8>, u32, u32, bool)> {
    if let Some(thumb) = exif_thumbnail(jpeg) {
        if let Ok(img) = image::load_from_memory_with_format(thumb, image::ImageFormat::Jpeg) {
            let rgb = img.to_rgb8();
            let (w, h) = rgb.dimensions();
            return Some((rgb.into_raw(), w, h, true));
        }
    }
    let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg).ok()?;
    let img = if img.width() > max_width { img.thumbnail(max_width, u32::MAX) } else { img };
    let rgb = img.to_rgb8();
    let (w, h) = rgb.dimensions();
    Some((rgb.into_raw(), w, h, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb(color));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        buf.into_inner()
    }

    #[test]
    fn extracts_thumbnail_both_endians() {
        let thumb = jpeg(32, 24, [10, 200, 30]);
        for little in [true, false] {
            let full = insert_segment(&jpeg(400, 300, [0, 0, 0]), &build_exif_with_thumbnail(&thumb, little));
            assert_eq!(exif_thumbnail(&full), Some(thumb.as_slice()), "little_endian={}", little);
            // The full image is still a valid JPEG
            assert_eq!(image::load_from_memory(&full).unwrap().width(), 400);
        }
    }

    #[test]
    fn no_thumbnail_or_garbage() {
        assert!(exif_thumbnail(&jpeg(10, 10, [1, 2, 3])).is_none());
        assert!(exif_thumbnail(b"").is_none());
        assert!(exif_thumbnail(b"\xFF\xD8\xFF\xE1\xFF\xFF").is_none());
        assert!(exif_thumbnail(&[0xFF; 100]).is_none());
        // Truncated EXIF with a lying length must not panic
        let mut bad = build_exif_with_thumbnail(&jpeg(8, 8, [0, 0, 0]), true);
        bad.truncate(30);
        let mut img = vec![0xFF, 0xD8];
        img.extend_from_slice(&bad);
        assert!(exif_thumbnail(&img).is_none());
    }

    #[test]
    fn decode_prefers_thumbnail() {
        let thumb = jpeg(32, 24, [10, 200, 30]);
        let full = insert_segment(&jpeg(1000, 750, [0, 0, 0]), &build_exif_with_thumbnail(&thumb, true));
        let (rgb, w, h, used) = decode_for_analysis(&full, 640).unwrap();
        assert!(used);
        assert_eq!((w, h), (32, 24));
        assert!(rgb[1] > 150, "green thumbnail pixels");

        let (_, w, h, used) = decode_for_analysis(&jpeg(1000, 750, [0, 0, 0]), 640).unwrap();
        assert!(!used);
        assert_eq!(w, 640);
        assert_eq!(h, 480);
    }

    #[test]
    fn exif_is_transplanted() {
        let seg = build_exif_with_thumbnail(&jpeg(8, 8, [1, 1, 1]), true);
        let original = insert_segment(&jpeg(50, 50, [5, 5, 5]), &seg);
        let reencoded = jpeg(50, 50, [6, 6, 6]);
        let out = transplant_exif(&original, reencoded.clone());
        assert_eq!(out.len(), reencoded.len() + seg.len());
        assert!(exif_thumbnail(&out).is_some());
        assert!(image::load_from_memory(&out).is_ok());
        // Nothing to transplant
        assert_eq!(transplant_exif(&reencoded, reencoded.clone()), reencoded);
    }
}
