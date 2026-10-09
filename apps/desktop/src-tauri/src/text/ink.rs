//! What a line of text is made of, read from the pixels: its backdrop (the box's border ring), its ink colour
//! (the pixels furthest from the backdrop), per-pixel coverage (where a pixel sits between the two in linear RGB),
//! the mask, the per-character segments, and the reasons a line is left to the trace. `paint_out` is the
//! inverse: the ink gone, the backdrop continued through it.
use super::{Class, Reason, INK_SPREAD_MAX, MISMATCH_SHARE, ROTATED_DEG};
use crate::redraw::Source;

/// How far a pixel may be from each of its eight neighbours and still be in the ink's core.
const CORE_FLAT: f32 = 6.0;

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
}

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
    let empty = LineInk { left: 0, top: 0, width: 0, height: 0, coverage: Vec::new(), mask: Vec::new(), backdrop: [0, 0, 0], colour: [0, 0, 0], spread: 0.0, segments: Vec::new(), ink_bounds: None };
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
    let mut coverage = vec![0f32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let p = px(left + x, top + y);
            let l = [srgb_to_linear(p[0]) - b[0], srgb_to_linear(p[1]) - b[1], srgb_to_linear(p[2]) - b[2]];
            let t = (l[0] * axis[0] + l[1] * axis[1] + l[2] * axis[2]) / len2;
            coverage[(y * width + x) as usize] = t.clamp(0.0, 1.0);
        }
    }
    let mask: Vec<bool> = coverage.iter().map(|&c| c >= 0.5).collect();
    // the spread: 90th percentile ΔE from the ink colour over the ink's core, since an edge pixel is a blend of
    // ink and backdrop by construction. The core is the mask pixels whose eight neighbours are all in the mask and
    // within CORE_FLAT of the pixel itself: a blurred or anti-aliased edge is a ramp and falls out, a flat ink or a
    // gradient ink's interior stays. (Coverage >= 0.98 would not do: it is measured against the single ink colour,
    // so a second, lighter ink on the same line would never count as core and a gradient would read as flat.)
    // A line with fewer than 8 such pixels (a stem two pixels wide) uses its whole mask.
    let at = |x: u32, y: u32| px(left + x, top + y);
    let mut spreads: Vec<f32> = Vec::new();
    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            if !mask[(y * width + x) as usize] {
                continue;
            }
            let here = at(x, y);
            let flat = (y - 1..=y + 1).all(|ny| (x - 1..=x + 1).all(|nx| mask[(ny * width + nx) as usize] && delta_e(at(nx, ny), here) <= CORE_FLAT));
            if flat {
                spreads.push(delta_e(here, colour));
            }
        }
    }
    if spreads.len() < 8 {
        spreads = (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).filter(|&(x, y)| mask[(y * width + x) as usize]).map(|(x, y)| delta_e(at(x, y), colour)).collect();
    }
    spreads.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let spread = spreads.get(spreads.len() * 9 / 10).copied().unwrap_or(0.0);
    let segments = segments_of(&mask, width, height);
    let ink_bounds = bounds_of(&mask, width, height);
    LineInk { left, top, width, height, coverage, mask, backdrop, colour, spread, segments, ink_bounds }
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

/// Connected components (8-connected) of the mask as x ranges, merged where the ranges overlap.
fn segments_of(mask: &[bool], width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut label = vec![0u32; mask.len()];
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        let id = ranges.len() as u32 + 1;
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        stack.push(start);
        label[start] = id;
        while let Some(i) = stack.pop() {
            let (x, y) = ((i as u32) % width, (i as u32) / width);
            lo = lo.min(x);
            hi = hi.max(x + 1);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }
                    let j = (ny as u32 * width + nx as u32) as usize;
                    if mask[j] && label[j] == 0 {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        ranges.push((lo, hi));
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
