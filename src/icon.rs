//! Level icons and the app icon, drawn procedurally and cached on disk.
//!
//! A toast needs an image *file* -- it cannot reference an icon inside a DLL --
//! so rather than ship binary assets the four level icons and the app icon are
//! drawn on first use and cached under %LOCALAPPDATA%\reminder\icons.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::level::{Glyph, Level};
use crate::png;

pub const ICON_SIZE: u32 = 96;

/// A line segment to stroke, in the 96px design space: x0, y0, x1, y1, and
/// half the stroke width.
type Stroke = (f32, f32, f32, f32, f32);
/// A filled dot: centre x, centre y, radius.
type Dot = (f32, f32, f32);

/// Per-user directory for generated icons.
fn data_dir() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("reminder")
}

fn icon_cache_dir() -> PathBuf {
    data_dir().join("icons")
}

/// Anti-aliased inside-ness of a shape edge, smoothed over roughly one pixel.
fn coverage(distance: f32, radius: f32) -> f32 {
    (radius - distance + 0.5).clamp(0.0, 1.0)
}

/// Shortest distance from a point to a line segment.
fn segment_distance(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq == 0.0 {
        0.0
    } else {
        (((px - x0) * dx + (py - y0) * dy) / length_sq).clamp(0.0, 1.0)
    };
    let (nx, ny) = (x0 + t * dx, y0 + t * dy);
    ((px - nx).powi(2) + (py - ny).powi(2)).sqrt()
}

/// The strokes and dots making up a glyph, in the 96px design space.
///
/// `solid` is an optional rounded triangle (apex, base-left, base-right,
/// corner radius) that the strokes and dots are cut *out of*; without one
/// the strokes and dots simply union as usual.
fn glyph_shapes(
    glyph: Glyph,
) -> (
    Option<([f32; 2], [f32; 2], [f32; 2], f32)>,
    Vec<Stroke>,
    Vec<Dot>,
) {
    match glyph {
        Glyph::Cross => (
            None,
            vec![(33.0, 33.0, 63.0, 63.0, 4.5), (63.0, 33.0, 33.0, 63.0, 4.5)],
            vec![],
        ),
        Glyph::Check => (
            None,
            vec![(29.0, 49.0, 42.0, 63.0, 4.5), (42.0, 63.0, 68.0, 33.0, 4.5)],
            vec![],
        ),
        Glyph::Triangle => (
            // A solid warning triangle shaped like the classic important
            // sign (e.g. the notify-send "important" icon): taller than wide,
            // corners rounded, "!" cut out. The unrounded corners sit inside
            // the disc (furthest point 43px from centre against a 46px
            // disc radius); the rounded shape never reaches further.
            Some(([48.0, 5.0], [13.0, 71.0], [83.0, 71.0], 5.0)),
            vec![(48.0, 24.8, 48.0, 45.9, 4.0)],
            vec![(48.0, 57.8, 4.0)],
        ),
        Glyph::Info => (
            None,
            vec![(48.0, 44.0, 48.0, 71.0, 4.5)],
            vec![(48.0, 28.0, 5.5)],
        ),
    }
}

/// Shortest signed distance to triangle `abc`: negative inside.
fn sd_triangle(px: f32, py: f32, a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    let (e0x, e0y) = (b[0] - a[0], b[1] - a[1]);
    let (e1x, e1y) = (c[0] - b[0], c[1] - b[1]);
    let (e2x, e2y) = (a[0] - c[0], a[1] - c[1]);
    let (v0x, v0y) = (px - a[0], py - a[1]);
    let (v1x, v1y) = (px - b[0], py - b[1]);
    let (v2x, v2y) = (px - c[0], py - c[1]);

    let proj = |vx: f32, vy: f32, ex: f32, ey: f32| -> (f32, f32) {
        let t = ((vx * ex + vy * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
        (vx - t * ex, vy - t * ey)
    };
    let (p0x, p0y) = proj(v0x, v0y, e0x, e0y);
    let (p1x, p1y) = proj(v1x, v1y, e1x, e1y);
    let (p2x, p2y) = proj(v2x, v2y, e2x, e2y);

    let s = (e0x * e2y - e0y * e2x).signum();
    let d0 = (p0x * p0x + p0y * p0y, s * (v0x * e0y - v0y * e0x));
    let d1 = (p1x * p1x + p1y * p1y, s * (v1x * e1y - v1y * e1x));
    let d2 = (p2x * p2x + p2y * p2y, s * (v2x * e2y - v2y * e2x));
    // Component-wise min, as in the original vec2 min: distance from the
    // closest edge, sign from the most-negative cross term. (A point is
    // inside the triangle iff it is inside all three edge half-planes, so
    // the sign stays correct even where two edges are equidistant at a
    // corner -- lexicographic tuple comparison flips the sign there.)
    let dx = d0.0.min(d1.0).min(d2.0);
    let dy = d0.1.min(d1.1).min(d2.1);
    -dx.sqrt() * dy.signum()
}

/// Signed distance to a rounded triangle: each vertex is pulled in along its
/// angle bisector so every edge moves inward by `round`, then the result is
/// expanded by `round`, so the corners become arcs of radius `round` while
/// the straight edges land back on the original ones. Negative inside.
fn sd_rounded_triangle(px: f32, py: f32, a: [f32; 2], b: [f32; 2], c: [f32; 2], round: f32) -> f32 {
    let shrink = |v: [f32; 2], w: [f32; 2], u: [f32; 2]| -> [f32; 2] {
        let (e1x, e1y) = (w[0] - v[0], w[1] - v[1]);
        let (e2x, e2y) = (u[0] - v[0], u[1] - v[1]);
        let l1 = (e1x * e1x + e1y * e1y).sqrt();
        let l2 = (e2x * e2x + e2y * e2y).sqrt();
        let (u1x, u1y) = (e1x / l1, e1y / l1);
        let (u2x, u2y) = (e2x / l2, e2y / l2);
        let cos_th = (u1x * u2x + u1y * u2y).clamp(-1.0, 1.0);
        let sin_half = ((1.0 - cos_th) / 2.0).sqrt();
        let (bx, by) = (u1x + u2x, u1y + u2y);
        let bl = (bx * bx + by * by).sqrt();
        let d = round / sin_half;
        [v[0] + (bx / bl) * d, v[1] + (by / bl) * d]
    };
    sd_triangle(px, py, shrink(a, b, c), shrink(b, a, c), shrink(c, a, b)) - round
}

/// Paint a filled disc in `colour` with a white glyph on top, returning
/// straight RGBA bytes. Split out of render() so tests can inspect pixels.
fn paint_level(colour: (u8, u8, u8), glyph: Glyph) -> Vec<u8> {
    let n = ICON_SIZE;
    let centre = n as f32 / 2.0;
    let disc_radius = n as f32 / 2.0 - 2.0;
    let scale = n as f32 / 96.0;

    let (solid, strokes, dots) = glyph_shapes(glyph);
    let solid: Option<([f32; 2], [f32; 2], [f32; 2], f32)> = solid.map(|(a, b, c, r)| {
        (
            [a[0] * scale, a[1] * scale],
            [b[0] * scale, b[1] * scale],
            [c[0] * scale, c[1] * scale],
            r * scale,
        )
    });
    let strokes: Vec<Stroke> = strokes
        .into_iter()
        .map(|(a, b, c, d, t)| (a * scale, b * scale, c * scale, d * scale, t * scale))
        .collect();
    let dots: Vec<Dot> = dots
        .into_iter()
        .map(|(x, y, r)| (x * scale, y * scale, r * scale))
        .collect();

    let (cr, cg, cb) = colour;
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);

    for y in 0..n {
        for x in 0..n {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);

            let from_centre = ((px - centre).powi(2) + (py - centre).powi(2)).sqrt();
            let disc = coverage(from_centre, disc_radius);
            if disc <= 0.0 {
                pixels.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            // White glyph coverage: the union of every stroke and dot -- or,
            // for a solid shape, the shape with those cut out of it.
            let mut cut: f32 = 0.0;
            for &(x0, y0, x1, y1, half) in &strokes {
                cut = cut.max(coverage(segment_distance(px, py, x0, y0, x1, y1), half));
            }
            for &(dx, dy, r) in &dots {
                let d = ((px - dx).powi(2) + (py - dy).powi(2)).sqrt();
                cut = cut.max(coverage(d, r));
            }
            let ink = match &solid {
                Some((a, b, c, round)) => {
                    coverage(sd_rounded_triangle(px, py, *a, *b, *c, *round), 0.0) * (1.0 - cut)
                }
                None => cut,
            };

            // Composite white over the level colour, then apply the disc's alpha.
            let mix = |c: u8| (c as f32 + (255.0 - c as f32) * ink).round() as u8;
            pixels.extend_from_slice(&[mix(cr), mix(cg), mix(cb), (disc * 255.0).round() as u8]);
        }
    }

    pixels
}

/// Draw a filled disc in `colour` with a white glyph on top, returning PNG bytes.
fn render(colour: (u8, u8, u8), glyph: Glyph) -> Vec<u8> {
    png::encode_rgba(ICON_SIZE, ICON_SIZE, &paint_level(colour, glyph))
}

/// Install `data` at `path` (write to a temporary name and rename, so a
/// concurrent reader never sees a half-written file), returning the path if
/// the file is usable afterwards.
fn cached(path: &Path, data: &[u8]) -> Option<PathBuf> {
    let dir = path.parent()?;
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("Could not create the icon cache: {e}");
        return None;
    }

    let tmp = dir.join(format!("{}.tmp", path.file_name()?.to_string_lossy()));
    if let Err(e) = fs::write(&tmp, data) {
        eprintln!("Could not write the icon: {e}");
        return None;
    }
    if let Err(e) = fs::rename(&tmp, path) {
        // Another process may have won the race, in which case the icon is
        // already there and perfectly good.
        if !fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
            eprintln!("Could not install the icon: {e}");
            return None;
        }
        let _ = fs::remove_file(&tmp);
    }

    Some(path.to_path_buf())
}

/// Bump when a level icon's design changes: the cache key includes it, so
/// an updated icon replaces the cached one instead of going stale.
const LEVEL_ICON_VERSION: u32 = 6;

/// Path to the cached icon for `level`, drawing it on first use.
///
/// Returns None if the icon cannot be written. A toast without an icon is
/// still perfectly useful, so this never fails the send.
pub fn level_icon(level: &Level) -> Option<PathBuf> {
    let path = icon_cache_dir().join(format!(
        "{}-v{LEVEL_ICON_VERSION}-{ICON_SIZE}.png",
        level.name
    ));
    if fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return Some(path);
    }
    cached(&path, &render(level.colour, level.glyph))
}

// ---------- The app icon ----------
//
// A hand-drawn reminder checklist: a wobbly periwinkle frame with one
// deliberate sketch gap, and two coloured rings paired with bars of text.
// Nothing else -- the background (including the inside of the frame) is
// transparent. Everything is expressed in the same 96px design space as the
// level icons and scaled to the requested size.

/// Bump when the design changes: the cache key includes it, so an updated
/// icon replaces the cached one instead of going stale.
const APP_ICON_VERSION: u32 = 2;

const PERIWINKLE: (u8, u8, u8) = (0x7B, 0x9B, 0xE0);
const ORANGE: (u8, u8, u8) = (0xF5, 0xA5, 0x5E);
const GREEN: (u8, u8, u8) = (0x55, 0xC9, 0xA6);

/// Smooth, deterministic perturbation of at most ~0.9 design units. What
/// makes an edge read as hand-drawn rather than CAD-drawn; the same seed
/// always yields the same line, so the cache stays stable.
fn wobble(x: f32, y: f32, seed: f32) -> f32 {
    0.6 * (x * 0.11 + y * 0.07 + seed).sin() * (y * 0.09 - x * 0.05 + 1.7 * seed).sin()
        + 0.3 * (x * 0.21 - y * 0.13 + 2.9 * seed).sin()
}

/// Signed distance to a (possibly rotated) rounded box, in design units.
/// Negative inside.
fn rbox(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32, rot: f32) -> f32 {
    let (c, s) = (rot.cos(), rot.sin());
    let (dx, dy) = (px - cx, py - cy);
    let (rx, ry) = (dx * c + dy * s, -dx * s + dy * c);
    let qx = rx.abs() - hw;
    let qy = ry.abs() - hh;
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + (qx.max(qy)).min(0.0) - r
}

/// Straight-alpha RGBA buffer in floats, so several anti-aliased layers can
/// composite before the final quantization.
struct Rgba {
    data: Vec<[f32; 4]>,
}

impl Rgba {
    fn new(n: u32) -> Self {
        Rgba {
            data: vec![[0.0; 4]; (n * n) as usize],
        }
    }

    /// Porter-Duff "over": source colour with coverage `a` (0..1) onto pixel
    /// `i`.
    fn over(&mut self, i: usize, rgb: (u8, u8, u8), a: f32) {
        if a <= 0.0 {
            return;
        }
        let dst = &mut self.data[i];
        let da = dst[3];
        let out_a = a + da * (1.0 - a);
        let inv = 1.0 / out_a;
        dst[0] = (rgb.0 as f32 * a + dst[0] * da * (1.0 - a)) * inv;
        dst[1] = (rgb.1 as f32 * a + dst[1] * da * (1.0 - a)) * inv;
        dst[2] = (rgb.2 as f32 * a + dst[2] * da * (1.0 - a)) * inv;
        dst[3] = out_a;
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.data.len() * 4);
        for [r, g, b, a] in &self.data {
            out.extend_from_slice(&[
                r.clamp(0.0, 255.0).round() as u8,
                g.clamp(0.0, 255.0).round() as u8,
                b.clamp(0.0, 255.0).round() as u8,
                (a * 255.0).round() as u8,
            ]);
        }
        out
    }
}

/// Paint the app icon into a buffer at `size` pixels. Split out of
/// render_app() so tests can inspect the pixels.
fn paint_app(size: u32) -> Rgba {
    let n = size;
    let scale = n as f32 / 96.0;
    let mut buf = Rgba::new(n);

    // The two list rows: a coloured ring (the "checkbox") and the y of the
    // bar of text next to it.
    let rows: [((u8, u8, u8), f32); 2] = [(ORANGE, 34.0), (GREEN, 62.0)];

    for y in 0..n {
        for x in 0..n {
            // Pixel centre, mapped back into the 96px design space so the
            // same geometry renders at any size.
            let (px, py) = ((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
            let i = (y * n + x) as usize;

            // Rings and their bars of text.
            for (row, (colour, cy)) in rows.iter().enumerate() {
                let w = wobble(px, py, 3.0 + row as f32);
                let dist = ((px - 31.0).powi(2) + (py - cy).powi(2)).sqrt();
                let ring = coverage(dist + 0.5 * w, 10.0) - coverage(dist + 0.3 * w, 5.0);
                buf.over(i, *colour, ring.clamp(0.0, 1.0));

                let bar = coverage(
                    segment_distance(px, py, 46.0, *cy, 68.0, *cy)
                        + 0.5 * wobble(px, py, 6.0 + row as f32),
                    2.4,
                );
                buf.over(i, PERIWINKLE, bar);
            }

            // The sketchy frame, with its one deliberate gap, drawn last so it
            // stays crisp.
            let frame =
                (rbox(px, py, 48.0, 48.0, 31.0, 33.0, 15.0, 0.0) + wobble(px, py, 3.1)).abs();
            let gap = segment_distance(px, py, 42.0, 15.0, 55.0, 15.0) + 0.6 * wobble(px, py, 9.0);
            buf.over(
                i,
                PERIWINKLE,
                coverage(frame, 2.6) * (1.0 - coverage(gap, 3.8)),
            );
        }
    }

    buf
}

/// Render the app icon at `size` pixels, returning PNG bytes.
fn render_app(size: u32) -> Vec<u8> {
    png::encode_rgba(size, size, &paint_app(size).to_bytes())
}

/// Path to the cached app icon (96x96 PNG), drawn on first use.
pub fn app_icon() -> Option<PathBuf> {
    let path = icon_cache_dir().join(format!("app-v{APP_ICON_VERSION}-{ICON_SIZE}.png"));
    if fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return Some(path);
    }
    cached(&path, &render_app(ICON_SIZE))
}

/// Path to the cached app icon as an .ico -- the format a Start Menu shortcut
/// needs -- bundled from one PNG per size.
pub fn app_icon_ico() -> Option<PathBuf> {
    let path = icon_cache_dir().join(format!("app-v{APP_ICON_VERSION}.ico"));
    if fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
        return Some(path);
    }
    let entries: Vec<(u32, Vec<u8>)> = [16u32, 24, 32, 48, 64, 128, 256]
        .into_iter()
        .map(|size| (size, render_app(size)))
        .collect();
    let data = crate::ico::encode(
        &entries
            .iter()
            .map(|(s, p)| (*s, p.as_slice()))
            .collect::<Vec<_>>(),
    );
    cached(&path, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The icon paints its design: opaque on a ring's band, transparent
    /// everywhere else -- middle, hole, and corners.
    #[test]
    fn app_icon_paints_the_design() {
        let buf = paint_app(96);
        let ring = &buf.data[(34 * 96 + 38) as usize];
        assert!(ring[3] > 0.99, "ring band should be opaque");
        let centre = &buf.data[(48 * 96 + 48) as usize];
        assert!(centre[3] < 0.01, "centre should be transparent");
        let corner = &buf.data[0];
        assert!(corner[3] < 0.01, "corner should be transparent");
    }

    /// The SDF keeps the right sign at the ambiguous case: a point beyond a
    /// corner is equidistant to the two edges meeting there, where a
    /// lexicographic (distance, cross) comparison would pick the wrong sign.
    #[test]
    fn sd_triangle_stays_outside_beyond_corners() {
        let a = [48.0f32, 5.0];
        let b = [13.0, 71.0];
        let c = [83.0, 71.0];
        assert!(sd_triangle(48.5, 48.5, a, b, c) < 0.0, "centre is inside");
        assert!(
            sd_triangle(18.5, 82.5, a, b, c) > 0.0,
            "beyond base-left corner is outside"
        );
        assert!(
            sd_triangle(48.5, 1.5, a, b, c) > 0.0,
            "beyond the apex is outside"
        );
        assert!(
            sd_rounded_triangle(18.5, 82.5, a, b, c, 5.0) > 0.0,
            "rounded: beyond corner is outside"
        );
        assert!(
            sd_rounded_triangle(48.5, 48.5, a, b, c, 5.0) < 0.0,
            "rounded: centre is inside"
        );
    }

    /// The warning glyph is a solid triangle with an exclamation mark cut
    /// out: white body, disc-coloured "!", transparent corner.
    #[test]
    fn warning_glyph_is_solid_with_cutout() {
        let level = crate::level::by_name("warning").unwrap();
        let px = paint_level(level.colour, level.glyph);
        let at = |x: u32, y: u32| &px[((y * ICON_SIZE + x) * 4) as usize..][..4];
        // Inside the triangle, clear of the "!": white.
        for (x, y) in [(38, 40), (48, 16), (48, 70), (60, 50)] {
            let p = at(x, y);
            assert!(
                p[0] > 250 && p[1] > 250 && p[2] > 250,
                "({x},{y}) should be white, got {p:?}"
            );
        }
        // The "!" bar and dot: disc colour, not white.
        for (x, y) in [(48, 35), (48, 58)] {
            let p = at(x, y);
            assert!(p[0] < 245, "({x},{y}) should be cut out, got {p:?}");
        }
        // Beyond the triangle's base-left corner: disc colour, not a stray
        // white sliver (regression for the SDF sign flip at corners).
        let p = at(18, 82);
        assert!(
            p[0] < 245,
            "(18,82) beyond the corner should be disc colour, got {p:?}"
        );
        // Disc corner: transparent.
        assert!(at(0, 0)[3] < 8);
    }
}
