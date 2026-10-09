//! Fitting a shaped string onto a line's ink: size, baseline, start and tracking by coordinate descent on the
//! soft IoU of rendered coverage against the ink's, then per-glyph nudges along the line. The renderer is
//! tiny-skia, anti-aliased, at 1×: the same rasteriser the engine's refine stage uses.
use super::glyphs::Shaped;
use super::ink::LineInk;
use super::{Fit, NUDGE_MAX};
use tiny_skia::{FillRule, Paint, Pixmap, Transform};

/// Glyph origins along the line for a fit: x of each glyph's origin in source pixels.
fn origins(shaped: &Shaped, fit: &Fit) -> Vec<f32> {
    let mut x = fit.x;
    let mut out = Vec::with_capacity(shaped.glyphs.len());
    for (i, g) in shaped.glyphs.iter().enumerate() {
        let nudge = fit.nudges.get(i).copied().unwrap_or(0.0);
        out.push(x + g.x_offset * fit.size + nudge);
        x += g.advance * fit.size + fit.tracking;
    }
    out
}

/// Renders the shaped string at `fit` into a coverage buffer the size of the ink box (ink.width × ink.height), 0..1.
pub fn render_coverage(shaped: &Shaped, fit: &Fit, ink: &LineInk) -> Vec<f32> {
    let Some(mut pm) = Pixmap::new(ink.width, ink.height) else { return vec![0.0; (ink.width * ink.height) as usize] };
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    for (g, ox) in shaped.glyphs.iter().zip(origins(shaped, fit)) {
        let Some(path) = &g.path else { continue };
        // em units y up → source pixels y down, into the ink box's frame
        let t = Transform::from_row(fit.size, 0.0, 0.0, -fit.size, ox - ink.left as f32, fit.baseline - ink.top as f32);
        pm.fill_path(path, &paint, FillRule::Winding, t, None);
    }
    pm.data().chunks(4).map(|p| p[3] as f32 / 255.0).collect()
}

/// Soft IoU: Σ min / Σ max over the two buffers.
pub fn soft_iou(a: &[f32], b: &[f32]) -> f32 {
    let (mut inter, mut union) = (0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        inter += x.min(*y);
        union += x.max(*y);
    }
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn to_linear(v: f32) -> f32 {
    let c = v / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(l: f32) -> f32 {
    let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    c * 255.0
}

/// `line_ink` places a pixel between backdrop and ink in linear RGB, but a rasteriser (the webview that will show
/// the outlines, tiny-skia here, whatever drew the source) blends alpha on the encoded values: a half-covered
/// pixel is the mean of the two sRGB colours, and reads 0.8 in linear light. The coverage the glyph fill must
/// have to reproduce the pixel is the position along the encoded colours, from every channel that moves.
fn encoded_coverage(ink: &LineInk) -> Vec<f32> {
    let (b, k) = (ink.backdrop.map(f32::from), ink.colour.map(f32::from));
    let channels: Vec<usize> = (0..3).filter(|&c| (k[c] - b[c]).abs() >= 8.0).collect();
    if channels.is_empty() {
        return ink.coverage.clone();
    }
    let lin: Vec<(f32, f32)> = channels.iter().map(|&c| (to_linear(b[c]), to_linear(k[c]))).collect();
    ink.coverage
        .iter()
        .map(|&t| {
            if t <= 0.0 || t >= 1.0 {
                return t;
            }
            let a: f32 = channels.iter().zip(&lin).map(|(&c, &(lb, lk))| (to_srgb(lb + t * (lk - lb)) - b[c]) / (k[c] - b[c])).sum::<f32>() / channels.len() as f32;
            a.clamp(0.0, 1.0)
        })
        .collect()
}

/// The ink's coverage with its backdrop's own residue taken out. `line_ink` measures coverage against one backdrop
/// colour, so a backdrop that ramps across the box (a gradient) leaves its far end reading as faint ink, and that
/// floor is in the union of every score. A column of the box always holds blank rows above and below the text, so
/// each column's minimum is the backdrop's level there.
fn target_of(ink: &LineInk) -> Vec<f32> {
    let w = (ink.width as usize).max(1);
    let mut out = encoded_coverage(ink);
    let mut floor = vec![1f32; w];
    for row in out.chunks(w) {
        for (f, v) in floor.iter_mut().zip(row) {
            *f = f.min(*v);
        }
    }
    for row in out.chunks_mut(w) {
        for (v, f) in row.iter_mut().zip(&floor) {
            let f = f.min(0.5);
            *v = ((*v - f) / (1.0 - f)).clamp(0.0, 1.0);
        }
    }
    out
}

/// What a fit is scored against: the ink's clean coverage, and the same blurred for the coarse stage.
struct Goal {
    sharp: Vec<f32>,
    soft: Vec<f32>,
    width: usize,
    height: usize,
}

/// What a nudge must add to the whole line's IoU to be kept: a smaller gain is the noise a nudge would fit, and
/// a run of such nudges all one way is a shift of the line, which belongs to `x`.
const NUDGE_GAIN: f32 = 2e-3;

/// Radius of the box blur that widens the basin in the coarse stage, px.
const COARSE_BLUR: usize = 3;

/// A separable box blur of radius `r` (zero beyond the edges).
fn blur(src: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let norm = 1.0 / (2 * r + 1) as f32;
    let mut tmp = vec![0f32; src.len()];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let mut acc: f32 = row.iter().take(r + 1).sum();
        for x in 0..w {
            tmp[y * w + x] = acc * norm;
            if x + r + 1 < w {
                acc += row[x + r + 1];
            }
            if x >= r {
                acc -= row[x - r];
            }
        }
    }
    let mut out = vec![0f32; src.len()];
    for x in 0..w {
        let mut acc: f32 = (0..=r.min(h - 1)).map(|y| tmp[y * w + x]).sum();
        for y in 0..h {
            out[y * w + x] = acc * norm;
            if y + r + 1 < h {
                acc += tmp[(y + r + 1) * w + x];
            }
            if y >= r {
                acc -= tmp[(y - r) * w + x];
            }
        }
    }
    out
}

fn goal_of(ink: &LineInk) -> Goal {
    let (width, height) = (ink.width as usize, ink.height as usize);
    let sharp = target_of(ink);
    let soft = blur(&sharp, width, height, COARSE_BLUR);
    Goal { sharp, soft, width, height }
}

fn score(shaped: &Shaped, fit: &Fit, ink: &LineInk, goal: &Goal, coarse: bool) -> f32 {
    let rendered = render_coverage(shaped, fit, ink);
    if coarse {
        soft_iou(&blur(&rendered, goal.width, goal.height, COARSE_BLUR), &goal.soft)
    } else {
        soft_iou(&rendered, &goal.sharp)
    }
}

/// Coordinate descent on size, baseline, start and tracking at each of `steps`, on the blurred coverage while
/// `coarse` (the sharp optimum sits inside the blurred one's basin, and a line a glyph off finds it), else the
/// sharp. `fit.iou` is always the sharp score afterwards.
fn descend(shaped: &Shaped, ink: &LineInk, goal: &Goal, fit: &mut Fit, steps: &[f32], coarse: bool) {
    // a change of size that keeps the line's two ends where they are: size and tracking trade against each other
    // along the line's width, and moving either alone climbs out of that valley's floor
    let n = shaped.glyphs.len();
    let first_lsb = shaped.glyphs.first().and_then(|g| g.path.as_ref()).map(|p| p.bounds().left()).unwrap_or(0.0);
    let last_rsb = shaped.glyphs.last().map(|g| g.advance - g.path.as_ref().map(|p| p.bounds().right()).unwrap_or(g.advance)).unwrap_or(0.0);
    let span = shaped.advance() - first_lsb - last_rsb;
    let mut current = score(shaped, fit, ink, goal, coarse);
    for &step in steps {
        loop {
            let mut improved = false;
            for p in 0..7 {
                for dir in [-1.0f32, 1.0] {
                    let mut trial = fit.clone();
                    match p {
                        0 => trial.size += step * dir * 0.5,
                        1 => trial.baseline += step * dir,
                        2 => trial.x += step * dir,
                        3 => trial.tracking += step * dir * 0.5,
                        4 => {
                            let d = step * dir * 0.5;
                            trial.size += d;
                            trial.x -= first_lsb * d;
                            if n > 1 {
                                trial.tracking -= span * d / (n - 1) as f32;
                            }
                        }
                        // size or tracking changed about the line's centre: the width moves, the middle stays
                        5 => {
                            let d = step * dir * 0.5;
                            trial.size += d;
                            trial.x -= (first_lsb + span / 2.0) * d;
                        }
                        _ => {
                            let d = step * dir * 0.5;
                            trial.tracking += d;
                            trial.x -= d * (n as f32 - 1.0) / 2.0;
                        }
                    }
                    if trial.size <= 1.0 {
                        continue;
                    }
                    let s = score(shaped, &trial, ink, goal, coarse);
                    if s > current + 1e-5 {
                        *fit = trial;
                        current = s;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }
    fit.iou = score(shaped, fit, ink, goal, false);
}

/// Per-glyph nudges, 0.25 px steps up to NUDGE_MAX, kept only where the whole line's IoU rises. The first glyph is
/// never nudged: it is where `x` is, so a shift of the whole line stays in `x` and the nudges are what is left.
fn nudge(shaped: &Shaped, ink: &LineInk, goal: &Goal, fit: &mut Fit) {
    for i in 1..shaped.glyphs.len() {
        if shaped.glyphs[i].path.is_none() {
            continue;
        }
        let mut best = fit.clone();
        let mut d = -NUDGE_MAX;
        while d <= NUDGE_MAX + 1e-6 {
            let mut trial = fit.clone();
            trial.nudges[i] = d;
            trial.iou = score(shaped, &trial, ink, goal, false);
            if trial.iou > best.iou + NUDGE_GAIN {
                best = trial;
            }
            d += 0.25;
        }
        *fit = best;
    }
}

/// The initial guess from the ink bounds, then coordinate descent on (size, baseline, x, tracking), then per-glyph
/// nudges. The fit's x, baseline are in source pixels (ink.left/top added); `iou` is the final score. None when
/// the ink is empty.
///
/// The guess reads the size and the baseline off the string's own outlines (their union's height, and how far it
/// reaches below the baseline), so an all-caps line, a line with descenders and a font whose rounds overshoot all
/// start where the ink is; `_all_caps` is kept for the caller's contract and for a string whose outlines are empty.
/// The descent is started from a few sizes either side, since a serif's hairlines and a wide string hold a
/// descent that began a few per cent off in a smaller-tracking, larger-size optimum.
pub fn fit_line(shaped: &Shaped, ink: &LineInk, all_caps: bool) -> Option<Fit> {
    let (l, t, r, b) = ink.ink_bounds?;
    let goal = goal_of(ink);
    let ink_h = (b - t) as f32;
    let ink_w = (r - l) as f32;
    let em_h = match shaped.bbox_height() {
        h if h > 0.0 => h,
        _ if all_caps => shaped.cap_height,
        _ => shaped.x_height,
    }
    .max(0.1);
    let depth = shaped.glyphs.iter().filter_map(|g| g.path.as_ref()).map(|p| -p.bounds().top()).fold(f32::MIN, f32::max).max(0.0);
    let n = shaped.glyphs.len().max(1) as f32;
    let first_lsb = shaped.glyphs.first().and_then(|g| g.path.as_ref()).map(|p| p.bounds().left()).unwrap_or(0.0);
    let last_rsb = shaped.glyphs.last().map(|g| g.advance - g.path.as_ref().map(|p| p.bounds().right()).unwrap_or(g.advance)).unwrap_or(0.0);
    let glyph_span = shaped.advance() - first_lsb - last_rsb;
    let mut best: Option<Fit> = None;
    for scale in [1.0f32, 0.96, 1.04, 0.92, 1.08] {
        let size = ink_h / em_h * scale;
        let tracking = if n > 1.0 { (ink_w - glyph_span * size) / (n - 1.0) } else { 0.0 };
        let mut fit = Fit {
            size,
            // the ink's bottom is the outlines' lowest point, `depth` below the baseline
            baseline: (ink.top + b) as f32 - depth * size,
            x: (ink.left + l) as f32 - first_lsb * size,
            tracking: tracking.max(-size * 0.1),
            nudges: vec![0.0; shaped.glyphs.len()],
            iou: 0.0,
        };
        descend(shaped, ink, &goal, &mut fit, &[2.0, 1.0, 0.5], true);
        descend(shaped, ink, &goal, &mut fit, &[0.5], false);
        if best.as_ref().is_none_or(|b| fit.iou > b.iou) {
            best = Some(fit);
        }
    }
    let mut fit = best?;
    descend(shaped, ink, &goal, &mut fit, &[0.25, 0.1], false);
    for _ in 0..2 {
        nudge(shaped, ink, &goal, &mut fit);
        descend(shaped, ink, &goal, &mut fit, &[0.25, 0.1], false);
    }
    Some(fit)
}

fn fmt(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

/// The glyph outlines at `fit`, as an SVG path `d` in source pixels (y down), every number to 2 decimals.
pub fn path_data(shaped: &Shaped, fit: &Fit) -> String {
    use tiny_skia::PathSegment;
    let mut d = String::new();
    for (g, ox) in shaped.glyphs.iter().zip(origins(shaped, fit)) {
        let Some(path) = &g.path else { continue };
        let t = Transform::from_row(fit.size, 0.0, 0.0, -fit.size, ox, fit.baseline);
        let Some(p) = path.clone().transform(t) else { continue };
        for seg in p.segments() {
            match seg {
                PathSegment::MoveTo(p) => d.push_str(&format!("M{} {}", fmt(p.x), fmt(p.y))),
                PathSegment::LineTo(p) => d.push_str(&format!("L{} {}", fmt(p.x), fmt(p.y))),
                PathSegment::QuadTo(c, p) => d.push_str(&format!("Q{} {} {} {}", fmt(c.x), fmt(c.y), fmt(p.x), fmt(p.y))),
                PathSegment::CubicTo(a, b, p) => d.push_str(&format!("C{} {} {} {} {} {}", fmt(a.x), fmt(a.y), fmt(b.x), fmt(b.y), fmt(p.x), fmt(p.y))),
                PathSegment::Close => d.push('Z'),
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_iou_of_identical_buffers_is_one_and_of_disjoint_is_zero() {
        assert!((soft_iou(&[0.5, 1.0, 0.0], &[0.5, 1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(soft_iou(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn the_box_blur_keeps_a_flat_field_flat_and_spreads_a_point() {
        let flat = blur(&[1.0; 25], 5, 5, 1);
        assert!((flat[12] - 1.0).abs() < 1e-6);
        let mut dot = vec![0.0; 25];
        dot[12] = 9.0;
        let b = blur(&dot, 5, 5, 1);
        assert!((b[12] - 1.0).abs() < 1e-6 && (b[6] - 1.0).abs() < 1e-6 && b[0] == 0.0);
    }

    #[test]
    fn a_linear_half_is_an_encoded_four_fifths_and_the_ends_stay() {
        let ink = LineInk { left: 0, top: 0, width: 3, height: 1, coverage: vec![0.0, 0.79, 1.0], mask: vec![false, true, true], backdrop: [255; 3], colour: [0; 3], spread: 0.0, segments: Vec::new(), ink_bounds: None };
        let c = encoded_coverage(&ink);
        // black on white: linear light 0.79 of the way to black is the encoded mid-grey, 0.5 covered
        assert!((c[1] - 0.5).abs() < 0.02, "{}", c[1]);
        assert_eq!((c[0], c[2]), (0.0, 1.0));
    }

    #[test]
    fn path_data_is_the_outline_in_source_pixels_y_down() {
        use super::super::glyphs::ShapedGlyph;
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_ltrb(0.0, 0.0, 0.5, 0.7).unwrap());
        let shaped = Shaped { glyphs: vec![ShapedGlyph { ch: 'I', path: pb.finish(), advance: 0.6, x_offset: 0.0 }], cap_height: 0.7, x_height: 0.5 };
        let fit = Fit { size: 10.0, baseline: 20.0, x: 5.0, tracking: 0.0, nudges: vec![0.0], iou: 1.0 };
        assert_eq!(path_data(&shaped, &fit), "M5 20L10 20L10 13L5 13Z");
    }

    #[test]
    fn numbers_are_written_to_two_decimals() {
        assert_eq!(fmt(12.345), "12.35");
        assert_eq!(fmt(3.0), "3");
        assert_eq!(fmt(-0.001), "0");
    }
}
