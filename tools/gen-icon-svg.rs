// One-off generator: exports the app icon (as painted by src/icon.rs, design
// version 2) to app-icon.svg. Runs standalone from the repo root:
//
//     rustc -O tools/gen-icon-svg.rs -o target/gen-icon-svg && target/gen-icon-svg.exe
//
// The SVG is the README banner. If the design in src/icon.rs changes, update
// the constants here (or the seeds) to match, or the banner goes stale.
//
// The raster draws bands as |sdf + wobble| < half_width. A band whose edges
// are displaced by a smooth field is equivalent (to <1px) to stroking the
// undisturbed centreline displaced by -wobble along its normal, which is what
// this samples into SVG polylines.

fn wobble(x: f32, y: f32, seed: f32) -> f32 {
    0.6 * (x * 0.11 + y * 0.07 + seed).sin() * (y * 0.09 - x * 0.05 + 1.7 * seed).sin()
        + 0.3 * (x * 0.21 - y * 0.13 + 2.9 * seed).sin()
}

type Pt = (f32, f32);

fn fmt_pt((x, y): Pt) -> String {
    format!("{:.2} {:.2}", x, y)
}

fn path_from(points: &[Pt], close: bool) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        d.push_str(if i == 0 { "M" } else { "L" });
        d.push_str(&fmt_pt(*p));
        d.push(' ');
    }
    if close {
        d.push_str("Z ");
    }
    d
}

/// Sample the rounded-rect centreline (half 31x33, radius 15, centre 48,48),
/// clockwise from the top edge, with the top-edge gap (x 42..55) removed.
/// Returns displaced points: p - normal * wobble(p, 3.1).
fn frame_path() -> String {
    let step = 0.75;
    let mut pts: Vec<Pt> = Vec::new();

    let push = |pts: &mut Vec<Pt>, x: f32, y: f32, nx: f32, ny: f32| {
        let dx = -nx * wobble(x, y, 3.1);
        let dy = -ny * wobble(x, y, 3.1);
        pts.push((x + dx, y + dy));
    };

    // Top edge, right half: x 42 -> 64 (gap start -> corner start).
    let mut x = 42.0;
    while x <= 64.0 {
        push(&mut pts, x, 15.0, 0.0, -1.0);
        x += step;
    }
    // Top-right corner: centre (64,30), -90deg -> 0deg.
    let mut a = -std::f32::consts::FRAC_PI_2;
    while a <= 0.0 {
        push(&mut pts, 64.0 + 15.0 * a.cos(), 30.0 + 15.0 * a.sin(), a.cos(), a.sin());
        a += step / 15.0;
    }
    // Right edge: y 30 -> 66.
    let mut y = 30.0;
    while y <= 66.0 {
        push(&mut pts, 79.0, y, 1.0, 0.0);
        y += step;
    }
    // Bottom-right corner: centre (64,66), 0 -> 90deg.
    let mut a = 0.0;
    while a <= std::f32::consts::FRAC_PI_2 {
        push(&mut pts, 64.0 + 15.0 * a.cos(), 66.0 + 15.0 * a.sin(), a.cos(), a.sin());
        a += step / 15.0;
    }
    // Bottom edge: x 64 -> 32 (the corner arcs cover 32->79 on each side).
    let mut x = 64.0;
    while x >= 32.0 {
        push(&mut pts, x, 81.0, 0.0, 1.0);
        x -= step;
    }
    // Bottom-left corner: centre (32,66), 90 -> 180deg.
    let mut a = std::f32::consts::FRAC_PI_2;
    while a <= std::f32::consts::PI {
        push(&mut pts, 32.0 + 15.0 * a.cos(), 66.0 + 15.0 * a.sin(), a.cos(), a.sin());
        a += step / 15.0;
    }
    // Left edge: y 66 -> 30.
    let mut y = 66.0;
    while y >= 30.0 {
        push(&mut pts, 17.0, y, -1.0, 0.0);
        y -= step;
    }
    // Top-left corner: centre (32,30), 180 -> 270deg.
    let mut a = std::f32::consts::PI;
    while a <= 1.5 * std::f32::consts::PI {
        push(&mut pts, 32.0 + 15.0 * a.cos(), 30.0 + 15.0 * a.sin(), a.cos(), a.sin());
        a += step / 15.0;
    }
    // Top edge, left half: x 32 -> 55 (gap end).
    let mut x = 32.0;
    while x <= 55.0 {
        push(&mut pts, x, 15.0, 0.0, -1.0);
        x += step;
    }

    path_from(&pts, false)
}

/// Wobbled ring: centre (cx,cy), band outer 10 / inner 5. Centreline radius
/// 7.5 displaced by -0.4 * wobble (the band's mid-edge), sampled per angle.
fn ring_path(cx: f32, cy: f32, seed: f32) -> String {
    let mut pts: Vec<Pt> = Vec::new();
    let mut a = 0.0f32;
    while a < std::f32::consts::TAU {
        let (ux, uy) = (a.cos(), a.sin());
        let base = (cx + 7.5 * ux, cy + 7.5 * uy);
        let r = 7.5 - 0.4 * wobble(base.0, base.1, seed);
        pts.push((cx + r * ux, cy + r * uy));
        a += 0.05;
    }
    path_from(&pts, true)
}

/// Wobbled bar: segment (x0,y)->(x1,y), half-width 2.4; centreline shifted
/// vertically by 0.5 * wobble per sample.
fn bar_path(x0: f32, x1: f32, y: f32, seed: f32) -> String {
    let mut pts: Vec<Pt> = Vec::new();
    let mut x = x0;
    while x <= x1 {
        pts.push((x, y + 0.5 * wobble(x, y, seed)));
        x += 1.0;
    }
    path_from(&pts, false)
}

fn main() {
    let mut svg = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"96\" height=\"96\" viewBox=\"0 0 96 96\">\n",
    );
    let frame = frame_path();
    svg.push_str(&format!(
        "  <path d=\"{frame}\" fill=\"none\" stroke=\"#7B9BE0\" stroke-width=\"5.2\" stroke-linecap=\"butt\"/>\n"
    ));
    let ring1 = ring_path(31.0, 34.0, 3.0);
    svg.push_str(&format!(
        "  <path d=\"{ring1}\" fill=\"none\" stroke=\"#F5A55E\" stroke-width=\"5\" stroke-linejoin=\"round\"/>\n"
    ));
    let ring2 = ring_path(31.0, 62.0, 4.0);
    svg.push_str(&format!(
        "  <path d=\"{ring2}\" fill=\"none\" stroke=\"#55C9A6\" stroke-width=\"5\" stroke-linejoin=\"round\"/>\n"
    ));
    let bar1 = bar_path(46.0, 68.0, 34.0, 6.0);
    svg.push_str(&format!(
        "  <path d=\"{bar1}\" fill=\"none\" stroke=\"#7B9BE0\" stroke-width=\"4.8\" stroke-linecap=\"round\"/>\n"
    ));
    let bar2 = bar_path(46.0, 68.0, 62.0, 7.0);
    svg.push_str(&format!(
        "  <path d=\"{bar2}\" fill=\"none\" stroke=\"#7B9BE0\" stroke-width=\"4.8\" stroke-linecap=\"round\"/>\n"
    ));
    svg.push_str("</svg>\n");

    std::fs::write("app-icon.svg", &svg).unwrap();
    println!("wrote app-icon.svg ({} bytes)", svg.len());
}
