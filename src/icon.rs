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
fn glyph_shapes(glyph: Glyph) -> (Vec<Stroke>, Vec<Dot>) {
    match glyph {
        Glyph::Cross => (
            vec![(33.0, 33.0, 63.0, 63.0, 4.5), (63.0, 33.0, 33.0, 63.0, 4.5)],
            vec![],
        ),
        Glyph::Check => (
            vec![(29.0, 49.0, 42.0, 63.0, 4.5), (42.0, 63.0, 68.0, 33.0, 4.5)],
            vec![],
        ),
        Glyph::Bang => (vec![(48.0, 25.0, 48.0, 55.0, 4.5)], vec![(48.0, 69.0, 5.5)]),
        Glyph::Info => (vec![(48.0, 44.0, 48.0, 71.0, 4.5)], vec![(48.0, 28.0, 5.5)]),
    }
}

/// Draw a filled disc in `colour` with a white glyph on top, returning PNG bytes.
fn render(colour: (u8, u8, u8), glyph: Glyph) -> Vec<u8> {
    let n = ICON_SIZE;
    let centre = n as f32 / 2.0;
    let disc_radius = n as f32 / 2.0 - 2.0;
    let scale = n as f32 / 96.0;

    let (strokes, dots) = glyph_shapes(glyph);
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

            // White glyph coverage: the union of every stroke and dot.
            let mut ink: f32 = 0.0;
            for &(x0, y0, x1, y1, half) in &strokes {
                ink = ink.max(coverage(segment_distance(px, py, x0, y0, x1, y1), half));
            }
            for &(dx, dy, r) in &dots {
                let d = ((px - dx).powi(2) + (py - dy).powi(2)).sqrt();
                ink = ink.max(coverage(d, r));
            }

            // Composite white over the level colour, then apply the disc's alpha.
            let mix = |c: u8| (c as f32 + (255.0 - c as f32) * ink).round() as u8;
            pixels.extend_from_slice(&[mix(cr), mix(cg), mix(cb), (disc * 255.0).round() as u8]);
        }
    }

    png::encode_rgba(n, n, &pixels)
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

/// Path to the cached icon for `level`, drawing it on first use.
///
/// Returns None if the icon cannot be written. A toast without an icon is
/// still perfectly useful, so this never fails the send.
pub fn level_icon(level: &Level) -> Option<PathBuf> {
    let path = icon_cache_dir().join(format!("{}-{ICON_SIZE}.png", level.name));
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
}
