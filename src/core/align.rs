//! Alignment of the frames of a stack on the stars (pure functions, no I/O).
//!
//! The sky turns around the celestial pole (15"/s at the celestial equator).
//! With the 2.7 mm lens a pixel covers about 118", so 8 frames of 30 s smear
//! the stars over up to some 30 pixels when they are averaged as they are.
//! Here every new frame of a stack is matched to the first one (the
//! reference) and resampled before it is added:
//!
//! 1. stars of both frames (`core::stars`, sky only);
//! 2. matching by triangles: every triangle of the [`TRIANGLE_STARS`]
//!    brightest stars is described by the ratios of its sides (shortest /
//!    longest, middle / longest) and its orientation, which a rotation and a
//!    translation do not change; triangles of the two frames with close
//!    ratios give candidate star pairs;
//! 3. robust estimation (RANSAC, deterministic): every candidate triangle
//!    pair, best ratios first, gives a rigid transform (rotation +
//!    translation, i.e. a rotation around one point: the celestial pole when
//!    it is in the field); the one that brings the most reference stars
//!    within [`INLIER_RADIUS`] pixels of a star of the frame wins, and it is
//!    refitted by least squares on those pairs;
//! 4. acceptance: at least [`MIN_MATCHES`] pairs and an RMS residual of at
//!    most [`MAX_RMS_PX`] pixels.
//!
//! **Rigid model and fisheye.** The motion of the stars in the image is a
//! rigid rotation only when the projection is symmetric around the pole. A
//! computation (outside the repository, numpy) on an equidistant fisheye
//! (`r = f·θ`, f = 2.7 mm / 1.55 µm ≈ 1742 px), 50 stars in the upper 65 % of
//! the frame, latitude 45°, gave these RMS residuals of the best rigid fit:
//! 0 px with the pole on the optical axis; 0.2 px over 60 s, 0.75 px over
//! 240 s and 1.5 px over 480 s with the pole 15° away from the axis. A
//! general affine model only lowered them by 5 to 10 %, so it is not used:
//! the error is local (it changes across the field), not a global shear.
//! The residual grows with the time between the reference and the frame,
//! which bounds the duration of a stack; with a rectilinear lens the same
//! computation gave about 4 times more. The projection of the real 2.7 mm
//! lens is not known: **residuals to be measured on the hardware** (the
//! session log gives them for every frame). Beyond that, the next step is a
//! model on the sphere (lens calibration), not a richer plane model.
//!
//! **Failures** (clouds, Moon, twilight, frame too different): the frame is
//! left out of the stack and the reason goes to `session.log` (see
//! [`AlignedStack`]). A stack whose *reference* has fewer than [`MIN_STARS`]
//! stars is built without alignment (there are no stars to smear anyway);
//! the next stack takes a new reference.
//!
//! Memory: the stack itself (`denoise::Stacker`, about 382 MB at 4056×3040)
//! plus the star detection of one frame at a time (about 15 MB, freed after
//! each frame) and the star lists (a few kB).

use crate::core::denoise::{Stacker, Transform};
use crate::core::stars::{detect_stars, sky_rows, Star, StarField};

/// Stars of each frame used to build triangles (C(20, 3) = 1140 triangles).
pub const TRIANGLE_STARS: usize = 20;
/// Stars needed in each frame to try an alignment.
pub const MIN_STARS: usize = 8;
/// Star pairs needed to accept an alignment.
pub const MIN_MATCHES: usize = 6;
/// Distance (full-resolution pixels) within which a transformed reference
/// star matches a star of the frame.
pub const INLIER_RADIUS: f64 = 4.0;
/// Largest RMS residual accepted (full-resolution pixels). A design choice:
/// about the radius of a small star; to be tuned with real nights.
pub const MAX_RMS_PX: f64 = 1.5;
/// Largest difference of side ratios between two matching triangles.
const TRIANGLE_TOLERANCE: f64 = 0.02;
/// Candidate triangle pairs tried at most (best ratios first).
const MAX_HYPOTHESES: usize = 400;
/// Shortest triangle side considered (full-resolution pixels): the ratios
/// of tiny triangles are dominated by the centroid errors.
const MIN_SIDE: f64 = 20.0;

/// Accepted alignment of a frame on the reference.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    /// Reference pixel → frame pixel.
    pub transform: Transform,
    /// Star pairs used by the final fit.
    pub matches: usize,
    /// RMS distance of the pairs after the fit (full-resolution pixels).
    pub rms_px: f64,
}

/// Why a frame could not be aligned.
#[derive(Debug, Clone, PartialEq)]
pub enum AlignError {
    TooFewStars { reference: usize, frame: usize },
    NoMatch { best: usize },
    ResidualsTooLarge { rms_px: f64, matches: usize },
}

impl std::fmt::Display for AlignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlignError::TooFewStars { reference, frame } => write!(
                f,
                "trop peu d'étoiles ({} sur l'image, {} sur la référence, {} minimum : nuages, Lune ou crépuscule ?)",
                frame, reference, MIN_STARS
            ),
            AlignError::NoMatch { best } => {
                write!(f, "étoiles non appariées ({} paires au mieux, {} minimum)", best, MIN_MATCHES)
            }
            AlignError::ResidualsTooLarge { rms_px, matches } => write!(
                f,
                "résidu trop grand ({:.2} px sur {} étoiles, {} px maximum : distorsion ou image trop éloignée de la référence)",
                rms_px, matches, MAX_RMS_PX
            ),
        }
    }
}

type P = (f64, f64);

/// Least-squares rigid transform `q ≈ R·p + t` of point pairs (2D
/// Procrustes without scale). None with fewer than 2 pairs.
pub fn fit_rigid(pairs: &[(P, P)]) -> Option<Transform> {
    if pairs.len() < 2 {
        return None;
    }
    let n = pairs.len() as f64;
    let (mut pcx, mut pcy, mut qcx, mut qcy) = (0.0, 0.0, 0.0, 0.0);
    for &((px, py), (qx, qy)) in pairs {
        pcx += px;
        pcy += py;
        qcx += qx;
        qcy += qy;
    }
    let (pcx, pcy, qcx, qcy) = (pcx / n, pcy / n, qcx / n, qcy / n);
    let (mut sdot, mut scross) = (0.0, 0.0);
    for &((px, py), (qx, qy)) in pairs {
        let (ax, ay, bx, by) = (px - pcx, py - pcy, qx - qcx, qy - qcy);
        sdot += ax * bx + ay * by;
        scross += ax * by - ay * bx;
    }
    let angle = scross.atan2(sdot);
    let (s, c) = angle.sin_cos();
    Some(Transform { a: c, b: -s, c: s, d: c, tx: qcx - (c * pcx - s * pcy), ty: qcy - (s * pcx + c * pcy) })
}

fn dist2(a: P, b: P) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// One-to-one pairs (reference star, frame star) within `INLIER_RADIUS`
/// once the reference is transformed, closest pairs first.
fn inliers(t: &Transform, reference: &[P], frame: &[P]) -> Vec<(P, P)> {
    let r2 = INLIER_RADIUS * INLIER_RADIUS;
    let mut cands: Vec<(f64, usize, usize)> = Vec::new();
    for (i, &p) in reference.iter().enumerate() {
        let tp = t.apply(p.0, p.1);
        for (j, &q) in frame.iter().enumerate() {
            let d = dist2(tp, q);
            if d <= r2 {
                cands.push((d, i, j));
            }
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (mut used_i, mut used_j) = (vec![false; reference.len()], vec![false; frame.len()]);
    let mut out = Vec::new();
    for (_, i, j) in cands {
        if !used_i[i] && !used_j[j] {
            used_i[i] = true;
            used_j[j] = true;
            out.push((reference[i], frame[j]));
        }
    }
    out
}

fn rms(t: &Transform, pairs: &[(P, P)]) -> f64 {
    if pairs.is_empty() {
        return f64::INFINITY;
    }
    (pairs.iter().map(|&(p, q)| dist2(t.apply(p.0, p.1), q)).sum::<f64>() / pairs.len() as f64).sqrt()
}

/// Triangle of three star indices, vertices ordered by the length of the
/// opposite side (shortest first), with its two side ratios.
struct Triangle {
    v: [usize; 3],
    ratios: (f64, f64),
    clockwise: bool,
}

fn triangles(stars: &[P]) -> Vec<Triangle> {
    let n = stars.len().min(TRIANGLE_STARS);
    let mut out = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let idx = [i, j, k];
                // Side opposite each vertex
                let side = |m: usize| dist2(stars[idx[(m + 1) % 3]], stars[idx[(m + 2) % 3]]).sqrt();
                let mut order = [0usize, 1, 2];
                order.sort_by(|&a, &b| side(a).total_cmp(&side(b)));
                let (a, b, c) = (side(order[0]), side(order[1]), side(order[2]));
                if a < MIN_SIDE {
                    continue;
                }
                let v = [idx[order[0]], idx[order[1]], idx[order[2]]];
                let (p0, p1, p2) = (stars[v[0]], stars[v[1]], stars[v[2]]);
                let cross = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
                out.push(Triangle { v, ratios: (a / c, b / c), clockwise: cross > 0.0 });
            }
        }
    }
    out
}

/// Align `frame` on `reference` (stars brightest first, as given by
/// `core::stars`). The transform maps reference pixels to frame pixels.
pub fn align(reference: &[Star], frame: &[Star]) -> Result<Alignment, AlignError> {
    if reference.len() < MIN_STARS || frame.len() < MIN_STARS {
        return Err(AlignError::TooFewStars { reference: reference.len(), frame: frame.len() });
    }
    let rp: Vec<P> = reference.iter().map(|s| (s.x, s.y)).collect();
    let fp: Vec<P> = frame.iter().map(|s| (s.x, s.y)).collect();
    let rt = triangles(&rp);
    let mut ft = triangles(&fp);
    ft.sort_by(|a, b| a.ratios.0.total_cmp(&b.ratios.0));
    // Candidate triangle pairs, closest ratios first
    let mut cands: Vec<(f64, usize, usize)> = Vec::new();
    for (i, t) in rt.iter().enumerate() {
        let lo = ft.partition_point(|f| f.ratios.0 < t.ratios.0 - TRIANGLE_TOLERANCE);
        for (j, f) in ft.iter().enumerate().skip(lo) {
            if f.ratios.0 > t.ratios.0 + TRIANGLE_TOLERANCE {
                break;
            }
            let d = (f.ratios.0 - t.ratios.0).abs().max((f.ratios.1 - t.ratios.1).abs());
            if d <= TRIANGLE_TOLERANCE && f.clockwise == t.clockwise {
                cands.push((d, i, j));
            }
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    cands.truncate(MAX_HYPOTHESES);

    let mut best: Option<(Vec<(P, P)>, f64)> = None;
    for &(_, i, j) in &cands {
        let pairs: Vec<(P, P)> = (0..3).map(|m| (rp[rt[i].v[m]], fp[ft[j].v[m]])).collect();
        let Some(t) = fit_rigid(&pairs) else { continue };
        if rms(&t, &pairs) > INLIER_RADIUS {
            continue;
        }
        let found = inliers(&t, &rp, &fp);
        let r = rms(&t, &found);
        let better = match &best {
            None => true,
            Some((b, br)) => found.len() > b.len() || (found.len() == b.len() && r < *br),
        };
        if better {
            best = Some((found, r));
        }
    }
    let Some((mut pairs, _)) = best else {
        return Err(AlignError::NoMatch { best: 0 });
    };
    if pairs.len() < MIN_MATCHES {
        return Err(AlignError::NoMatch { best: pairs.len() });
    }
    // Least-squares refinement on all the pairs, twice
    let mut t = fit_rigid(&pairs).expect("at least MIN_MATCHES pairs");
    for _ in 0..2 {
        let again = inliers(&t, &rp, &fp);
        if again.len() < MIN_MATCHES {
            break;
        }
        pairs = again;
        t = fit_rigid(&pairs).expect("at least MIN_MATCHES pairs");
    }
    let rms_px = rms(&t, &pairs);
    if rms_px > MAX_RMS_PX {
        return Err(AlignError::ResidualsTooLarge { rms_px, matches: pairs.len() });
    }
    Ok(Alignment { transform: t, matches: pairs.len(), rms_px })
}

/// What [`AlignedStack::add_frame`] did with a frame.
#[derive(Debug, Clone, PartialEq)]
pub enum FrameOutcome {
    /// First frame of the stack, with enough stars: the reference.
    Reference { stars: usize },
    /// First frame without enough stars: this stack is not aligned.
    ReferenceWithoutStars { stars: usize },
    /// Added without alignment (stack without reference stars).
    Unaligned,
    /// Resampled on the reference and added.
    Aligned { alignment: Alignment, stars: usize, left_out_px: usize },
    /// Left out of the stack.
    Skipped(AlignError),
    /// Not the size of the stack (or stack full): ignored.
    Refused,
}

impl FrameOutcome {
    /// Line for `session.log` (`frame` = frame number of the night).
    pub fn describe(&self, frame: u64) -> String {
        match self {
            FrameOutcome::Reference { stars } => format!("EMPILEMENT image {:05} : référence, {} étoiles", frame, stars),
            FrameOutcome::ReferenceWithoutStars { stars } => format!(
                "EMPILEMENT image {:05} : référence avec {} étoiles ({} minimum), pile empilée sans recalage",
                frame, stars, MIN_STARS
            ),
            FrameOutcome::Unaligned => format!("EMPILEMENT image {:05} : ajoutée sans recalage", frame),
            FrameOutcome::Aligned { alignment: a, stars, .. } => {
                let centre = a
                    .transform
                    .fixed_point()
                    .map(|(x, y)| format!(", centre ({:.0}, {:.0})", x, y))
                    .unwrap_or_default();
                format!(
                    "EMPILEMENT image {:05} : recalée, rotation {:.3}°{}, {} étoiles appariées sur {}, résidu {:.2} px",
                    frame,
                    a.transform.angle().to_degrees(),
                    centre,
                    a.matches,
                    stars,
                    a.rms_px
                )
            }
            FrameOutcome::Skipped(e) => format!("EMPILEMENT image {:05} ignorée : {}", frame, e),
            FrameOutcome::Refused => format!("EMPILEMENT image {:05} ignorée : taille différente de la pile", frame),
        }
    }
}

/// A stack whose frames are aligned on the stars of its first frame.
pub struct AlignedStack {
    stacker: Stacker,
    roi_top_percent: u32,
    /// Stars of the reference; None before the first frame or when the
    /// reference had too few stars (stack without alignment).
    reference: Option<Vec<Star>>,
    /// Frames offered since the start of the stack (added or skipped).
    seen: u32,
}

impl AlignedStack {
    pub fn new(width: u32, height: u32, roi_top_percent: u32) -> Self {
        Self { stacker: Stacker::new(width, height), roi_top_percent, reference: None, seen: 0 }
    }

    pub fn dimensions(&self) -> (u32, u32) {
        self.stacker.dimensions()
    }

    pub fn roi_top_percent(&self) -> u32 {
        self.roi_top_percent
    }

    /// Frames added to the stack.
    pub fn count(&self) -> u32 {
        self.stacker.count()
    }

    /// Frames offered since the start of the stack, added or skipped.
    pub fn seen(&self) -> u32 {
        self.seen
    }

    fn stars(&self, rgb: &[u8]) -> StarField {
        let (w, h) = self.dimensions();
        detect_stars(rgb, w, h, self.roi_top_percent)
    }

    /// Offer one full-resolution RGB frame to the stack.
    pub fn add_frame(&mut self, rgb: &[u8]) -> FrameOutcome {
        let (w, h) = self.dimensions();
        if rgb.len() != (w * h * 3) as usize {
            return FrameOutcome::Refused;
        }
        self.seen += 1;
        if self.stacker.count() == 0 {
            let field = self.stars(rgb);
            if !self.stacker.add(rgb) {
                return FrameOutcome::Refused;
            }
            let stars = field.stars.len();
            if stars >= MIN_STARS {
                self.reference = Some(field.stars);
                return FrameOutcome::Reference { stars };
            }
            self.reference = None;
            return FrameOutcome::ReferenceWithoutStars { stars };
        }
        let Some(reference) = &self.reference else {
            return if self.stacker.add(rgb) { FrameOutcome::Unaligned } else { FrameOutcome::Refused };
        };
        let field = self.stars(rgb);
        match align(reference, &field.stars) {
            Ok(alignment) => match self.stacker.add_warped(rgb, &alignment.transform, sky_rows(h, self.roi_top_percent)) {
                Some(left_out_px) => FrameOutcome::Aligned { alignment, stars: field.stars.len(), left_out_px },
                None => FrameOutcome::Refused,
            },
            Err(e) => FrameOutcome::Skipped(e),
        }
    }

    pub fn result(&self) -> Option<Vec<u8>> {
        self.stacker.result()
    }

    /// Start a new stack (new reference at the next frame).
    pub fn reset(&mut self) {
        self.stacker.reset();
        self.reference = None;
        self.seen = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::denoise::to_linear_lut;
    use crate::core::stars::{random_stars, render_stars};

    const ROI: u32 = 65;

    fn frame(w: u32, h: u32, stars: &[(f64, f64, f64)], t: &Transform, seed: u64) -> Vec<u8> {
        let moved: Vec<_> = stars
            .iter()
            .map(|&(x, y, p)| {
                let (x, y) = t.apply(x, y);
                (x, y, p)
            })
            .collect();
        render_stars(w, h, &moved, 3.0, 1.5, sky_rows(h, ROI), seed)
    }

    /// FWHM (pixels) of the spot around (x, y), from its second moments in
    /// linear light above the local background.
    fn fwhm(rgb: &[u8], w: u32, x: f64, y: f64) -> f64 {
        let lut = to_linear_lut();
        let at = |xx: i64, yy: i64| lut[rgb[(yy as usize * w as usize + xx as usize) * 3 + 1] as usize] as f64;
        let (cx, cy) = (x.round() as i64, y.round() as i64);
        let mut ring = Vec::new();
        for d in -8i64..=8 {
            ring.extend([at(cx + d, cy - 8), at(cx + d, cy + 8), at(cx - 8, cy + d), at(cx + 8, cy + d)]);
        }
        ring.sort_by(f64::total_cmp);
        let bg = ring[ring.len() / 2];
        let (mut s, mut sx, mut sy, mut sxx, mut syy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for yy in cy - 5..=cy + 5 {
            for xx in cx - 5..=cx + 5 {
                let v = (at(xx, yy) - bg).max(0.0);
                s += v;
                sx += v * xx as f64;
                sy += v * yy as f64;
                sxx += v * (xx * xx) as f64;
                syy += v * (yy * yy) as f64;
            }
        }
        let (mx, my) = (sx / s, sy / s);
        let var = ((sxx / s - mx * mx) + (syy / s - my * my)) / 2.0;
        2.3548 * var.max(0.0).sqrt()
    }

    #[test]
    fn rigid_fit_recovers_a_rotation_around_a_point() {
        let t = Transform::rotation_about(0.02, 1500.0, -300.0);
        let pairs: Vec<(P, P)> = [(10.0, 20.0), (800.0, 100.0), (400.0, 600.0), (1200.0, 900.0)]
            .iter()
            .map(|&p| (p, t.apply(p.0, p.1)))
            .collect();
        let f = fit_rigid(&pairs).unwrap();
        assert!((f.angle() - 0.02).abs() < 1e-12);
        let (cx, cy) = f.fixed_point().unwrap();
        assert!((cx - 1500.0).abs() < 1e-6 && (cy + 300.0).abs() < 1e-6, "{} {}", cx, cy);
        assert!(fit_rigid(&pairs[..1]).is_none());
        assert!(Transform::IDENTITY.fixed_point().is_none());
    }

    #[test]
    fn recovers_a_known_rotation_and_stacks_sharp_stars() {
        let (w, h) = (1000, 760);
        let sky = sky_rows(h, ROI);
        let truth = random_stars(w, sky, 60, 21);
        // Rotation of 0.35° per frame around a point above the image (the
        // pole out of the field), like 8 frames of 30 s seen through a lens
        // about 4 times longer than the 2.7 mm (to keep the test small)
        let (cx, cy) = (430.0, -900.0);
        let step = 0.35f64.to_radians();
        let mut stack = AlignedStack::new(w, h, ROI);
        let reference = frame(w, h, &truth, &Transform::IDENTITY, 100);
        assert!(matches!(stack.add_frame(&reference), FrameOutcome::Reference { stars } if stars >= MIN_STARS));
        let mut plain = Stacker::new(w, h);
        plain.add(&reference);
        for k in 1..8 {
            let t = Transform::rotation_about(step * k as f64, cx, cy);
            let rgb = frame(w, h, &truth, &t, 100 + k);
            plain.add(&rgb);
            match stack.add_frame(&rgb) {
                FrameOutcome::Aligned { alignment: a, .. } => {
                    assert!((a.transform.angle() - step * k as f64).abs() < 2e-4, "angle {} vs {}", a.transform.angle(), step * k as f64);
                    // Predicted positions over the whole sky within 0.3 px
                    for (x, y) in [(0.0, 0.0), (w as f64, 0.0), (0.0, sky as f64), (w as f64, sky as f64), (500.0, 250.0)] {
                        let (p, q) = (a.transform.apply(x, y), t.apply(x, y));
                        assert!(dist2(p, q).sqrt() < 0.3, "frame {} at ({}, {}): {:?} vs {:?}", k, x, y, p, q);
                    }
                    // The centre is far (900 px) from the stars: its error is
                    // the angle error times that distance, a few % of it
                    let (fx, fy) = a.transform.fixed_point().unwrap();
                    assert!((fx - cx).hypot(fy - cy) < 45.0, "centre ({:.1}, {:.1})", fx, fy);
                    assert!(a.rms_px < 0.5, "rms {}", a.rms_px);
                }
                other => panic!("frame {} not aligned: {:?}", k, other),
            }
        }
        assert_eq!((stack.count(), stack.seen()), (8, 8));
        let aligned = stack.result().unwrap();
        let unaligned = plain.result().unwrap();
        // Isolated, well exposed stars of the reference, away from the edges
        let probe: Vec<_> = truth
            .iter()
            .filter(|s| s.2 > 80.0 && s.0 > 40.0 && s.0 < w as f64 - 40.0 && s.1 > 40.0 && s.1 < sky as f64 - 40.0)
            .filter(|s| truth.iter().all(|o| o == *s || (o.0 - s.0).hypot(o.1 - s.1) > 20.0))
            .collect();
        assert!(probe.len() >= 3, "{} probe stars", probe.len());
        for s in probe {
            let single = fwhm(&reference, w, s.0, s.1);
            let a = fwhm(&aligned, w, s.0, s.1);
            let u = fwhm(&unaligned, w, s.0, s.1);
            assert!(a < single * 1.3, "aligned FWHM {:.2} vs single {:.2}", a, single);
            assert!(u > single * 1.6, "unaligned stack should be smeared: {:.2} vs {:.2}", u, single);
        }
        // The ground (below the sky rows) is never moved: the stack of the
        // ground is the plain stack of the ground
        let g = (sky as usize + 10) * w as usize * 3;
        assert_eq!(&aligned[g..g + 300], &unaligned[g..g + 300]);
    }

    #[test]
    fn hot_pixels_do_not_pin_the_alignment() {
        // Fixed hot pixels (the same on every frame) and a rotated sky: the
        // transform found is the sky's, not the identity
        let (w, h) = (900, 700);
        let sky = sky_rows(h, ROI);
        let truth = random_stars(w, sky, 40, 33);
        let t = Transform::rotation_about(0.6f64.to_radians(), 450.0, -600.0);
        let mut a = frame(w, h, &truth, &Transform::IDENTITY, 1);
        let mut b = frame(w, h, &truth, &t, 2);
        let mut rng = crate::core::denoise::XorShift::new(77);
        for _ in 0..60 {
            let (x, y) = (5 + rng.next_u32() as usize % (w as usize - 10), 5 + rng.next_u32() as usize % (sky as usize - 10));
            for img in [&mut a, &mut b] {
                let i = (y * w as usize + x) * 3;
                img[i..i + 3].copy_from_slice(&[220, 220, 220]);
            }
        }
        let ra = detect_stars(&a, w, h, ROI);
        let rb = detect_stars(&b, w, h, ROI);
        let al = align(&ra.stars, &rb.stars).expect("aligned");
        assert!((al.transform.angle() - 0.6f64.to_radians()).abs() < 3e-4, "angle {}", al.transform.angle().to_degrees());
    }

    #[test]
    fn starless_sky_fails_cleanly() {
        let (w, h) = (600, 450);
        let sky = sky_rows(h, ROI);
        let cloudy = render_stars(w, h, &[], 3.0, 2.0, sky, 5);
        let mut stack = AlignedStack::new(w, h, ROI);
        assert_eq!(stack.add_frame(&cloudy), FrameOutcome::ReferenceWithoutStars { stars: 0 });
        assert_eq!(stack.add_frame(&cloudy), FrameOutcome::Unaligned);
        assert_eq!(stack.count(), 2);
        assert!(stack.result().is_some());
        // A reference with stars, then clouds: the cloudy frame is left out
        stack.reset();
        let clear = frame(w, h, &random_stars(w, sky, 40, 8), &Transform::IDENTITY, 6);
        assert!(matches!(stack.add_frame(&clear), FrameOutcome::Reference { .. }));
        let out = stack.add_frame(&cloudy);
        assert!(matches!(out, FrameOutcome::Skipped(AlignError::TooFewStars { frame: 0, .. })), "{:?}", out);
        assert!(out.describe(12).contains("ignorée : trop peu d'étoiles"), "{}", out.describe(12));
        assert_eq!((stack.count(), stack.seen()), (1, 2));
        assert_eq!(stack.add_frame(&[0u8; 10]), FrameOutcome::Refused);
    }

    #[test]
    fn unrelated_star_fields_do_not_match() {
        let (w, h) = (900, 700);
        let sky = sky_rows(h, ROI);
        let a = detect_stars(&frame(w, h, &random_stars(w, sky, 40, 1), &Transform::IDENTITY, 1), w, h, ROI);
        let b = detect_stars(&frame(w, h, &random_stars(w, sky, 40, 2), &Transform::IDENTITY, 2), w, h, ROI);
        let r = align(&a.stars, &b.stars);
        assert!(matches!(r, Err(AlignError::NoMatch { .. })), "{:?}", r);
    }

    #[test]
    fn describes_every_outcome_in_french() {
        let a = Alignment { transform: Transform::rotation_about(0.01, 100.0, 200.0), matches: 12, rms_px: 0.4 };
        let line = FrameOutcome::Aligned { alignment: a, stars: 30, left_out_px: 0 }.describe(7);
        assert!(line.contains("recalée, rotation 0.573°, centre (100, 200), 12 étoiles appariées sur 30, résidu 0.40 px"), "{}", line);
        let e = AlignError::ResidualsTooLarge { rms_px: 2.5, matches: 9 };
        assert!(FrameOutcome::Skipped(e).describe(3).contains("résidu trop grand (2.50 px"));
    }
}
