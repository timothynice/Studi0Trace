//! What a line of text is made of, read from the pixels: its backdrop (the box's border ring), its ink colour
//! (the pixels furthest from the backdrop), per-pixel coverage (where a pixel sits between the two in linear RGB),
//! the mask, the per-character segments, and the reasons a line is left to the trace. `paint_out` is the
//! inverse: the ink gone, the backdrop continued through it.
use super::{Class, Reason, INK_SPREAD_MAX, MISMATCH_SHARE, ROTATED_DEG};
use crate::redraw::Source;

/// How far a pixel may be from each of its eight neighbours and still be on a flat patch of ink.
const CORE_FLAT: f32 = 6.0;

/// How far off the backdrop-to-ink line (in linear RGB, over the line's length) a pixel may be and still be a blend
/// of the two: anti-aliased ink is on it to a few hundredths; a gold swash beside dark ink is 0.2 off it.
const OFF_LINE: f32 = 0.1;

/// The coverage at which a pixel joins its neighbours into a segment (the mask itself is coverage >= 0.5; the
/// fixture lines keep their segment counts for any value from 0.25 to 0.37; at 0.40 Playfair 12 px splits).
const SEGMENT_COVERAGE: f32 = 0.33;

pub struct LineInk {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<f32>,
    pub mask: Vec<bool>,
    pub backdrop: [u8; 3],
    pub colour: [u8; 3],
    pub spread: f32,
    pub segments: Vec<(u32, u32)>,
    pub ink_bounds: Option<(u32, u32, u32, u32)>,
    /// The colours of what lay over the letters: the pixels inside the characters' boxes that the off-colour rule
    /// kept out of the ink, in 8-connected blobs of at least `OCCLUDER_MIN_PX` that lie over the letters
    /// (`lies_over`), each blob's median colour, those within `OCCLUDER_MERGE` ΔE of one already listed merged into
    /// it. Usually empty.
    pub occluders: Vec<[u8; 3]>,
    /// Per pixel of the box, as `mask`: off the backdrop-to-ink line (another element's colour, never ink). Empty
    /// when unknown (a reduced or hand-made ink): then no pixel is foreign.
    pub foreign: Vec<bool>,
}

/// A blob of foreign pixels inside the characters' boxes under this many is noise (an anti-aliased fringe), not
/// something that lay over the letters.
pub const OCCLUDER_MIN_PX: usize = 12;
/// Occluder colours this close (ΔE) are one.
const OCCLUDER_MERGE: f32 = 6.0;
/// How far past a blob's edge (px) ink must lie for the blob to have letter on that side of it.
const CROSS_REACH: u32 = 3;
/// The share of a blob's pixels that must have ink on both sides of the blob along one axis for it to cross the
/// letters (and so be something laid over them) rather than sit beside them.
const CROSS_SHARE: f32 = 0.3;
/// A blob that does not cross the letters but has ink past it on one side at this share of its pixels or more is a
/// band hugging them (a drop shadow, an outline): never an occluder.
const HUG_SHARE: f32 = 0.8;
/// A blob with ink past it at under this share of its pixels does not touch the letters at all.
const TOUCH_SHARE: f32 = 0.1;

fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn lab(c: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])];
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

pub fn delta_e(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (p, q) = (lab(a), lab(b));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

fn median(v: &mut [u8]) -> u8 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

fn median_colour(pixels: &[[u8; 3]]) -> [u8; 3] {
    let mut r: Vec<u8> = pixels.iter().map(|p| p[0]).collect();
    let mut g: Vec<u8> = pixels.iter().map(|p| p[1]).collect();
    let mut b: Vec<u8> = pixels.iter().map(|p| p[2]).collect();
    [median(&mut r), median(&mut g), median(&mut b)]
}

fn quad_bbox(quad: [[f32; 2]; 4]) -> (f32, f32, f32, f32) {
    let xs = quad.iter().map(|p| p[0]);
    let ys = quad.iter().map(|p| p[1]);
    (xs.clone().fold(f32::MAX, f32::min), ys.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max), ys.fold(f32::MIN, f32::max))
}

pub fn quad_angle_deg(quad: [[f32; 2]; 4]) -> f32 {
    let (dx, dy) = (quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]);
    dy.atan2(dx).to_degrees()
}

pub fn line_ink(source: &Source, quad: [[f32; 2]; 4]) -> LineInk {
    let empty = LineInk { left: 0, top: 0, width: 0, height: 0, coverage: Vec::new(), mask: Vec::new(), backdrop: [0, 0, 0], colour: [0, 0, 0], spread: 0.0, segments: Vec::new(), ink_bounds: None, occluders: Vec::new(), foreign: Vec::new() };
    if source.width == 0 || source.height == 0 || source.rgba.len() != source.width as usize * source.height as usize * 4 {
        return empty;
    }
    let (x0, y0, x1, y1) = quad_bbox(quad);
    let grow = (y1 - y0) * 0.2;
    // the box, clipped to the image; a quad wholly outside it has no ink to read
    let (fl, ft) = ((x0 - grow).floor().max(0.0), (y0 - grow).floor().max(0.0));
    let (fr, fb) = ((x1 + grow).ceil().min(source.width as f32), (y1 + grow).ceil().min(source.height as f32));
    if !(fl < fr && ft < fb) {
        return empty;
    }
    let (left, top, right, bottom) = (fl as u32, ft as u32, fr as u32, fb as u32);
    let (width, height) = (right - left, bottom - top);
    let px = |x: u32, y: u32| -> [u8; 3] {
        let i = ((y * source.width + x) * 4) as usize;
        [source.rgba[i], source.rgba[i + 1], source.rgba[i + 2]]
    };
    // the backdrop: the 2 px ring
    let mut ring = Vec::new();
    for y in top..top + height {
        for x in left..left + width {
            let edge = x < left + 2 || x >= left + width.saturating_sub(2) || y < top + 2 || y >= top + height.saturating_sub(2);
            if edge {
                ring.push(px(x, y));
            }
        }
    }
    let backdrop = median_colour(&ring);
    // the ink: the pixels at least 90 % as far from the backdrop as the furthest
    let mut dist = vec![0f32; (width * height) as usize];
    let mut far = 0f32;
    for y in 0..height {
        for x in 0..width {
            let d = delta_e(px(left + x, top + y), backdrop);
            dist[(y * width + x) as usize] = d;
            far = far.max(d);
        }
    }
    let inkers: Vec<[u8; 3]> = (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).filter(|&(x, y)| far > 0.0 && dist[(y * width + x) as usize] >= 0.9 * far).map(|(x, y)| px(left + x, top + y)).collect();
    let colour = if inkers.is_empty() { backdrop } else { median_colour(&inkers) };
    // coverage: position between backdrop and ink along their line, in linear RGB
    let b = [srgb_to_linear(backdrop[0]), srgb_to_linear(backdrop[1]), srgb_to_linear(backdrop[2])];
    let k = [srgb_to_linear(colour[0]), srgb_to_linear(colour[1]), srgb_to_linear(colour[2])];
    let axis = [k[0] - b[0], k[1] - b[1], k[2] - b[2]];
    let len2 = axis.iter().map(|a| a * a).sum::<f32>().max(1e-6);
    // and how far off that line the pixel is: a blend of the ink and the backdrop sits on it, a pixel of another
    // colour (a neighbouring element's, reaching into the grown box) does not
    // colour (a neighbouring element's, reaching into the grown box) does not. Off it in linear light and in the
    // encoded values both: an image blends its edges on the encoded values, and a half-covered pixel of blue ink
    // on yellow sits a quarter of the line's length off the linear line while it is on the encoded one
    let (be, ke) = (backdrop.map(|v| v as f32 / 255.0), colour.map(|v| v as f32 / 255.0));
    let axis_e = [ke[0] - be[0], ke[1] - be[1], ke[2] - be[2]];
    let len2_e = axis_e.iter().map(|a| a * a).sum::<f32>().max(1e-6);
    let off_line = |l: [f32; 3], axis: [f32; 3], len2: f32| -> bool {
        let t = (l[0] * axis[0] + l[1] * axis[1] + l[2] * axis[2]) / len2;
        (0..3).map(|c| (l[c] - t * axis[c]).powi(2)).sum::<f32>() > OFF_LINE * OFF_LINE * len2
    };
    let mut coverage = vec![0f32; (width * height) as usize];
    let mut foreign = vec![false; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let p = px(left + x, top + y);
            let l = [srgb_to_linear(p[0]) - b[0], srgb_to_linear(p[1]) - b[1], srgb_to_linear(p[2]) - b[2]];
            let t = (l[0] * axis[0] + l[1] * axis[1] + l[2] * axis[2]) / len2;
            coverage[(y * width + x) as usize] = t.clamp(0.0, 1.0);
            let le = [p[0] as f32 / 255.0 - be[0], p[1] as f32 / 255.0 - be[1], p[2] as f32 / 255.0 - be[2]];
            foreign[(y * width + x) as usize] = off_line(l, axis, len2) && off_line(le, axis_e, len2_e);
        }
    }
    let mask: Vec<bool> = coverage.iter().map(|&c| c >= 0.5).collect();
    // the spread: the range of colour (`colour_range`) over the ink's flat patches, the mask pixels whose eight
    // neighbours are all in the mask and within CORE_FLAT of the pixel itself. An edge pixel is a blend of ink and
    // backdrop by construction, and on a stroke of a pixel or three there is nowhere a pixel and its neighbours
    // agree: a light ink and a thin one look the same there, so with fewer than 8 flat pixels there is no evidence
    // of a gradient and the spread is 0. (Not coverage >= some level, which is measured against the one ink colour
    // and so can never see a second, lighter ink such as the 90 and 160 grey bars of a gradient; and not the mask's
    // interior or its coverage ridge, which on 1-2 px strokes are still blends, 20-45 ΔE from the ink.) A range of
    // the flat pixels themselves, not their distance from the ink colour: that is the furthest pixels' colour, and
    // on large display letters with grain it is the darkest specks, 7-11 ΔE from a body that is one colour.
    //
    // Only the characters' own pixels count: the box is grown round the quad, and a rule, a divider or a
    // neighbouring element of another colour inside it is neither ink nor gradient (`character_pixels`).
    let linked: Vec<bool> = coverage.iter().map(|&c| c >= SEGMENT_COVERAGE).collect();
    let quad_box = ((x0 - left as f32).max(0.0), (y0 - top as f32).max(0.0), x1 - left as f32, y1 - top as f32);
    let characters = character_pixels(&linked, width, height, quad_box);
    let at = |x: u32, y: u32| px(left + x, top + y);
    let mut flats: Vec<[u8; 3]> = Vec::new();
    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            if !mask[(y * width + x) as usize] || !characters[(y * width + x) as usize] {
                continue;
            }
            let here = at(x, y);
            let flat = (y - 1..=y + 1).all(|ny| (x - 1..=x + 1).all(|nx| mask[(ny * width + nx) as usize] && delta_e(at(nx, ny), here) <= CORE_FLAT));
            if flat {
                flats.push(here);
            }
        }
    }
    let spread = colour_range(&flats);
    // pixels join into a segment at a lower coverage than the mask: a hairline serif or a thin join under 0.5 must
    // not cut a glyph in two (a fragment would read as an extra character); the segment's extent is still the mask's
    // what the spread has read, the line's ink is only what is on the ink's line: a pixel of another colour is no
    // coverage, no mask and no segment, so the fit does not chase it and the paint-out leaves it where it is (a
    // wordmark's box reaching the swash of a script under it had cut the swash)
    for (i, f) in foreign.iter().enumerate() {
        if *f {
            coverage[i] = 0.0;
        }
    }
    let mask: Vec<bool> = coverage.iter().map(|&c| c >= 0.5).collect();
    let linked: Vec<bool> = coverage.iter().map(|&c| c >= SEGMENT_COVERAGE).collect();
    let segments = segments_of(&mask, &linked, width, height);
    let ink_bounds = bounds_of(&mask, width, height);
    let occluders = occluders_of(&foreign, &mask, &segments, ink_bounds, width, height, |x, y| px(left + x, top + y));
    LineInk { left, top, width, height, coverage, mask, backdrop, colour, spread, segments, ink_bounds, occluders, foreign }
}

/// The colours that lay over the letters: `foreign` pixels (off the backdrop-to-ink line) inside a character's box,
/// each segment's x range by the ink's y range, grown 1 px, grouped into 8-connected blobs. A blob of at least
/// `OCCLUDER_MIN_PX` that lies over the letters (`lies_over`) gives its median colour; a colour within
/// `OCCLUDER_MERGE` ΔE of one already listed is dropped (the first is kept). In blob order.
fn occluders_of(foreign: &[bool], mask: &[bool], segments: &[(u32, u32)], ink_bounds: Option<(u32, u32, u32, u32)>, width: u32, height: u32, px: impl Fn(u32, u32) -> [u8; 3]) -> Vec<[u8; 3]> {
    let Some((_, top, _, bottom)) = ink_bounds else { return Vec::new() };
    let (y0, y1) = (top.saturating_sub(1), (bottom + 1).min(height));
    let inside = |x: u32, y: u32| y >= y0 && y < y1 && segments.iter().any(|&(lo, hi)| x + 1 >= lo && x < hi + 1);
    let in_box: Vec<bool> = (0..foreign.len()).map(|i| foreign[i] && inside((i as u32) % width, (i as u32) / width)).collect();
    let mut out: Vec<[u8; 3]> = Vec::new();
    for blob in blobs(&in_box, width, height) {
        if blob.len() < OCCLUDER_MIN_PX || !lies_over(&blob, &in_box, mask, width, height) {
            continue;
        }
        let colours: Vec<[u8; 3]> = blob.iter().map(|&i| px((i as u32) % width, (i as u32) / width)).collect();
        let c = median_colour(&colours);
        if !out.iter().any(|&o| delta_e(o, c) < OCCLUDER_MERGE) {
            out.push(c);
        }
    }
    out
}

/// Whether a blob lies over the letters. A pixel has ink past it in a direction when there is ink within
/// `CROSS_REACH` px past the blob's edge that way (the walk goes through the blob's own pixels first, `member`, so a
/// thick bar's middle rows count as its edge rows do). The blob crosses the letters when at least `CROSS_SHARE` of
/// its pixels have ink past them on both sides along one axis (above and below, or left and right): a bar or a
/// script stroke laid across a stem. Otherwise it is a band hugging the letters when `HUG_SHARE` of its pixels or
/// more have ink past them on one side (a drop shadow has ink on one side of it only, an outline only on its inside),
/// and not over them at all under `TOUCH_SHARE`; between the two it lies over the letters' edge and reaches away from
/// them (the script "Awaits" over the feet of ADVENTURE, which crosses no stem), and is laid over them.
fn lies_over(blob: &[usize], member: &[bool], mask: &[bool], width: u32, height: u32) -> bool {
    let ink_beyond = |i: usize, dx: i64, dy: i64| -> bool {
        let (mut x, mut y) = (((i as u32) % width) as i64, ((i as u32) / width) as i64);
        let (mut out, mut steps) = (false, 0);
        loop {
            x += dx;
            y += dy;
            if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                return false;
            }
            let j = (y as u32 * width + x as u32) as usize;
            if mask[j] {
                return true;
            }
            if !out && member[j] {
                continue;
            }
            out = true;
            steps += 1;
            if steps >= CROSS_REACH {
                return false;
            }
        }
    };
    let (mut across, mut beside) = (0usize, 0usize);
    for &i in blob {
        let [up, down, left, right] = [ink_beyond(i, 0, -1), ink_beyond(i, 0, 1), ink_beyond(i, -1, 0), ink_beyond(i, 1, 0)];
        across += ((up && down) || (left && right)) as usize;
        beside += (up || down || left || right) as usize;
    }
    let n = blob.len() as f32;
    across as f32 >= CROSS_SHARE * n || ((beside as f32) < HUG_SHARE * n && beside as f32 >= TOUCH_SHARE * n)
}

/// The 8-connected components of `set`, each as its pixel indices, in raster order of their first pixel.
fn blobs(set: &[bool], width: u32, height: u32) -> Vec<Vec<usize>> {
    let mut seen = vec![false; set.len()];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..set.len() {
        if !set[start] || seen[start] {
            continue;
        }
        let mut blob = Vec::new();
        stack.push(start);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            blob.push(i);
            let (x, y) = ((i as u32) % width, (i as u32) / width);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }
                    let j = (ny as u32 * width + nx as u32) as usize;
                    if set[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        out.push(blob);
    }
    out
}

/// The spread of a set of colours as a range, in ΔE: the 90th minus the 10th percentile of their positions along
/// their principal axis in Lab. A ramp of span D reads 0.8 D whichever end the ink colour was taken from, grain
/// reads about 2.6 times its deviation, and two or three separate inks read the distance between the outer two.
/// Fewer than 8 colours are no evidence of anything: 0.
fn colour_range(colours: &[[u8; 3]]) -> f32 {
    if colours.len() < 8 {
        return 0.0;
    }
    let labs: Vec<[f32; 3]> = colours.iter().map(|c| lab(*c)).collect();
    let n = labs.len() as f32;
    let mean: [f32; 3] = std::array::from_fn(|c| labs.iter().map(|l| l[c]).sum::<f32>() / n);
    let mut cov = [[0f32; 3]; 3];
    for l in &labs {
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += (l[i] - mean[i]) * (l[j] - mean[j]) / n;
            }
        }
    }
    // the principal axis by power iteration, from the axis of largest variance
    let start = (0..3).max_by(|&a, &b| cov[a][a].total_cmp(&cov[b][b])).unwrap_or(0);
    let mut v = [0.1f32; 3];
    v[start] = 1.0;
    for _ in 0..32 {
        let w: [f32; 3] = std::array::from_fn(|i| (0..3).map(|j| cov[i][j] * v[j]).sum());
        let norm = w.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm <= 1e-12 {
            return 0.0;
        }
        v = w.map(|x| x / norm);
    }
    let mut along: Vec<f32> = labs.iter().map(|l| (0..3).map(|c| (l[c] - mean[c]) * v[c]).sum()).collect();
    along.sort_by(f32::total_cmp);
    along[along.len() * 9 / 10] - along[along.len() / 10]
}

/// A character's height, against the line's, that a component may have and count as one for the spread.
const CHARACTER_HEIGHT: (f32, f32) = (0.4, 1.3);

/// The pixels of the components (8-connected, of `linked`) that are the line's characters: those overlapping the
/// quad as Vision gave it (`quad_box`, in the box's frame, before it was grown) whose height is within
/// `CHARACTER_HEIGHT` of the line's, the median height of the overlapping components at least 0.3 of the quad's
/// (punctuation left out of it). A rule under the line or a divider beside it (another colour, inside the grown
/// box) is too short or too tall; a neighbouring line's letters lie outside the quad. With no such component
/// every pixel counts, as before.
fn character_pixels(linked: &[bool], width: u32, height: u32, quad_box: (f32, f32, f32, f32)) -> Vec<bool> {
    let (w, h) = (width as usize, height as usize);
    let mut label = vec![u32::MAX; linked.len()];
    let mut boxes: Vec<(usize, usize, usize, usize)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..linked.len() {
        if !linked[start] || label[start] != u32::MAX {
            continue;
        }
        let id = boxes.len() as u32;
        let mut b = (usize::MAX, usize::MAX, 0, 0);
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            b = (b.0.min(x), b.1.min(y), b.2.max(x + 1), b.3.max(y + 1));
            for ny in y.saturating_sub(1)..(y + 2).min(h) {
                for nx in x.saturating_sub(1)..(x + 2).min(w) {
                    let j = ny * w + nx;
                    if linked[j] && label[j] == u32::MAX {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        boxes.push(b);
    }
    let (ql, qt, qr, qb) = quad_box;
    let overlaps = |b: &(usize, usize, usize, usize)| (b.0 as f32) < qr && (b.2 as f32) > ql && (b.1 as f32) < qb && (b.3 as f32) > qt;
    let mut heights: Vec<f32> = boxes.iter().filter(|b| overlaps(b)).map(|b| (b.3 - b.1) as f32).filter(|&hh| hh >= 0.3 * (qb - qt)).collect();
    if heights.is_empty() {
        return vec![true; linked.len()];
    }
    heights.sort_by(f32::total_cmp);
    let line = heights[heights.len() / 2];
    let keep: Vec<bool> = boxes.iter().map(|b| overlaps(b) && ((b.3 - b.1) as f32 / line) >= CHARACTER_HEIGHT.0 && ((b.3 - b.1) as f32 / line) <= CHARACTER_HEIGHT.1).collect();
    label.iter().map(|&id| id != u32::MAX && keep[id as usize]).collect()
}

fn bounds_of(mask: &[bool], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut l, mut t, mut r, mut b) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if mask[(y * width + x) as usize] {
                l = l.min(x);
                t = t.min(y);
                r = r.max(x + 1);
                b = b.max(y + 1);
            }
        }
    }
    (l < r).then_some((l, t, r, b))
}

/// Connected components (8-connected) of `linked` as the x ranges of their `mask` pixels, merged where the ranges
/// overlap. A component with no mask pixel is not a segment.
fn segments_of(mask: &[bool], linked: &[bool], width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for blob in blobs(linked, width, height) {
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        for i in blob.into_iter().filter(|&i| mask[i]) {
            let x = (i as u32) % width;
            lo = lo.min(x);
            hi = hi.max(x + 1);
        }
        if lo < hi {
            ranges.push((lo, hi));
        }
    }
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 < last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    merged
}

/// A gap between two segments this many times the line's median gap is a word space...
pub const SPACE_GAP: f32 = 1.8;
/// ...when it also exceeds the median by this share of the ink's height: a word space adds a space's advance
/// (about a third of the cap) to a letter gap, while a narrow glyph in a wide cell (a tabular 1) leaves gaps of
/// a quarter em either side of it, and a letter gap in untracked text is a few pixels, 1.8 times of which is
/// nothing.
const SPACE_EXCESS: f32 = 0.35;

/// The string with the word spaces the ink shows and the reader dropped put back. Vision drops the space in
/// widely tracked text ("ABRIGHTER", "LIFEIS"), and a fit with no space advance where the ink has a word gap
/// pulls every glyph after it out of place. Only when the ink's segments number the string's non-space
/// characters (so each gap is between two known characters): a space goes wherever the gap exceeds `SPACE_GAP`
/// times the median gap and the median plus `SPACE_EXCESS` of the ink's height, the string has none there
/// already and the two characters are not both digits (a number is never split). Nothing is ever removed.
pub fn repair_spaces(ink: &LineInk, string: &str) -> String {
    let chars: Vec<char> = string.chars().collect();
    let solid: Vec<usize> = (0..chars.len()).filter(|&i| !chars[i].is_whitespace()).collect();
    let Some((_, t, _, b)) = ink.ink_bounds else { return string.to_string() };
    if solid.len() != ink.segments.len() || solid.len() < 3 {
        return string.to_string();
    }
    let gaps: Vec<f32> = ink.segments.windows(2).map(|w| w[1].0 as f32 - w[0].1 as f32).collect();
    let mut sorted = gaps.clone();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let wide = (median * SPACE_GAP).max(median + (b - t) as f32 * SPACE_EXCESS);
    let mut out = String::with_capacity(string.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        // a gap after this solid character, before the next one, with nothing between them already
        if let Some(k) = solid.iter().position(|&s| s == i) {
            if k + 1 < solid.len() && solid[k + 1] == i + 1 && gaps[k] > wide && !(c.is_ascii_digit() && chars[i + 1].is_ascii_digit()) {
                out.push(' ');
            }
        }
    }
    out
}

pub fn decline(ink: &LineInk, string: &str, class: Class, quad: [[f32; 2]; 4]) -> Option<Reason> {
    match class {
        Class::Script => return Some(Reason::Script),
        Class::Custom => return Some(Reason::Custom),
        Class::Type => {}
    }
    if quad_angle_deg(quad).abs() > ROTATED_DEG {
        return Some(Reason::Rotated);
    }
    if ink.spread > INK_SPREAD_MAX {
        return Some(Reason::Gradient);
    }
    let chars = string.chars().filter(|c| !c.is_whitespace()).count().max(1) as f32;
    let diff = (ink.segments.len() as f32 - chars).abs() / chars;
    if diff > MISMATCH_SHARE {
        return Some(Reason::Mismatch);
    }
    None
}

pub fn paint_out(source: &Source, inks: &[&LineInk]) -> Vec<u8> {
    let (w, h) = (source.width as usize, source.height as usize);
    let mut masked = vec![false; w * h];
    for ink in inks {
        for y in 0..ink.height as usize {
            for x in 0..ink.width as usize {
                if ink.mask[y * ink.width as usize + x] {
                    // dilated by 1 px
                    for dy in -1i64..=1 {
                        for dx in -1i64..=1 {
                            let (sx, sy) = (ink.left as i64 + x as i64 + dx, ink.top as i64 + y as i64 + dy);
                            if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                                masked[sy as usize * w + sx as usize] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut out = source.rgba.clone();
    let at = |x: usize, y: usize, c: usize| source.rgba[(y * w + x) * 4 + c] as f32;
    // nearest unmasked neighbour along a row (left/right) and a column (up/down)
    let find = |x: usize, y: usize, dx: i64, dy: i64| -> Option<(usize, usize)> {
        let (mut cx, mut cy) = (x as i64, y as i64);
        loop {
            cx += dx;
            cy += dy;
            if cx < 0 || cy < 0 || cx as usize >= w || cy as usize >= h {
                return None;
            }
            if !masked[cy as usize * w + cx as usize] {
                return Some((cx as usize, cy as usize));
            }
        }
    };
    for y in 0..h {
        for x in 0..w {
            if !masked[y * w + x] {
                continue;
            }
            let (l, r, u, d) = (find(x, y, -1, 0), find(x, y, 1, 0), find(x, y, 0, -1), find(x, y, 0, 1));
            let lerp = |a: Option<(usize, usize)>, b: Option<(usize, usize)>, c: usize| -> Option<f32> {
                match (a, b) {
                    (Some(p), Some(q)) => {
                        let da = ((x as f32 - p.0 as f32).powi(2) + (y as f32 - p.1 as f32).powi(2)).sqrt();
                        let db = ((x as f32 - q.0 as f32).powi(2) + (y as f32 - q.1 as f32).powi(2)).sqrt();
                        Some((at(p.0, p.1, c) * db + at(q.0, q.1, c) * da) / (da + db).max(1e-6))
                    }
                    (Some(p), None) | (None, Some(p)) => Some(at(p.0, p.1, c)),
                    (None, None) => None,
                }
            };
            for c in 0..4 {
                let row = lerp(l, r, c);
                let col = lerp(u, d, c);
                let v = match (row, col) {
                    (Some(a), Some(b)) => (a + b) / 2.0,
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => at(x, y, c),
                };
                out[(y * w + x) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redraw::Source;

    fn solid(w: u32, h: u32, colour: [u8; 3]) -> Source {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
        Source { rgba, width: w, height: h }
    }

    fn paint(src: &mut Source, x0: u32, y0: u32, w: u32, h: u32, colour: [u8; 3]) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let i = ((y * src.width + x) * 4) as usize;
                src.rgba[i..i + 3].copy_from_slice(&colour);
            }
        }
    }

    /// Three "letters": two bars and an i (a bar with a dot over it), black on white.
    fn three_letters() -> (Source, [[f32; 2]; 4]) {
        let mut src = solid(120, 60, [255, 255, 255]);
        paint(&mut src, 20, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 40, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 60, 28, 6, 16, [0, 0, 0]);
        paint(&mut src, 60, 20, 6, 4, [0, 0, 0]);
        (src, [[15.0, 18.0], [75.0, 18.0], [75.0, 46.0], [15.0, 46.0]])
    }

    #[test]
    fn a_bar_of_another_colour_across_the_letters_is_an_occluder_and_a_plain_line_has_none() {
        let (src, quad) = three_letters();
        assert!(line_ink(&src, quad).occluders.is_empty());
        // a gold bar 4 px tall across the middle of the three bars
        let mut g = src.clone();
        paint(&mut g, 18, 30, 52, 4, [212, 160, 23]);
        let ink = line_ink(&g, quad);
        assert_eq!(ink.occluders.len(), 1, "{:?}", ink.occluders);
        let [r, gg, b] = ink.occluders[0];
        assert!(delta_e([r, gg, b], [212, 160, 23]) < 6.0);
        // the bar's pixels are not ink: the segments are still three
        assert_eq!(ink.segments.len(), 3);
    }

    /// The three letters with the rects of `under` (indices: the bars, the i's stem, its dot) also painted `dx`, `dy`
    /// px off and grown by `grow` in `colour` first, beneath them.
    fn three_letters_over(colour: [u8; 3], grow: u32, dx: u32, dy: u32, under: &[usize]) -> (Source, [[f32; 2]; 4]) {
        let mut src = solid(120, 60, [255, 255, 255]);
        let rects = [(20u32, 20u32, 6u32, 24u32), (40, 20, 6, 24), (60, 28, 6, 16), (60, 20, 6, 4)];
        for &(x, y, w, h) in under.iter().map(|&i| &rects[i]) {
            paint(&mut src, x + dx - grow, y + dy - grow, w + 2 * grow, h + 2 * grow, colour);
        }
        for &(x, y, w, h) in &rects {
            paint(&mut src, x, y, w, h, [0, 0, 0]);
        }
        (src, [[15.0, 18.0], [75.0, 18.0], [75.0, 46.0], [15.0, 46.0]])
    }

    #[test]
    fn a_drop_shadow_or_an_outline_of_another_colour_is_not_an_occluder() {
        // a light blue copy of the bars 3 px right and down, beneath them: ink on one side of it only. (The dot's
        // shadow is left out: 3 px down it falls into the gap between the dot and the stem, which has letter on
        // both sides of it, and reads as crossing; see the spec's Status.)
        let (src, quad) = three_letters_over([150, 170, 230], 0, 3, 3, &[0, 1, 2]);
        let ink = line_ink(&src, quad);
        assert!(ink.foreign.iter().filter(|f| **f).count() >= 3 * OCCLUDER_MIN_PX, "the shadow is off the ink's line");
        assert!(ink.occluders.is_empty(), "shadow read as {:?}", ink.occluders);
        assert_eq!(ink.segments.len(), 3);
        // a gold outline 2 px wide round every rect: ink on its inside only
        let (src, quad) = three_letters_over([212, 160, 23], 2, 0, 0, &[0, 1, 2, 3]);
        let ink = line_ink(&src, quad);
        assert!(ink.foreign.iter().filter(|f| **f).count() >= 3 * OCCLUDER_MIN_PX, "the outline is off the ink's line");
        assert!(ink.occluders.is_empty(), "outline read as {:?}", ink.occluders);
        assert_eq!(ink.segments.len(), 3);
    }

    #[test]
    fn a_script_laid_over_the_feet_of_a_letter_and_reaching_away_from_it_is_an_occluder() {
        // a bar, an arch (two stems under a cap) and a bar; a thick gold stroke covers the arch's feet and the gap
        // between them and runs on below the line: it crosses no stem, but most of it is away from the ink
        let mut src = solid(140, 70, [255, 255, 255]);
        paint(&mut src, 20, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 40, 20, 26, 5, [0, 0, 0]);
        paint(&mut src, 40, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 60, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 80, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 36, 36, 34, 20, [212, 160, 23]);
        let quad = [[15.0, 18.0], [95.0, 18.0], [95.0, 46.0], [15.0, 46.0]];
        let ink = line_ink(&src, quad);
        assert_eq!(ink.segments.len(), 3, "{:?}", ink.segments);
        assert_eq!(ink.occluders.len(), 1, "{:?}", ink.occluders);
        assert!(delta_e(ink.occluders[0], [212, 160, 23]) < 6.0);
    }

    #[test]
    fn reads_backdrop_ink_and_three_segments() {
        let (src, quad) = three_letters();
        let ink = line_ink(&src, quad);
        assert_eq!(ink.backdrop, [255, 255, 255]);
        assert_eq!(ink.colour, [0, 0, 0]);
        assert!(ink.spread < 1.0);
        assert_eq!(ink.segments.len(), 3, "{:?}", ink.segments);
        assert_eq!(ink.segments[0], (20 - ink.left, 26 - ink.left));
        assert_eq!(ink.ink_bounds, Some((20 - ink.left, 20 - ink.top, 66 - ink.left, 44 - ink.top)));
        let at = |x: u32, y: u32| ink.coverage[((y - ink.top) * ink.width + (x - ink.left)) as usize];
        assert!((at(22, 30) - 1.0).abs() < 0.01);
        assert!(at(30, 30) < 0.01);
    }

    #[test]
    fn declines_scripts_gradients_mismatches_and_rotation() {
        let (src, quad) = three_letters();
        let ink = line_ink(&src, quad);
        assert_eq!(decline(&ink, "lli", Class::Type, quad), None);
        assert_eq!(decline(&ink, "lli", Class::Script, quad), Some(Reason::Script));
        assert_eq!(decline(&ink, "hello world", Class::Type, quad), Some(Reason::Mismatch));
        let turned = [[15.0, 18.0], [75.0, 26.0], [75.0, 54.0], [15.0, 46.0]];
        assert_eq!(decline(&ink, "lli", Class::Type, turned), Some(Reason::Rotated));
        // a gradient ink: the bars go from black to mid grey across the line
        let mut g = src.clone();
        paint(&mut g, 40, 20, 6, 24, [90, 90, 90]);
        paint(&mut g, 60, 20, 6, 24, [160, 160, 160]);
        let ink = line_ink(&g, quad);
        assert_eq!(decline(&ink, "lli", Class::Type, quad), Some(Reason::Gradient));
    }

    #[test]
    fn paints_the_ink_out_flat_on_a_flat_backdrop_and_along_a_gradient() {
        let (src, quad) = three_letters();
        let ink = line_ink(&src, quad);
        let out = paint_out(&src, &[&ink]);
        assert!(out.chunks(4).all(|p| p[0] == 255 && p[3] == 255), "a flat backdrop stays flat");
        // a horizontal gradient backdrop: painted-out pixels sit within 2 levels of the gradient
        let mut g = solid(120, 60, [255, 255, 255]);
        for y in 0..60 {
            for x in 0..120 {
                paint(&mut g, x, y, 1, 1, [(255 - x) as u8, 240, 230]);
            }
        }
        paint(&mut g, 40, 20, 6, 24, [0, 0, 0]);
        let ink = line_ink(&g, quad);
        let out = paint_out(&g, &[&ink]);
        for y in 20..44u32 {
            for x in 40..46u32 {
                let r = out[((y * 120 + x) * 4) as usize];
                assert!((r as i32 - (255 - x as i32)).abs() <= 2, "({x},{y}) r={r}");
            }
        }
    }

    #[test]
    fn delta_e_is_zero_for_equal_colours_and_large_between_black_and_white() {
        assert_eq!(delta_e([10, 20, 30], [10, 20, 30]), 0.0);
        assert!(delta_e([0, 0, 0], [255, 255, 255]) > 90.0);
    }

    #[test]
    fn a_quad_outside_the_image_or_a_one_pixel_box_reads_nothing_and_does_not_panic() {
        let (src, _) = three_letters();
        let outside = [[200.0, 10.0], [260.0, 10.0], [260.0, 30.0], [200.0, 30.0]];
        let ink = line_ink(&src, outside);
        assert_eq!((ink.width, ink.height), (0, 0));
        assert!(ink.segments.is_empty() && ink.ink_bounds.is_none());
        assert_eq!(decline(&ink, "lli", Class::Type, outside), Some(Reason::Mismatch));
        assert_eq!(paint_out(&src, &[&ink]), src.rgba);
        let above = [[10.0, -90.0], [60.0, -90.0], [60.0, -50.0], [10.0, -50.0]];
        assert_eq!(line_ink(&src, above).width, 0);
        let nothing = Source { rgba: Vec::new(), width: 0, height: 0 };
        assert_eq!(line_ink(&nothing, outside).width, 0);
        // a box of one pixel: a one-pixel image
        let one = solid(1, 1, [10, 20, 30]);
        let q = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let ink = line_ink(&one, q);
        assert_eq!((ink.width, ink.height), (1, 1));
        let _ = decline(&ink, "l", Class::Type, q);
        let _ = paint_out(&one, &[&ink]);
        // and a quad hugging the last column, two pixels wide after clipping
        let edge = [[119.0, 10.0], [200.0, 10.0], [200.0, 14.0], [119.0, 14.0]];
        let ink = line_ink(&src, edge);
        assert!(ink.width == 2 && ink.left + ink.width == 120);
        let _ = paint_out(&src, &[&ink]);
    }

    #[test]
    fn a_quad_straddling_the_right_and_bottom_edges_is_clipped_and_its_ink_still_read() {
        let mut src = solid(60, 40, [255, 255, 255]);
        paint(&mut src, 40, 20, 6, 20, [0, 0, 0]);
        let quad = [[35.0, 18.0], [75.0, 18.0], [75.0, 50.0], [35.0, 50.0]];
        let ink = line_ink(&src, quad);
        assert_eq!(ink.left + ink.width, 60);
        assert_eq!(ink.top + ink.height, 40);
        assert_eq!(ink.colour, [0, 0, 0]);
        assert_eq!(ink.segments.len(), 1);
        let (l, _, r, b) = ink.ink_bounds.unwrap();
        assert_eq!((l + ink.left, r + ink.left, b + ink.top), (40, 46, 40));
        let out = paint_out(&src, &[&ink]);
        assert!(out.chunks(4).all(|p| p[0] == 255));
    }

    /// Five bars, 4 px wide and 24 tall, with the gaps given, black on white.
    fn bars(gaps: [u32; 4]) -> (Source, [[f32; 2]; 4]) {
        let mut src = solid(200, 60, [255, 255, 255]);
        let mut x = 20;
        for i in 0..5 {
            paint(&mut src, x, 20, 4, 24, [0, 0, 0]);
            x += 4 + gaps.get(i).copied().unwrap_or(0);
        }
        (src, [[15.0, 18.0], [x as f32 + 5.0, 18.0], [x as f32 + 5.0, 46.0], [15.0, 46.0]])
    }

    #[test]
    fn a_word_gap_the_reader_dropped_is_put_back_and_nothing_else_changes() {
        // a wide word gap after the third letter, letter gaps of 8
        let (src, quad) = bars([8, 8, 22, 8]);
        let ink = line_ink(&src, quad);
        assert_eq!(ink.segments.len(), 5);
        assert_eq!(repair_spaces(&ink, "ABCDE"), "ABC DE");
        // a space already there stays the only one
        assert_eq!(repair_spaces(&ink, "ABC DE"), "ABC DE");
        // the string and the ink disagree: the string is left as it is
        assert_eq!(repair_spaces(&ink, "ABCD"), "ABCD");
        // even gaps: no space
        let (src, quad) = bars([8, 8, 9, 8]);
        assert_eq!(repair_spaces(&line_ink(&src, quad), "ABCDE"), "ABCDE");
        // tight text: a gap twice the median of two is still not a word space
        let (src, quad) = bars([2, 2, 5, 2]);
        assert_eq!(repair_spaces(&line_ink(&src, quad), "ABCDE"), "ABCDE");
    }

    #[test]
    fn a_rule_or_a_divider_of_another_colour_in_the_grown_box_is_not_a_gradient() {
        // the three letters, with a gold divider beside them taller than the line and a gold rule under them, both
        // inside the box grown round the quad but neither a character
        let (mut src, quad) = three_letters();
        paint(&mut src, 70, 13, 4, 38, [190, 150, 80]);
        paint(&mut src, 18, 46, 52, 3, [190, 150, 80]);
        let ink = line_ink(&src, quad);
        assert!(ink.spread < INK_SPREAD_MAX, "spread {}", ink.spread);
        assert_eq!(ink.colour, [0, 0, 0]);
        assert_ne!(decline(&ink, "lli", Class::Type, quad), Some(Reason::Gradient));
        // a second ink colour on the characters themselves still is one
        paint(&mut src, 40, 20, 6, 24, [190, 150, 80]);
        paint(&mut src, 60, 28, 6, 16, [120, 90, 50]);
        assert_eq!(decline(&line_ink(&src, quad), "lli", Class::Type, quad), Some(Reason::Gradient));
    }

    #[test]
    fn a_large_ink_with_grain_and_a_darker_speck_or_two_is_not_a_gradient() {
        // display letters as an image generator paints them: a body of 40 with ±3 levels of grain and a few specks
        // of 15, which are the furthest pixels from the backdrop and so set the ink colour. The body is one colour;
        // measured from the specks it was 12 ΔE away everywhere and the line was declined as a gradient
        let mut src = solid(160, 80, [250, 248, 244]);
        for (x0, w) in [(20u32, 14u32), (50, 14), (80, 14)] {
            for y in 15..65u32 {
                for x in x0..x0 + w {
                    let v = (40 + ((x * 7 + y * 13) % 7) as i32 - 3) as u8;
                    paint(&mut src, x, y, 1, 1, [v, v, v]);
                }
            }
            paint(&mut src, x0 + 4, 30, 1, 1, [15, 15, 15]);
            paint(&mut src, x0 + 9, 50, 1, 1, [15, 15, 15]);
        }
        let quad = [[15.0, 12.0], [100.0, 12.0], [100.0, 68.0], [15.0, 68.0]];
        let ink = line_ink(&src, quad);
        assert!(ink.colour[0] < 30, "the specks set the ink colour: {:?}", ink.colour);
        assert!(ink.spread < INK_SPREAD_MAX, "spread {}", ink.spread);
        assert_eq!(decline(&ink, "III", Class::Type, quad), None);
    }

    #[test]
    fn a_neighbour_of_another_colour_reaching_into_the_box_is_neither_ink_nor_painted_out() {
        // a gold swash crosses the bottom of the grown box under the three letters, as a script under a wordmark does
        let (mut src, quad) = three_letters();
        paint(&mut src, 10, 45, 100, 3, [200, 150, 90]);
        let ink = line_ink(&src, quad);
        assert_eq!(ink.colour, [0, 0, 0]);
        assert_eq!(ink.segments.len(), 3, "{:?}", ink.segments);
        assert_eq!(ink.ink_bounds, Some((20 - ink.left, 20 - ink.top, 66 - ink.left, 44 - ink.top)));
        let out = paint_out(&src, &[&ink]);
        for x in 10..110u32 {
            let i = ((46 * 120 + x) * 4) as usize;
            assert_eq!(&out[i..i + 3], &[200, 150, 90], "the swash at x {x} is left as it was");
        }
    }

    /// Three wide letters (14 px by 50) whose grey runs from `from` to `to` across the line, column by column.
    fn ramp(from: u8, to: u8) -> (Source, [[f32; 2]; 4]) {
        let mut src = solid(160, 80, [250, 248, 244]);
        let (x0, x1) = (20u32, 94u32);
        for (l, w) in [(20u32, 14u32), (50, 14), (80, 14)] {
            for x in l..l + w {
                let v = (from as f32 + (to as f32 - from as f32) * (x - x0) as f32 / (x1 - x0) as f32).round() as u8;
                paint(&mut src, x, 15, 1, 50, [v, v, v]);
            }
        }
        (src, [[15.0, 12.0], [100.0, 12.0], [100.0, 68.0], [15.0, 68.0]])
    }

    #[test]
    fn a_smooth_ramp_of_a_dozen_de_across_the_letters_is_a_gradient_and_one_of_four_is_not() {
        // 40 to 75 grey is 14 ΔE of lightness; 40 to 50 is 4
        assert!((12.0..16.0).contains(&delta_e([40; 3], [75; 3])), "{}", delta_e([40; 3], [75; 3]));
        let (src, quad) = ramp(40, 75);
        let ink = line_ink(&src, quad);
        assert!(ink.spread > INK_SPREAD_MAX, "a 14 ΔE ramp reads {}", ink.spread);
        assert_eq!(decline(&ink, "III", Class::Type, quad), Some(Reason::Gradient));
        assert!(delta_e([40; 3], [50; 3]) < 5.0);
        let (src, quad) = ramp(40, 50);
        let ink = line_ink(&src, quad);
        assert!(ink.spread < INK_SPREAD_MAX, "a 4 ΔE ramp reads {}", ink.spread);
        assert_eq!(decline(&ink, "III", Class::Type, quad), None);
    }

    /// Bars 24 px tall: (left, width) each, black on white, and the quad round them.
    fn glyph_bars(glyphs: &[(u32, u32)]) -> (Source, [[f32; 2]; 4]) {
        let mut src = solid(260, 60, [255, 255, 255]);
        for &(x, w) in glyphs {
            paint(&mut src, x, 20, w, 24, [0, 0, 0]);
        }
        let right = glyphs.last().map_or(20, |&(x, w)| x + w) as f32;
        (src, [[15.0, 18.0], [right + 5.0, 18.0], [right + 5.0, 46.0], [15.0, 46.0]])
    }

    #[test]
    fn a_narrow_digit_in_a_tabular_cell_is_not_set_apart_and_a_dropped_word_space_still_is() {
        // "ABCDE 20106" read without its space: letters and digits 12 px wide 4 px apart (a median gap of a tenth
        // of an em), the tabular 1 a 3 px stem centred in a 12 px cell, so 9 px either side of it (a quarter em),
        // and the word gap 20 px
        let mut glyphs = Vec::new();
        let mut x = 20;
        for (w, gap) in [(12, 4), (12, 4), (12, 4), (12, 4), (12, 20), (12, 4), (12, 9), (3, 9), (12, 4), (12, 0)] {
            glyphs.push((x, w));
            x += w + gap;
        }
        let (src, quad) = glyph_bars(&glyphs);
        let ink = line_ink(&src, quad);
        assert_eq!(ink.segments.len(), 10);
        assert_eq!(repair_spaces(&ink, "ABCDE20106"), "ABCDE 20106");
        // the same gaps between letters are not word spaces either: a quarter em is under the excess
        assert_eq!(repair_spaces(&ink, "ABCDEFGHIJ"), "ABCDE FGHIJ");
    }

    #[test]
    fn anti_aliased_edges_do_not_make_a_flat_ink_a_gradient() {
        let (mut src, quad) = three_letters();
        for (x0, y0, w, h) in [(20u32, 20u32, 6u32, 24u32), (40, 20, 6, 24), (60, 28, 6, 16), (60, 20, 6, 4)] {
            paint(&mut src, x0, y0, 1, h, [186, 186, 186]);
            paint(&mut src, x0 + w - 1, y0, 1, h, [186, 186, 186]);
            paint(&mut src, x0, y0, w, 1, [235, 235, 235]);
            paint(&mut src, x0, y0 + h - 1, w, 1, [235, 235, 235]);
        }
        let ink = line_ink(&src, quad);
        assert!(ink.spread < INK_SPREAD_MAX, "spread {}", ink.spread);
        assert_eq!(decline(&ink, "lli", Class::Type, quad), None);
    }
}
