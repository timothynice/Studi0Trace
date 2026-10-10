//! Fitting a shaped string onto a line's ink: size, baseline, start and tracking by coordinate descent on the
//! soft IoU of rendered coverage against the ink's, then per-glyph nudges along the line. The renderer is
//! tiny-skia, anti-aliased, at 1×: the same rasteriser the engine's refine stage uses.
use super::glyphs::Shaped;
use super::ink::LineInk;
use super::{Fit, NUDGE_MAX};
use tiny_skia::{FillRule, Paint, Pixmap, Transform};

/// Glyph origins along the line for a fit: x of each glyph's origin in source pixels, nudges included.
pub fn origins(shaped: &Shaped, fit: &Fit) -> Vec<f32> {
    let mut x = fit.x;
    let mut out = Vec::with_capacity(shaped.glyphs.len());
    for (i, g) in shaped.glyphs.iter().enumerate() {
        let nudge = fit.nudges.get(i).copied().unwrap_or(0.0);
        out.push(x + g.x_offset * fit.size + nudge);
        x += g.advance * fit.size + fit.tracking;
    }
    out
}

thread_local! {
    /// The pixmap every render of this thread draws into, reallocated only when a render needs a different size: a
    /// fit renders the line some thousands of times.
    static CANVAS: std::cell::RefCell<Option<Pixmap>> = const { std::cell::RefCell::new(None) };
}

/// Draws the shaped string at `fit` into a `w`×`h` canvas whose top-left is source pixel (`x0`, `y0`), and hands
/// `read` its pixels (premultiplied RGBA, the glyphs white: alpha is coverage). A glyph wholly outside the canvas
/// is not drawn.
fn rendered<R>(shaped: &Shaped, fit: &Fit, x0: f32, y0: f32, w: u32, h: u32, read: impl FnOnce(&[u8]) -> R) -> R {
    CANVAS.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.as_ref().is_none_or(|pm| pm.width() != w || pm.height() != h) {
            *slot = Pixmap::new(w.max(1), h.max(1));
        }
        let Some(pm) = slot.as_mut() else { return read(&[]) };
        pm.fill(tiny_skia::Color::TRANSPARENT);
        let mut paint = Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        paint.anti_alias = true;
        for (g, ox) in shaped.glyphs.iter().zip(origins(shaped, fit)) {
            let Some(path) = &g.path else { continue };
            let b = path.bounds();
            if ox + b.right() * fit.size < x0 - 1.0 || ox + b.left() * fit.size > x0 + w as f32 + 1.0 {
                continue;
            }
            // em units y up → source pixels y down, into the canvas's frame
            let t = Transform::from_row(fit.size, 0.0, 0.0, -fit.size, ox - x0, fit.baseline - y0);
            pm.fill_path(path, &paint, FillRule::Winding, t, None);
        }
        read(pm.data())
    })
}

/// Renders the shaped string at `fit` into a coverage buffer the size of the ink box (ink.width × ink.height), 0..1.
pub fn render_coverage(shaped: &Shaped, fit: &Fit, ink: &LineInk) -> Vec<f32> {
    if ink.width == 0 || ink.height == 0 {
        return Vec::new();
    }
    rendered(shaped, fit, ink.left as f32, ink.top as f32, ink.width, ink.height, |px| px.chunks(4).map(|p| p[3] as f32 / 255.0).collect())
}

/// Σ min and Σ max of the drawn coverage against the goal's sharp target over the ink box's columns `x0..x1`.
fn window_sums(shaped: &Shaped, fit: &Fit, ink: &LineInk, goal: &Goal, x0: usize, x1: usize) -> (f32, f32) {
    let w = x1.saturating_sub(x0);
    if w == 0 || goal.height == 0 {
        return (0.0, 0.0);
    }
    rendered(shaped, fit, (ink.left as usize + x0) as f32, ink.top as f32, w as u32, goal.height as u32, |px| {
        let (mut inter, mut union) = (0f32, 0f32);
        for y in 0..goal.height {
            let row = &goal.sharp[y * goal.width + x0..y * goal.width + x1];
            for (x, g) in row.iter().enumerate() {
                let a = px.get((y * w + x) * 4 + 3).copied().unwrap_or(0) as f32 / 255.0;
                inter += a.min(*g);
                union += a.max(*g);
            }
        }
        (inter, union)
    })
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
    if coarse {
        soft_iou(&blur(&render_coverage(shaped, fit, ink), goal.width, goal.height, COARSE_BLUR), &goal.soft)
    } else {
        let (inter, union) = window_sums(shaped, fit, ink, goal, 0, goal.width);
        if union <= 0.0 { 0.0 } else { inter / union }
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

/// Per-glyph nudges, 0.25 px steps up to NUDGE_MAX either side of where the glyph is (the tracking model's place,
/// or the ink layout's), kept only where the whole line's IoU rises. The first glyph is never nudged: it is where
/// `x` is, so a shift of the whole line stays in `x` and the nudges are what is left. A nudge changes the pixels
/// round its own glyph only, so each trial redraws that window and the line's IoU is updated from the window's
/// sums: the same score, at the cost of one glyph rather than the line.
fn nudge(shaped: &Shaped, ink: &LineInk, goal: &Goal, fit: &mut Fit) {
    let (mut inter, mut union) = window_sums(shaped, fit, ink, goal, 0, goal.width);
    for i in 1..shaped.glyphs.len() {
        let Some(path) = &shaped.glyphs[i].path else { continue };
        let b = path.bounds();
        let ox = origins(shaped, fit)[i] - ink.left as f32;
        let x0 = (ox + b.left() * fit.size - NUDGE_MAX - 2.0).floor().max(0.0) as usize;
        let x1 = ((ox + b.right() * fit.size + NUDGE_MAX + 2.0).ceil().max(0.0) as usize).min(goal.width);
        if x0 >= x1 {
            continue;
        }
        let (bi, bu) = window_sums(shaped, fit, ink, goal, x0, x1);
        let iou = |i2: f32, u2: f32| {
            let u = union - bu + u2;
            if u <= 0.0 { 0.0 } else { (inter - bi + i2) / u }
        };
        let base = fit.nudges[i];
        let mut best = (iou(bi, bu), base, bi, bu);
        let mut d = -NUDGE_MAX;
        while d <= NUDGE_MAX + 1e-6 {
            let mut trial = fit.clone();
            trial.nudges[i] = base + d;
            let (ti, tu) = window_sums(shaped, &trial, ink, goal, x0, x1);
            let s = iou(ti, tu);
            if s > best.0 + NUDGE_GAIN {
                best = (s, base + d, ti, tu);
            }
            d += 0.25;
        }
        fit.nudges[i] = best.1;
        inter += best.2 - bi;
        union += best.3 - bu;
    }
    fit.iou = score(shaped, fit, ink, goal, false);
}

/// Each glyph's ink segment, when the ink's segments number the string's outlined, non-space glyphs one to one
/// (in order); None otherwise (letters that touch, a stray mark), and the tracking model alone lays the line out.
fn glyph_segments(shaped: &Shaped, ink: &LineInk) -> Option<Vec<Option<(u32, u32)>>> {
    let solid = shaped.glyphs.iter().filter(|g| g.path.is_some() && !g.ch.is_whitespace()).count();
    if solid == 0 || solid != ink.segments.len() {
        return None;
    }
    let mut next = ink.segments.iter();
    Some(shaped.glyphs.iter().map(|g| if g.path.is_some() && !g.ch.is_whitespace() { next.next().copied() } else { None }).collect())
}

/// The fit that puts each glyph's outline centred on its ink segment at `size`, shifted by `dx`: AI-made logos
/// space letters unevenly and their word gaps are wider than their letter gaps, which one tracking value cannot
/// model and ±2 px nudges cannot absorb. The centre, not the left edge, is matched, so a candidate a little
/// heavier or lighter than the ink still sits on it. It is written in the tracking model's terms: `x` and
/// `tracking` are the least-squares line through the glyphs' places (so `tracking` is the mean gap) and each
/// glyph's nudge is what is left.
fn ink_layout(shaped: &Shaped, ink: &LineInk, segs: &[Option<(u32, u32)>], size: f32, baseline: f32, dx: f32) -> Fit {
    let n = shaped.glyphs.len();
    // each placed glyph: (index, origin the ink asks for, origin the tracking model puts it at with x = tracking = 0)
    let mut placed: Vec<(f32, f32, f32)> = Vec::new();
    let mut run = 0f32;
    for (i, g) in shaped.glyphs.iter().enumerate() {
        let model = run + g.x_offset * size;
        if let (Some((l, r)), Some(path)) = (segs[i], &g.path) {
            let b = path.bounds();
            let centre = ink.left as f32 + (l + r) as f32 / 2.0 + dx;
            placed.push((i as f32, centre - (b.left() + b.right()) / 2.0 * size, model));
        }
        run += g.advance * size;
    }
    let m = placed.len() as f32;
    let (mi, mr) = (placed.iter().map(|p| p.0).sum::<f32>() / m, placed.iter().map(|p| p.1 - p.2).sum::<f32>() / m);
    let var: f32 = placed.iter().map(|p| (p.0 - mi).powi(2)).sum();
    let tracking = if var > 0.0 { placed.iter().map(|p| (p.0 - mi) * (p.1 - p.2 - mr)).sum::<f32>() / var } else { 0.0 };
    let x = mr - tracking * mi;
    let mut nudges = vec![0.0; n];
    for &(i, want, model) in &placed {
        nudges[i as usize] = want - (x + model + tracking * i);
    }
    Fit { size, baseline, x, tracking, nudges, iou: 0.0, stroke: 0.0 }
}

/// Coordinate descent on size, baseline and the shift of an ink layout.
fn descend_ink(shaped: &Shaped, ink: &LineInk, goal: &Goal, segs: &[Option<(u32, u32)>], p: &mut [f32; 3], steps: &[f32], coarse: bool) -> f32 {
    let mut current = score(shaped, &ink_layout(shaped, ink, segs, p[0], p[1], p[2]), ink, goal, coarse);
    for &step in steps {
        loop {
            let mut improved = false;
            for k in 0..3 {
                for dir in [-1.0f32, 1.0] {
                    let mut t = *p;
                    t[k] += step * dir * if k == 0 { 0.5 } else { 1.0 };
                    if t[0] <= 1.0 {
                        continue;
                    }
                    let s = score(shaped, &ink_layout(shaped, ink, segs, t[0], t[1], t[2]), ink, goal, coarse);
                    if s > current + 1e-5 {
                        *p = t;
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
    current
}

/// The starting fits for a line: the tracking model from each of `scales` of the guess and, when every glyph has
/// its segment, the ink layout from the first three, each descended on the blurred coverage and then the sharp.
/// Best tracking start first, then the best ink layout.
fn starts(shaped: &Shaped, ink: &LineInk, goal: &Goal, all_caps: bool, scales: &[f32]) -> Vec<Fit> {
    let Some((l, t, r, b)) = ink.ink_bounds else { return Vec::new() };
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
    for scale in scales {
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
            stroke: 0.0,
        };
        descend(shaped, ink, goal, &mut fit, &[2.0, 1.0, 0.5], true);
        descend(shaped, ink, goal, &mut fit, &[0.5], false);
        if best.as_ref().is_none_or(|b| fit.iou > b.iou) {
            best = Some(fit);
        }
    }
    let mut out: Vec<Fit> = best.into_iter().collect();
    // the ink layout, when every glyph has its segment, from the same guesses (on evenly spaced text the tracking
    // model's uniform spacing is the truth, and the segments' whole-pixel extents only add noise to it, so both go on)
    if let Some(segs) = glyph_segments(shaped, ink) {
        let mut laid: Option<Fit> = None;
        for scale in scales.iter().take(3) {
            let size = ink_h / em_h * scale;
            let mut p = [size, (ink.top + b) as f32 - depth * size, 0.0];
            descend_ink(shaped, ink, goal, &segs, &mut p, &[2.0, 1.0, 0.5], true);
            let iou = descend_ink(shaped, ink, goal, &segs, &mut p, &[0.5, 0.25], false);
            if laid.as_ref().is_none_or(|f| iou > f.iou) {
                laid = Some(Fit { iou, ..ink_layout(shaped, ink, &segs, p[0], p[1], p[2]) });
            }
        }
        out.extend(laid);
    }
    out
}

/// A line whose ink is taller than this (px) is fitted on a reduced copy first (`reduction`).
const SHARP_HEIGHT: u32 = 64;

/// How much a line's ink is reduced for the search: by 2 up to twice `SHARP_HEIGHT`, by 4 above. A display line's
/// render costs its area; its shape is found as well at a quarter of the size, and the full size is used only for
/// the last, sharp pass.
fn reduction(ink: &LineInk) -> u32 {
    match ink.ink_bounds.map(|(_, t, _, b)| b - t).unwrap_or(0) {
        h if h <= SHARP_HEIGHT => 1,
        h if h <= 2 * SHARP_HEIGHT => 2,
        _ => 4,
    }
}

/// The ink reduced by `f`: each pixel the mean coverage of the `f`×`f` source pixels it covers, in a frame scaled by
/// 1/`f` (source pixel X is reduced pixel X/f), so a fit on it is the source's fit divided by `f`.
fn reduced(ink: &LineInk, f: u32) -> LineInk {
    let (left, top) = (ink.left / f, ink.top / f);
    let (width, height) = ((ink.left + ink.width).div_ceil(f) - left, (ink.top + ink.height).div_ceil(f) - top);
    let mut coverage = vec![0f32; (width * height) as usize];
    let norm = 1.0 / (f * f) as f32;
    for y in 0..ink.height {
        let sy = (ink.top + y) / f - top;
        for x in 0..ink.width {
            let sx = (ink.left + x) / f - left;
            coverage[(sy * width + sx) as usize] += ink.coverage[(y * ink.width + x) as usize] * norm;
        }
    }
    let mask = coverage.iter().map(|&c| c >= 0.5).collect();
    let down = |v: u32| (ink.left + v) / f - left;
    let up = |v: u32| (ink.left + v).div_ceil(f) - left;
    let segments = ink.segments.iter().map(|&(l, r)| (down(l), up(r))).collect();
    let ink_bounds = ink.ink_bounds.map(|(l, t, r, b)| (down(l), (ink.top + t) / f - top, up(r), (ink.top + b).div_ceil(f) - top));
    LineInk { left, top, width, height, coverage, mask, backdrop: ink.backdrop, colour: ink.colour, spread: ink.spread, segments, ink_bounds, occluders: Vec::new(), foreign: Vec::new() }
}

fn enlarged(fit: Fit, f: u32) -> Fit {
    let k = f as f32;
    Fit { size: fit.size * k, baseline: fit.baseline * k, x: fit.x * k, tracking: fit.tracking * k, nudges: fit.nudges.iter().map(|n| n * k).collect(), iou: 0.0, stroke: 0.0 }
}

/// The sharp polish: descent at `steps`, then per-glyph nudges and descent again, twice.
fn polish(shaped: &Shaped, ink: &LineInk, goal: &Goal, fit: &mut Fit, steps: &[f32]) {
    descend(shaped, ink, goal, fit, steps, false);
    for _ in 0..2 {
        nudge(shaped, ink, goal, fit);
        descend(shaped, ink, goal, fit, &[0.25, 0.1], false);
    }
}

/// A line fitted in full (`fit_line`) or quickly to rank it against other fonts (`fit_line_quick`).
fn fit_with(shaped: &Shaped, ink: &LineInk, all_caps: bool, full: bool) -> Option<Fit> {
    ink.ink_bounds?;
    let scales: &[f32] = if full { &[1.0, 0.96, 1.04, 0.92, 1.08] } else { &[1.0] };
    let goal = goal_of(ink);
    let f = reduction(ink);
    let mut fits: Vec<Fit> = if f > 1 {
        // the search on the reduced ink, then each start's sharp score at the full size
        let small = reduced(ink, f);
        let small_goal = goal_of(&small);
        starts(shaped, &small, &small_goal, all_caps, scales)
            .into_iter()
            .map(|s| {
                let mut fit = enlarged(s, f);
                fit.iou = score(shaped, &fit, ink, &goal, false);
                fit
            })
            .collect()
    } else {
        starts(shaped, ink, &goal, all_caps, scales)
    };
    if f > 1 {
        // on a reduced line the better start only goes on, from a coarser step
        fits.sort_by(|a, b| b.iou.total_cmp(&a.iou));
        fits.truncate(1);
        if !full {
            // a quick fit is still brought to the full size's pixel, or a display line's ranking is the reduction's
            for fit in fits.iter_mut() {
                descend(shaped, ink, &goal, fit, &[0.5 * f as f32, 0.5], false);
            }
        }
    }
    if full {
        // both starts polished on a line at its own size
        let steps: Vec<f32> = if f > 1 { vec![0.5 * f as f32, 0.5, 0.25, 0.1] } else { vec![0.25, 0.1] };
        for fit in fits.iter_mut() {
            polish(shaped, ink, &goal, fit, &steps);
        }
    }
    let mut fit = fits.into_iter().max_by(|a, b| a.iou.total_cmp(&b.iou))?;
    fit.stroke = stroke_ratio(shaped, &fit, ink);
    Some(fit)
}

/// The initial guess from the ink bounds, then coordinate descent on (size, baseline, x, tracking), then per-glyph
/// nudges. The fit's x, baseline are in source pixels (ink.left/top added); `iou` is the final score and `stroke`
/// the drawn stroke over the ink's. None when the ink is empty.
///
/// The guess reads the size and the baseline off the string's own outlines (their union's height, and how far it
/// reaches below the baseline), so an all-caps line, a line with descenders and a font whose rounds overshoot all
/// start where the ink is; `_all_caps` is kept for the caller's contract and for a string whose outlines are empty.
/// The descent is started from a few sizes either side, since a serif's hairlines and a wide string hold a
/// descent that began a few per cent off in a smaller-tracking, larger-size optimum; and from the ink layout
/// (each glyph on its own segment) when the ink has one segment per glyph. A line taller than `SHARP_HEIGHT` is
/// searched on a reduced copy and polished at its own size.
pub fn fit_line(shaped: &Shaped, ink: &LineInk, all_caps: bool) -> Option<Fit> {
    fit_with(shaped, ink, all_caps, true)
}

/// A fit good enough to rank fonts by: one start of each layout, no polish and no nudges, at a tenth of the cost of
/// `fit_line`. Its IoU is a little under the full fit's.
pub fn fit_line_quick(shaped: &Shaped, ink: &LineInk, all_caps: bool) -> Option<Fit> {
    fit_with(shaped, ink, all_caps, false)
}

/// The rows a stroke is measured on: 40 % to 60 % of the ink's height, at least one.
fn stroke_rows(ink: &LineInk) -> Option<std::ops::Range<usize>> {
    let (_, t, _, b) = ink.ink_bounds?;
    let h = (b - t) as f32;
    let (lo, hi) = (t as usize + (h * 0.4) as usize, t as usize + (h * 0.6) as usize);
    Some(lo..hi.max(lo + 1).min(ink.height as usize))
}

/// The median horizontal stroke width across `rows` of a coverage buffer, in px: each run of pixels covered more
/// than 0.05 that is somewhere covered at least half is one stroke crossing, and its width is its summed coverage,
/// so a 1.4 px hairline reads 1.4 and not the two or three pixels its anti-aliased edge touches. Rows at mid-height
/// cross the vertical stems (and a few bars); the ink and a candidate drawn on it cross the same strokes, so the
/// two medians compare like with like.
pub fn stroke_px(coverage: &[f32], width: usize, rows: std::ops::Range<usize>) -> Option<f32> {
    let mut widths = Vec::new();
    for row in rows.filter_map(|y| coverage.get(y * width..(y + 1) * width)) {
        let (mut sum, mut peak) = (0f32, 0f32);
        for &c in row.iter().chain(std::iter::once(&0.0)) {
            if c > 0.05 {
                sum += c;
                peak = peak.max(c);
            } else if sum > 0.0 {
                if peak >= 0.5 {
                    widths.push(sum);
                }
                (sum, peak) = (0.0, 0.0);
            }
        }
    }
    if widths.is_empty() {
        return None;
    }
    widths.sort_by(f32::total_cmp);
    Some(widths[widths.len() / 2])
}

/// The ink's stroke width in px (`stroke_px` over its clean coverage).
pub fn ink_stroke(ink: &LineInk) -> Option<f32> {
    stroke_px(&target_of(ink), ink.width as usize, stroke_rows(ink)?)
}

/// The stroke of the string drawn at `fit` over the ink's: above 1 the candidate is heavier than the source, below
/// lighter; 0 when either has no stroke to measure. IoU on a hairline source is edge-dominated (a stroke a pixel
/// off scores far below what the eye sees, and a 500 can outscore the 300 the source is), so the acceptance and
/// the choice between near-equal candidates read the weight from this.
pub fn stroke_ratio(shaped: &Shaped, fit: &Fit, ink: &LineInk) -> f32 {
    let Some(rows) = stroke_rows(ink) else { return 0.0 };
    let (Some(want), Some(drawn)) = (ink_stroke(ink), stroke_px(&render_coverage(shaped, fit, ink), ink.width as usize, rows)) else { return 0.0 };
    if want > 0.0 { drawn / want } else { 0.0 }
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
        let ink = LineInk { left: 0, top: 0, width: 3, height: 1, coverage: vec![0.0, 0.79, 1.0], mask: vec![false, true, true], backdrop: [255; 3], colour: [0; 3], spread: 0.0, segments: Vec::new(), ink_bounds: None, occluders: Vec::new(), foreign: Vec::new() };
        let c = encoded_coverage(&ink);
        // black on white: linear light 0.79 of the way to black is the encoded mid-grey, 0.5 covered
        assert!((c[1] - 0.5).abs() < 0.02, "{}", c[1]);
        assert_eq!((c[0], c[2]), (0.0, 1.0));
    }

    #[test]
    fn blue_ink_on_yellow_keeps_its_anti_aliased_edges() {
        // three bars of blue on yellow, each edge column the half-and-half blend of the encoded values (how images
        // anti-alias): that pixel is a quarter of the line's length off the linear-light line between the two
        // colours, and is still ink, half covered
        let (blue, yellow) = ([30u8, 60, 200], [250u8, 230, 60]);
        let half: [u8; 3] = std::array::from_fn(|c| ((blue[c] as u16 + yellow[c] as u16) / 2) as u8);
        let (w, h) = (120u32, 60u32);
        let mut rgba = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let bar = [20u32, 40, 60].iter().find(|&&l| (l..l + 6).contains(&x) && (20..44).contains(&y)).copied();
                let c = match bar {
                    Some(l) if x == l || x == l + 5 => half,
                    Some(_) => blue,
                    None => yellow,
                };
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        let src = crate::redraw::Source { rgba, width: w, height: h };
        let ink = super::super::ink::line_ink(&src, [[15.0, 18.0], [75.0, 18.0], [75.0, 46.0], [15.0, 46.0]]);
        assert_eq!(ink.colour, blue);
        assert_eq!(ink.segments.len(), 3, "{:?}", ink.segments);
        let at = |x: u32, y: u32| ((y - ink.top) * ink.width + (x - ink.left)) as usize;
        assert!(ink.coverage[at(20, 30)] > 0.1, "the edge pixel keeps its coverage: {}", ink.coverage[at(20, 30)]);
        let encoded = encoded_coverage(&ink);
        assert!((encoded[at(20, 30)] - 0.5).abs() < 0.1, "and reads half covered to the fit: {}", encoded[at(20, 30)]);
        assert!((encoded[at(22, 30)] - 1.0).abs() < 0.02 && encoded[at(30, 30)] < 0.02);
    }

    #[test]
    fn path_data_is_the_outline_in_source_pixels_y_down() {
        use super::super::glyphs::ShapedGlyph;
        let mut pb = tiny_skia::PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_ltrb(0.0, 0.0, 0.5, 0.7).unwrap());
        let shaped = Shaped { glyphs: vec![ShapedGlyph { ch: 'I', path: pb.finish(), advance: 0.6, x_offset: 0.0 }], cap_height: 0.7, x_height: 0.5 };
        let fit = Fit { size: 10.0, baseline: 20.0, x: 5.0, tracking: 0.0, nudges: vec![0.0], iou: 1.0, stroke: 0.0 };
        assert_eq!(path_data(&shaped, &fit), "M5 20L10 20L10 13L5 13Z");
    }

    /// `string` in the fixture Montserrat drawn black on white at `truth` (nudges included), with the quad Vision
    /// would give, through the real ink reader.
    pub(super) fn drawn(string: &str, truth: &Fit, w: u32, h: u32) -> (super::super::glyphs::Font, LineInk) {
        drawn_in("Montserrat-Regular.ttf", string, truth, w, h)
    }

    /// The same in one of the fixture fonts.
    pub(super) fn drawn_in(file: &str, string: &str, truth: &Fit, w: u32, h: u32) -> (super::super::glyphs::Font, LineInk) {
        use super::super::glyphs::Font;
        let font = Font::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text/fonts").join(file), 0).unwrap();
        let shaped = font.shape(string);
        let page = LineInk { left: 0, top: 0, width: w, height: h, coverage: Vec::new(), mask: Vec::new(), backdrop: [255; 3], colour: [0; 3], spread: 0.0, segments: Vec::new(), ink_bounds: None, occluders: Vec::new(), foreign: Vec::new() };
        let cov = render_coverage(&shaped, truth, &page);
        let rgba: Vec<u8> = cov.iter().flat_map(|c| { let v = (255.0 * (1.0 - c)).round() as u8; [v, v, v, 255] }).collect();
        let src = crate::redraw::Source { rgba, width: w, height: h };
        let top = truth.baseline - truth.size * shaped.cap_height - 4.0;
        let quad = [[4.0, top], [w as f32 - 4.0, top], [w as f32 - 4.0, truth.baseline + 4.0], [4.0, truth.baseline + 4.0]];
        (font, super::super::ink::line_ink(&src, quad))
    }

    #[test]
    fn a_line_with_uneven_letter_gaps_and_a_wide_word_gap_is_laid_out_from_its_ink() {
        // tracked 6 px, the word gap 14 px wider than the tracking makes it, and letters a few px off either way:
        // what an AI-made tagline looks like, and what one tracking value with ±2 px nudges cannot follow
        let string = "LIFE IS GOOD";
        let mut nudges = vec![0.0, 3.0, -2.5, 3.5, 0.0, 14.0, 17.5, 0.0, 28.0, 25.0, 31.5, 28.0];
        nudges[4] = 0.0;
        let truth = Fit { size: 40.0, baseline: 60.0, x: 10.0, tracking: 6.0, nudges, iou: 1.0, stroke: 0.0 };
        let (font, ink) = drawn(string, &truth, 560, 80);
        let shaped = font.shape(string);
        assert_eq!(ink.segments.len(), 10);
        let fit = fit_line(&shaped, &ink, true).unwrap();
        assert!(fit.iou > 0.9, "iou {}", fit.iou);
        assert!((fit.size - 40.0).abs() < 0.8, "size {}", fit.size);
        for (i, (a, b)) in origins(&shaped, &fit).iter().zip(origins(&shaped, &truth)).enumerate() {
            if shaped.glyphs[i].path.is_some() {
                assert!((a - b).abs() < 0.75, "glyph {i} at {a}, drawn at {b}");
            }
        }
    }

    #[test]
    fn a_stroke_is_its_summed_coverage_across_the_row() {
        // two strokes per row: a 1.4 px hairline (0.7, 0.7) and a 3 px stem (0.5, 1, 1, 0.5); faint noise is no stroke
        let row = [0.0, 0.7, 0.7, 0.0, 0.0, 0.5, 1.0, 1.0, 0.5, 0.0, 0.04, 0.3, 0.0];
        let buf: Vec<f32> = row.iter().chain(row.iter()).chain(row.iter()).copied().collect();
        let w = stroke_px(&buf, row.len(), 0..3).unwrap();
        assert!((w - 3.0).abs() < 1e-5, "{w}");
        assert_eq!(stroke_px(&buf[..6], 6, 0..1), Some(1.4));
        assert_eq!(stroke_px(&[0.0, 0.3, 0.0], 3, 0..1), None);
    }

    #[test]
    fn a_heavy_candidate_on_a_hairline_reads_heavier_than_the_ink_and_the_true_one_reads_even() {
        // drawn in Josefin Sans Light at 40 px: fitted with its own font the stroke is the ink's, with Montserrat
        // Regular (a 400 with twice the stem) it is far heavier, though the IoU alone would not say how much
        let truth = Fit { size: 40.0, baseline: 60.0, x: 10.0, tracking: 6.0, nudges: vec![0.0; 7], iou: 1.0, stroke: 0.0 };
        let (light, ink) = drawn_in("JosefinSans-Light.ttf", "HILLTOP", &truth, 400, 80);
        let own = fit_line(&light.shape("HILLTOP"), &ink, true).unwrap();
        assert!((own.stroke - 1.0).abs() < 0.12, "the true font: {}", own.stroke);
        let hair = ink_stroke(&ink).unwrap();
        assert!((1.5..3.5).contains(&hair), "Josefin Sans Light's stem at 40 px: {hair}");
        let (regular, _) = drawn("HILLTOP", &truth, 400, 80);
        let heavy = fit_line(&regular.shape("HILLTOP"), &ink, true).unwrap();
        assert!(heavy.stroke > 1.35, "Montserrat Regular on Josefin Light: {}", heavy.stroke);
    }

    #[test]
    fn a_nudge_scored_on_its_glyph_s_window_scores_what_the_whole_line_does() {
        let truth = Fit { size: 30.0, baseline: 50.0, x: 8.0, tracking: 5.0, nudges: vec![0.0; 7], iou: 1.0, stroke: 0.0 };
        let (font, ink) = drawn("HILLTOP", &truth, 360, 70);
        let shaped = font.shape("HILLTOP");
        let goal = goal_of(&ink);
        let (inter, union) = window_sums(&shaped, &truth, &ink, &goal, 0, goal.width);
        assert!((inter / union - soft_iou(&render_coverage(&shaped, &truth, &ink), &goal.sharp)).abs() < 1e-4);
        // glyph 3 moved 1.5 px: its window's sums, swapped into the line's, give the line's own score
        let mut moved = truth.clone();
        moved.nudges[3] = 1.5;
        let ox = origins(&shaped, &truth)[3] - ink.left as f32;
        let b = shaped.glyphs[3].path.as_ref().unwrap().bounds();
        let (x0, x1) = ((ox + b.left() * 30.0 - 4.0).floor() as usize, (ox + b.right() * 30.0 + 4.0).ceil() as usize);
        let (bi, bu) = window_sums(&shaped, &truth, &ink, &goal, x0, x1);
        let (ti, tu) = window_sums(&shaped, &moved, &ink, &goal, x0, x1);
        let windowed = (inter - bi + ti) / (union - bu + tu);
        assert!((windowed - score(&shaped, &moved, &ink, &goal, false)).abs() < 1e-4, "{windowed}");
    }

    #[test]
    fn a_display_line_is_found_on_a_reduced_copy_and_lands_on_its_ink_at_full_size() {
        // 140 px caps, searched at a quarter of the size: the result is the truth at the full size
        let truth = Fit { size: 200.0, baseline: 190.0, x: 20.0, tracking: 30.0, nudges: vec![0.0; 7], iou: 1.0, stroke: 0.0 };
        let (font, ink) = drawn("HORIZON", &truth, 1300, 240);
        assert_eq!(reduction(&ink), 4);
        let shaped = font.shape("HORIZON");
        let fit = fit_line(&shaped, &ink, true).unwrap();
        assert!(fit.iou > 0.95, "iou {}", fit.iou);
        assert!((fit.size - 200.0).abs() < 2.0, "size {}", fit.size);
        assert!((fit.baseline - 190.0).abs() < 0.5, "baseline {}", fit.baseline);
        for (i, (a, b)) in origins(&shaped, &fit).iter().zip(origins(&shaped, &truth)).enumerate() {
            assert!((a - b).abs() < 1.0, "glyph {i} at {a}, drawn at {b}");
        }
        // the quick fit ranks it nearly as well
        let quick = fit_line_quick(&shaped, &ink, true).unwrap();
        assert!(quick.iou > fit.iou - 0.05 && quick.iou <= fit.iou + 1e-3, "{} vs {}", quick.iou, fit.iou);
    }

    #[test]
    fn the_reduced_ink_keeps_the_frame_scaled_and_the_coverage_averaged() {
        let mut coverage = vec![0f32; 8 * 4];
        for y in 0..4 {
            for x in 2..6 {
                coverage[y * 8 + x] = 1.0;
            }
        }
        let ink = LineInk { left: 10, top: 6, width: 8, height: 4, coverage, mask: Vec::new(), backdrop: [255; 3], colour: [0; 3], spread: 0.0, segments: vec![(2, 6)], ink_bounds: Some((2, 0, 6, 4)), occluders: Vec::new(), foreign: Vec::new() };
        let small = reduced(&ink, 2);
        assert_eq!((small.left, small.top, small.width, small.height), (5, 3, 4, 2));
        // source columns 12..16 are reduced columns 6..8, the box's 1..3
        assert_eq!(small.coverage, vec![0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0]);
        assert_eq!(small.segments, vec![(1, 3)]);
        assert_eq!(small.ink_bounds, Some((1, 0, 3, 2)));
    }

    #[test]
    fn numbers_are_written_to_two_decimals() {
        assert_eq!(fmt(12.345), "12.35");
        assert_eq!(fmt(3.0), "3");
        assert_eq!(fmt(-0.001), "0");
    }
}
