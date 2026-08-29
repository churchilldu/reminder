//! Level icons, drawn procedurally and cached on disk.
//!
//! A toast needs an image *file* -- it cannot reference an icon inside a DLL --
//! so rather than ship binary assets the four icons are drawn on first use and
//! cached under %LOCALAPPDATA%\reminder-rs\icons.

use std::env;
use std::fs;
use std::path::PathBuf;

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
    base.join("reminder-rs")
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

/// Path to the cached icon for `level`, drawing it on first use.
///
/// Returns None if the icon cannot be written. A toast without an icon is
/// still perfectly useful, so this never fails the send.
pub fn level_icon(level: &Level) -> Option<PathBuf> {
    let dir = icon_cache_dir();
    let path = dir.join(format!("{}-{}.png", level.name, ICON_SIZE));

    let usable = fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false);
    if usable {
        return Some(path);
    }

    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("Could not create the icon cache: {e}");
        return None;
    }

    // Write to a temporary name and rename, so a concurrent reader never sees
    // a half-written file.
    let data = render(level.colour, level.glyph);
    let tmp = dir.join(format!("{}-{}.png.tmp", level.name, ICON_SIZE));
    if let Err(e) = fs::write(&tmp, &data) {
        eprintln!("Could not write the {} icon: {e}", level.name);
        return None;
    }
    if let Err(e) = fs::rename(&tmp, &path) {
        // Another process may have won the race, in which case the icon is
        // already there and perfectly good.
        if !fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
            eprintln!("Could not install the {} icon: {e}", level.name);
            return None;
        }
        let _ = fs::remove_file(&tmp);
    }

    Some(path)
}
