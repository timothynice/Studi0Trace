//! Stage 2: discontinuity map and the initial, edge-bounded partition.
//!
//! A discontinuity is a *ridge* of the colour gradient, not merely a large
//! gradient: a steep but smooth ramp has a large, flat gradient magnitude and
//! must stay one region, while a hard edge has a peak. Non-maximum suppression
//! along the local gradient orientation separates the two, as in Canny.

use crate::core::filters::{self, Mode};
use crate::core::grid::{Grid, Image, Mask};
use crate::core::labels::{self, Labels};
use crate::core::morphology::{dilate_cross, dilate_cross_n, erode_cross_n};
use rayon::prelude::*;
use crate::core::watershed::{watershed, watershed_masked};
use crate::timing::Timer;

/// Per-pixel colour change (ΔE-like units per pixel) across all feature channels.
pub fn discontinuity(features: &Image, sigma: f64) -> Grid<f64> {
    let (h, w) = (features.h, features.w);
    let mut g2 = Grid::<f64>::new(h, w);
    for c in 0..features.c {
        let g = filters::scharr(&features.channel(c));
        for i in 0..h * w {
            g2.data[i] = (g2.data[i] as f32 + (g.data[i] as f32) * (g.data[i] as f32)) as f64;
        }
    }
    let grad = Grid { h, w, data: g2.data.iter().map(|v| (*v as f32).sqrt() as f64).collect() };
    if sigma > 0.0 {
        filters::gaussian_filter_f32(&grad, sigma, Mode::Reflect, 0.0)
    } else {
        grad
    }
}

/// The local discontinuity floor (`partition.noise_floor`): the median of
/// `grad` over `NOISE_WINDOW`. Where it is under `CLEAN_FLOOR` the area is
/// clean and the ridge threshold is `G_LOW_MIN`, the junction test's
/// `G_SEED_MIN`; elsewhere they stay `g_low` and `g_seed` (`partition.py` says why).
pub const NOISE_WINDOW: usize = 9;
pub const CLEAN_FLOOR: f32 = 0.1;
pub const G_LOW_MIN: f32 = 0.75;
pub const G_SEED_MIN: f32 = 1.5;

pub fn noise_floor(grad: &Grid<f64>, features: &Image, g_low: f64) -> Grid<f64> {
    let (h, w) = (grad.h, grad.w);
    let inf = Grid::filled(h, w, f64::INFINITY);
    let (ridge, _) = ridges_and_valleys(features, grad, g_low, &inf);
    let band = dilate_cross_n(&ridge, 2);
    let half = (NOISE_WINDOW / 2) as isize;
    let mut out = Grid::new(h, w);
    let mut win: Vec<f32> = Vec::with_capacity(NOISE_WINDOW * NOISE_WINDOW);
    for r in 0..h {
        for c in 0..w {
            win.clear();
            for dr in -half..=half {
                let rr = (r as isize + dr).clamp(0, h as isize - 1) as usize;
                for dc in -half..=half {
                    let cc = (c as isize + dc).clamp(0, w as isize - 1) as usize;
                    let i = rr * w + cc;
                    if !band.data[i] {
                        win.push(grad.data[i] as f32);
                    }
                }
            }
            out.data[r * w + c] = if win.is_empty() {
                g_low
            } else {
                win.sort_by(|a, b| a.total_cmp(b));
                win[(win.len() - 1) / 2] as f64
            };
        }
    }
    out
}

/// The ridge threshold at each pixel: `g_low` where the floor is high, down to
/// `G_LOW_MIN` where the image is clean; `g_low` under transparent pixels
/// (`features[3] < 50`), whose inpainted colour has seams that are not edges.
fn threshold_at(g_low: f32, floor_min: f32, floor: f32, alpha: f64) -> f32 {
    if alpha >= 50.0 && floor < CLEAN_FLOOR {
        floor_min
    } else {
        g_low
    }
}

/// (ridge, valley) masks from non-maximum / non-minimum tests along the
/// structure-tensor gradient orientation.
pub fn ridges_and_valleys(features: &Image, grad: &Grid<f64>, g_low: f64, floor: &Grid<f64>) -> (Mask, Mask) {
    let mut t = Timer::new();
    let (h, w) = (grad.h, grad.w);
    let n = h * w;
    let mut jxx = Grid::<f64>::new(h, w);
    let mut jyy = Grid::<f64>::new(h, w);
    let mut jxy = Grid::<f64>::new(h, w);
    for c in 0..features.c {
        let ch = features.channel(c);
        let gy = filters::scharr_h(&ch);
        let gx = filters::scharr_v(&ch);
        for i in 0..n {
            jxx.data[i] = (jxx.data[i] as f32 + (gx.data[i] * gx.data[i]) as f32) as f64;
            jyy.data[i] = (jyy.data[i] as f32 + (gy.data[i] * gy.data[i]) as f32) as f64;
            jxy.data[i] = (jxy.data[i] as f32 + (gx.data[i] * gy.data[i]) as f32) as f64;
        }
    }
    t.lap("      ridges/scharr");
    let jxx = filters::gaussian_filter_f32(&jxx, 1.0, Mode::Reflect, 0.0);
    let jyy = filters::gaussian_filter_f32(&jyy, 1.0, Mode::Reflect, 0.0);
    let jxy = filters::gaussian_filter_f32(&jxy, 1.0, Mode::Reflect, 0.0);

    // One independent decision per pixel over four bilinear samples and a
    // trig call; at 768² that is the whole cost of the stage.
    let mut flags: Vec<u8> = vec![0; n];
    flags
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(r, row)| {
            for (c, slot) in row.iter_mut().enumerate() {
                let i = r * w + c;
                // The orientation and the sample offsets are computed in f32, as
                // the Python's float32 arrays are. At a plateau the near sample can
                // land within 1e-4 of the centre pixel, and `grad >= g_plus` then
                // turns on the last bit: carrying f64 here flips ridge pixels.
                let theta = 0.5f32
                    * (2.0f32 * jxy.data[i] as f32).atan2(jxx.data[i] as f32 - jyy.data[i] as f32);
                let (dx, dy) = (theta.cos(), theta.sin());
                let g = grad.data[i];
                let (rf, cf) = (r as f32, c as f32);
                // map_coordinates writes float32 for a float32 input, and the
                // comparisons below then happen in float32 too
                let at = |sy: f32, sx: f32| -> f32 {
                    filters::bilinear_nearest(grad, (rf + sy) as f64, (cf + sx) as f64) as f32
                };
                let gp = at(dy, dx);
                let gm = at(-dy, -dx);
                // Prominence is judged 1.5 px out: a real edge has fallen to ≈ 40 %
                // there, while the periodic bumps 8-bit quantisation puts on a steep
                // ramp are only a few percent high.
                let gpf = at(1.5f32 * dy, 1.5f32 * dx);
                let gmf = at(-1.5f32 * dy, -1.5f32 * dx);
                let g32 = g as f32;
                let prominent = g32 > 1.10f32 * gp.max(gm) || g32 > 1.10f32 * gpf.max(gmf);
                let g_low_eff = threshold_at(g_low as f32, G_LOW_MIN, floor.data[i] as f32, features.at(r, c)[3]);
                let is_ridge = g32 > g_low_eff && g32 >= gp && g32 >= gm && prominent;
                let is_valley = g32 <= gp && g32 <= gm && !is_ridge;
                *slot = is_ridge as u8 | ((is_valley as u8) << 1);
            }
        });
    t.lap("      ridges/nms");
    let ridge = Grid { h, w, data: flags.iter().map(|f| f & 1 != 0).collect() };
    let valley = Grid { h, w, data: flags.iter().map(|f| f & 2 != 0).collect() };
    (ridge, valley)
}

/// (smooth, ridge): the seed mask and the ridge mask it was cut by
/// (`partition.seed_parts`). Seeds: pixels clear of the ridge band, or valley
/// pixels, that also have a low gradient. NMS loses ridge pixels at
/// T-junctions, and those gap pixels still carry a high gradient, so the
/// gradient test keeps neighbouring regions from leaking into one seed.
pub fn seed_parts(features: &Image, grad: &Grid<f64>, g_low: f64, g_seed: f64) -> (Mask, Mask) {
    let floor = noise_floor(grad, features, g_low);
    let (ridge, valley) = ridges_and_valleys(features, grad, g_low, &floor);
    let band = dilate_cross(&ridge);
    let near_band = dilate_cross_n(&band, 2);
    let mut out = Grid::filled(grad.h, grad.w, false);
    let w = grad.w;
    for i in 0..grad.len() {
        let g_seed_eff = threshold_at(g_seed as f32, G_SEED_MIN, floor.data[i] as f32, features.at(i / w, i % w)[3]);
        let leaky = near_band.data[i] && (grad.data[i] as f32) >= g_seed_eff;
        out.data[i] = (!band.data[i] && !leaky) || valley.data[i];
    }
    (out, ridge)
}

/// The seed mask alone (`seed_parts`).
pub fn seed_mask(features: &Image, grad: &Grid<f64>, g_low: f64, g_seed: f64) -> Mask {
    seed_parts(features, grad, g_low, g_seed).0
}

/// A thin stroke is seeded as a stroke (`partition.stroke_seeds`, which says
/// why and how): widest feature and shortest component it admits.
pub const STROKE_SEED_W: usize = 5;
pub const STROKE_SEED_LEN: usize = 12;
/// A candidate's least opacity on the feature scale (100·alpha), where
/// `threshold_at` reads "clean": the colour under a transparent pixel is
/// inpainted and means nothing, and a stroke whose medial pixel is under half
/// covered is thinner than a pixel or translucent.
pub const STROKE_SEED_ALPHA: f64 = 50.0;
/// A stroke differs from both sides by more than this many `detail`s (the
/// sides' agreement and the one-colour spread stay at `detail`): JPEG's ringing
/// leaves one-colour streaks of 8-11 ΔE beside an edge in a soft field, and
/// legible text stands far above 12 ΔE from its ground.
pub const STROKE_SEED_CONTRAST: f64 = 2.0;
/// (dr, dc): the order ties between equally narrow axes break in.
const AXES: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];
/// The perpendicular of each of `AXES`: the direction a stroke runs in (`partition._ALONG`).
const ALONG: [(isize, isize); 4] = [(1, 0), (0, 1), (1, -1), (1, 1)];

/// Along +(dr, dc) from (r, c): the distance 1..W to the nearest ridge pixel
/// (0: none within W) and the distance to the first non-ridge pixel on the
/// image past that ridge's run, within W more (0: none). Off the image counts
/// as ridge for the second: no sample there (`partition._ridge_reach`).
fn ridge_reach(ridge: &Mask, r: usize, c: usize, dr: isize, dc: isize) -> (usize, usize) {
    let at = |k: usize| -> Option<bool> {
        let rr = r as isize + k as isize * dr;
        let cc = c as isize + k as isize * dc;
        if rr < 0 || cc < 0 || rr >= ridge.h as isize || cc >= ridge.w as isize {
            None
        } else {
            Some(ridge.data[rr as usize * ridge.w + cc as usize])
        }
    };
    let Some(dist) = (1..=STROKE_SEED_W).find(|k| at(*k) == Some(true)) else {
        return (0, 0);
    };
    match (dist + 1..=dist + STROKE_SEED_W).find(|k| at(*k) == Some(false)) {
        Some(k) => (dist, k),
        None => (dist, 0),
    }
}

/// The pixels of every stroke seed (`partition.stroke_seeds`): the non-ridge
/// pixels between two ridge pixels within `STROKE_SEED_W` along one of four
/// axes, in 4-connected components that are `STROKE_SEED_LEN` long (pixels
/// over mean width), one colour (RMS spread under `detail`), a different
/// colour from the first non-ridge pixel beyond the ridge on both sides by
/// more than `detail` while those two agree within `detail`, and not touching
/// a kept seed (a component of `smooth` of `floor` pixels or more) of their
/// own colour, read off the seed's pixels outside the stroke candidates. A
/// candidate is at least half opaque (`STROKE_SEED_ALPHA`), the two "differs"
/// tests are at `STROKE_SEED_CONTRAST`·`detail`, and an accepted seed stops
/// `STROKE_SEED_W` four-connected steps short of its end pixels (one with
/// nothing of its component one or two steps on, within a pixel either side,
/// along the perpendicular of its narrowest axis); a ring has none. Sums run
/// in raster order in f64, as the Python's do; the trim is a distance map.
pub fn stroke_seeds(ridge: &Mask, smooth: &Mask, features: &Image, detail: f64, floor: usize) -> Mask {
    let (h, w) = (ridge.h, ridge.w);
    let nc = features.c;
    let d2 = detail * detail;
    let none = (usize::MAX, usize::MAX);
    let mut width = vec![0usize; h * w]; // the narrowest axis's width; 0: not between two ridges
    let mut axis = vec![0u8; h * w]; // the narrowest axis's index into AXES (where width > 0)
    let mut out = vec![none; h * w]; // the samples beyond the + and - ridges
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            if ridge.data[i] {
                continue;
            }
            for (ai, (dr, dc)) in AXES.into_iter().enumerate() {
                let (dp, bp) = ridge_reach(ridge, r, c, dr, dc);
                let (dm, bm) = ridge_reach(ridge, r, c, -dr, -dc);
                if dp == 0 || dm == 0 {
                    continue;
                }
                let wd = dp + dm - 1;
                if wd > STROKE_SEED_W || (width[i] != 0 && wd >= width[i]) {
                    continue;
                }
                width[i] = wd;
                axis[i] = ai as u8;
                let idx = |k: usize, sr: isize, sc: isize| -> usize {
                    ((r as isize + k as isize * sr) as usize) * w + (c as isize + k as isize * sc) as usize
                };
                out[i] = if bp > 0 && bm > 0 { (idx(bp, dr, dc), idx(bm, -dr, -dc)) } else { none };
            }
        }
    }
    // at least half opaque
    let cand = Grid { h, w, data: (0..h * w).map(|i| width[i] > 0 && features.px(i)[3] >= STROKE_SEED_ALPHA).collect() };
    if !cand.any() {
        return cand;
    }
    let comps = labels::label_mask(&cand, 1);
    let n = comps.data.iter().copied().max().unwrap_or(0) as usize + 1;
    let size = labels::bincount(&comps);
    let mut wsum = vec![0usize; n];
    let mut sums = vec![0.0f64; n * nc];
    for i in 0..h * w {
        let k = comps.data[i] as usize;
        if k == 0 {
            continue;
        }
        wsum[k] += width[i];
        let px = features.px(i);
        for ch in 0..nc {
            sums[k * nc + ch] += px[ch];
        }
    }
    let mean = |k: usize, ch: usize| sums[k * nc + ch] / (size[k].max(1) as f64);
    let cd = STROKE_SEED_CONTRAST * detail;
    let c2 = cd * cd;
    let mut sq = vec![0.0f64; n];
    let mut passing = vec![0usize; n];
    for i in 0..h * w {
        let k = comps.data[i] as usize;
        if k == 0 {
            continue;
        }
        let px = features.px(i);
        let mut dsq = 0.0f64;
        for ch in 0..nc {
            let d = px[ch] - mean(k, ch);
            dsq += d * d;
        }
        sq[k] += dsq;
        let (a, b) = out[i];
        if a != usize::MAX {
            let (fa, fb) = (features.px(a), features.px(b));
            let (mut da, mut db, mut ds) = (0.0f64, 0.0f64, 0.0f64);
            for ch in 0..nc {
                da += (px[ch] - fa[ch]) * (px[ch] - fa[ch]);
                db += (px[ch] - fb[ch]) * (px[ch] - fb[ch]);
                ds += (fa[ch] - fb[ch]) * (fa[ch] - fb[ch]);
            }
            if da > c2 && db > c2 && ds < d2 {
                passing[k] += 1;
            }
        }
    }
    // kept seeds: components of `smooth` of `floor` pixels or more. Colour is read
    // off the pixels outside the stroke candidates (a stroke's own valley line may
    // join the neighbouring seed in one component and would pull its colour).
    let kept = labels::label_mask(smooth, 1);
    let nk = kept.data.iter().copied().max().unwrap_or(0) as usize + 1;
    let ksize = labels::bincount(&kept);
    let mut kcount = vec![0usize; nk];
    let mut ksums = vec![0.0f64; nk * nc];
    for i in 0..h * w {
        if !smooth.data[i] || cand.data[i] {
            continue;
        }
        let k = kept.data[i] as usize;
        kcount[k] += 1;
        let px = features.px(i);
        for ch in 0..nc {
            ksums[k * nc + ch] += px[ch];
        }
    }
    let kmean = |k: usize, ch: usize| ksums[k * nc + ch] / (kcount[k].max(1) as f64);
    // a component of the stroke's own pixels alone has no other colour to compare
    let big: Vec<bool> = (0..nk).map(|k| k != 0 && ksize[k] >= floor && kcount[k] > 0).collect();
    let mut pairs = std::collections::BTreeSet::new();
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            let k = comps.data[i] as usize;
            if k == 0 {
                continue;
            }
            for (dr, dc) in [(0isize, 0isize), (0, 1), (0, -1), (1, 0), (-1, 0)] {
                let (rr, cc) = (r as isize + dr, c as isize + dc);
                if rr < 0 || cc < 0 || rr >= h as isize || cc >= w as isize {
                    continue;
                }
                let kk = kept.data[rr as usize * w + cc as usize] as usize;
                if big[kk] {
                    pairs.insert((k, kk));
                }
            }
        }
    }
    let mut touching = vec![false; n];
    for (k, kk) in pairs {
        let mut dsq = 0.0f64;
        for ch in 0..nc {
            let d = mean(k, ch) - kmean(kk, ch);
            dsq += d * d;
        }
        if dsq < d2 {
            touching[k] = true;
        }
    }
    let accept: Vec<bool> = (0..n)
        .map(|k| {
            k != 0
                && size[k] * size[k] >= STROKE_SEED_LEN * wsum[k]
                && sq[k] / (size[k].max(1) as f64) < d2
                && 2 * passing[k] > size[k]
                && !touching[k]
        })
        .collect();
    let keep: Vec<bool> = comps.data.iter().map(|k| accept[*k as usize]).collect();
    // stop short of the ends: a multi-source BFS, four-connected inside the kept
    // pixels, from every end pixel to depth STROKE_SEED_W (a distance map: no
    // visiting order decides anything)
    let mut dist = vec![usize::MAX; h * w];
    let mut queue = std::collections::VecDeque::new();
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            if !keep[i] {
                continue;
            }
            let ai = axis[i] as usize;
            let ((dr, dc), (ar, ac)) = (AXES[ai], ALONG[ai]);
            let continues = |sgn: isize| {
                [1isize, 2].iter().any(|k| {
                    [-1isize, 0, 1].iter().any(|j| {
                        let rr = r as isize + sgn * k * ar + j * dr;
                        let cc = c as isize + sgn * k * ac + j * dc;
                        rr >= 0 && cc >= 0 && rr < h as isize && cc < w as isize && comps.data[rr as usize * w + cc as usize] == comps.data[i]
                    })
                })
            };
            if !continues(1) || !continues(-1) {
                dist[i] = 0;
                queue.push_back(i);
            }
        }
    }
    while let Some(i) = queue.pop_front() {
        if dist[i] >= STROKE_SEED_W {
            continue;
        }
        let (r, c) = (i / w, i % w);
        let mut step = |j: usize| {
            if keep[j] && dist[j] == usize::MAX {
                dist[j] = dist[i] + 1;
                queue.push_back(j);
            }
        };
        if r > 0 {
            step(i - w);
        }
        if r + 1 < h {
            step(i + w);
        }
        if c > 0 {
            step(i - 1);
        }
        if c + 1 < w {
            step(i + 1);
        }
    }
    Grid { h, w, data: (0..h * w).map(|i| keep[i] && dist[i] == usize::MAX).collect() }
}

/// Flood regions below `min_region` pixels from their neighbours along low gradient.
/// `protected[label]` exempts a label: a stroke seed's basin is a region however small.
fn absorb_small(labels: &Labels, grad: &Grid<f64>, min_region: usize, rounds: usize, protected: &[bool]) -> Labels {
    let mut cur = labels.clone();
    for _ in 0..rounds {
        let sizes = labels::bincount(&cur);
        let small: Vec<bool> = sizes.iter().enumerate().map(|(i, s)| i != 0 && *s < min_region && !protected.get(i).copied().unwrap_or(false)).collect();
        if !small.iter().any(|b| *b) {
            break;
        }
        let mut markers = cur.clone();
        let mut max = 0i32;
        for v in markers.data.iter_mut() {
            if *v >= 0 && small[*v as usize] {
                *v = 0;
            }
            if *v > max {
                max = *v;
            }
        }
        if max == 0 {
            return Grid::filled(cur.h, cur.w, 1);
        }
        cur = watershed(grad, &markers);
    }
    cur
}

/// How many cross erosions find the pieces a neck in a seed joins (`partition.py`).
pub const NECK_ERODE: usize = 3;
/// How far the boundary between two split pieces must stand above their
/// interiors for the split to be a step, not a ramp (`partition.rejoin_ramps`).
pub const NECK_PROMINENCE: f64 = 3.0;
/// ... and the share of the two markers' colour difference it must carry.
pub const NECK_STEP: f64 = 0.25;
/// ... and a step is a ridge: at more than half of the boundary's pairs the
/// discontinuity is at least this many times the higher of the two
/// `NECK_REACH` pixels to either side of the pair (`partition.py` says why).
pub const NECK_RIDGE: f64 = 1.25;
pub const NECK_REACH: usize = 3;
/// Fewest pixels a piece needs, after the erosion, to be seeded apart.
pub const NECK_PIECE: usize = 64;

/// The watershed's markers: the seed mask's connected components of at least
/// `floor` pixels, each split where a neck joins areas more than `detail`
/// apart in mean feature colour (`partition.seed_markers`, which says why).
/// Numbered in raster order of each marker's first pixel; with, per marker
/// id, the seed component it was split from (0 for one seeded whole).
pub fn seed_markers(smooth: &Mask, floor: usize, features: &Image, detail: f64, grad: &Grid<f64>, strokes: Option<&Mask>) -> (Labels, Vec<i64>) {
    let joined: Mask = match strokes {
        Some(s) => smooth.or(s),
        None => smooth.clone(),
    };
    let mut comps = labels::label_mask(&joined, 1);
    if comps.data.iter().copied().max().unwrap_or(0) == 0 {
        return (comps, vec![0]);
    }
    let sizes = labels::bincount(&comps);
    let mut keep: Vec<bool> = sizes.iter().enumerate().map(|(i, s)| i != 0 && *s >= floor).collect();
    if let Some(s) = strokes {
        // a stroke seed is kept whatever its area
        for (c, is_stroke) in comps.data.iter().zip(s.data.iter()) {
            if *is_stroke && *c > 0 {
                keep[*c as usize] = true;
            }
        }
    }
    for v in comps.data.iter_mut() {
        if *v > 0 && !keep[*v as usize] {
            *v = 0;
        }
    }
    let seeded = Grid { h: comps.h, w: comps.w, data: comps.data.iter().map(|v| *v > 0).collect() };
    let core = erode_cross_n(&seeded, NECK_ERODE, true);
    let pieces = labels::label_mask(&core, 1);
    let psize = labels::bincount(&pieces);
    let n_p = psize.len();
    let big: Vec<bool> = psize.iter().enumerate().map(|(i, s)| i != 0 && *s >= floor).collect();
    let mut owner = vec![0i32; n_p];
    for (p, c) in pieces.data.iter().zip(comps.data.iter()) {
        owner[*p as usize] = *c;
    }
    let mut count = vec![0usize; sizes.len()];
    let mut by_comp: Vec<Vec<usize>> = vec![Vec::new(); sizes.len()];
    for p in 0..n_p {
        if big[p] {
            count[owner[p] as usize] += 1;
            by_comp[owner[p] as usize].push(p);
        }
    }
    let mut out: Vec<i64> = comps.data.iter().map(|v| *v as i64).collect();
    let mut split = vec![false; sizes.len()];
    if count.iter().any(|n| *n >= 2) {
        // mean feature colour per piece, summed in raster order in f64
        let nc = features.c;
        let mut sums = vec![0.0f64; n_p * nc];
        for (i, p) in pieces.data.iter().enumerate() {
            let px = features.at(i / features.w, i % features.w);
            for ch in 0..nc {
                sums[*p as usize * nc + ch] += px[ch];
            }
        }
        let mean = |p: usize, ch: usize| sums[p * nc + ch] / (psize[p].max(1) as f64);
        let mut group = vec![0i64; n_p];
        let mut next_id = sizes.len() as i64;
        for c in 0..sizes.len() {
            if count[c] < 2 {
                continue;
            }
            let mut ps = by_comp[c].clone();
            ps.sort_by(|a, b| psize[*b].cmp(&psize[*a]).then(a.cmp(b)));
            let mut anchors: Vec<(usize, i64)> = Vec::new();
            for &p in &ps {
                let mut joined = false;
                for &(a, gid) in &anchors {
                    let mut d2 = 0.0f64;
                    for ch in 0..nc {
                        let d = mean(p, ch) - mean(a, ch);
                        d2 += d * d;
                    }
                    if d2 < detail * detail {
                        group[p] = gid;
                        joined = true;
                        break;
                    }
                }
                if !joined && (anchors.is_empty() || psize[p] >= NECK_PIECE) {
                    anchors.push((p, next_id));
                    group[p] = next_id;
                    next_id += 1;
                }
            }
            if anchors.len() >= 2 {
                split[c] = true;
            } else {
                for &p in &ps {
                    group[p] = 0;
                }
            }
        }
        // each group grown back over its component's own seed (`partition.py`)
        let inside = Grid { h: comps.h, w: comps.w, data: comps.data.iter().map(|c| split[*c as usize]).collect() };
        let ids = Grid {
            h: comps.h,
            w: comps.w,
            data: (0..out.len())
                .map(|i| if inside.data[i] { group[pieces.data[i] as usize] as i32 } else { 0 })
                .collect(),
        };
        let ids = watershed_masked(grad, &ids, Some(&inside));
        for i in 0..out.len() {
            if inside.data[i] {
                out[i] = ids.data[i] as i64;
            }
        }
    }
    // renumber 1..K in raster order of each id's first pixel
    let mut map: std::collections::HashMap<i64, i32> = std::collections::HashMap::new();
    let mut data = vec![0i32; out.len()];
    for (i, v) in out.iter().enumerate() {
        if *v == 0 {
            continue;
        }
        let next = map.len() as i32 + 1;
        data[i] = *map.entry(*v).or_insert(next);
    }
    let mut origin = vec![0i64; map.len() + 1];
    for (i, m) in data.iter().enumerate() {
        let c = comps.data[i];
        if *m > 0 && split[c as usize] {
            origin[*m as usize] = c as i64;
        }
    }
    (Grid { h: comps.h, w: comps.w, data }, origin)
}

/// Join again the basins of a split seed that no step divides
/// (`partition.rejoin_ramps`, which says why): two adjacent basins split from
/// one component stay apart only when the mean discontinuity along their
/// shared boundary is at least `NECK_PROMINENCE` times the lower-middle
/// discontinuity over either's marker and `NECK_STEP` times the distance
/// between the markers' mean feature colours, and more than half of its pairs
/// are `NECK_RIDGE` times steeper than the pixels `NECK_REACH` to either side.
/// Boundary sums run horizontal pairs, then vertical ones, each in raster
/// order, as the Python's do.
pub fn rejoin_ramps(l: &Labels, markers: &Labels, origin: &[i64], grad: &Grid<f64>, features: &Image) -> Labels {
    if !origin.iter().any(|o| *o > 0) {
        return l.clone();
    }
    let k = origin.len();
    let mut vals: Vec<Vec<f64>> = vec![Vec::new(); k];
    for (m, g) in markers.data.iter().zip(grad.data.iter()) {
        if *m > 0 && origin[*m as usize] > 0 {
            vals[*m as usize].push(*g);
        }
    }
    let mut interior = vec![0.0f64; k];
    for (m, v) in vals.iter_mut().enumerate() {
        if v.is_empty() {
            continue;
        }
        v.sort_by(|a, b| a.total_cmp(b));
        interior[m] = v[(v.len() - 1) / 2];
    }
    // mean feature colour per marker, summed in raster order in f64
    let nc = features.c;
    let mut count = vec![0usize; k];
    let mut sums = vec![0.0f64; k * nc];
    for (i, m) in markers.data.iter().enumerate() {
        let m = *m as usize;
        count[m] += 1;
        let px = features.at(i / features.w, i % features.w);
        for ch in 0..nc {
            sums[m * nc + ch] += px[ch];
        }
    }
    let colour = |m: usize, ch: usize| sums[m * nc + ch] / (count[m].max(1) as f64);
    let (h, w) = (l.h, l.w);
    // per pair of pieces: pairs, sum of the boundary discontinuity, ridge pairs
    let mut acc: std::collections::BTreeMap<(i32, i32), (f64, f64, f64)> = std::collections::BTreeMap::new();
    let mut visit = |a: i32, b: i32, g: f64, before: f64, after: f64| {
        if a != b && origin[a as usize] > 0 && origin[a as usize] == origin[b as usize] {
            let e = acc.entry((a.min(b), a.max(b))).or_insert((0.0, 0.0, 0.0));
            e.0 += 1.0;
            e.1 += g;
            if g >= NECK_RIDGE * before.max(after) {
                e.2 += 1.0;
            }
        }
    };
    for r in 0..h {
        for c in 0..w.saturating_sub(1) {
            let i = r * w + c;
            let before = grad.data[r * w + c.saturating_sub(NECK_REACH)];
            let after = grad.data[r * w + (c + 1 + NECK_REACH).min(w - 1)];
            visit(l.data[i], l.data[i + 1], 0.5 * (grad.data[i] + grad.data[i + 1]), before, after);
        }
    }
    for r in 0..h.saturating_sub(1) {
        for c in 0..w {
            let i = r * w + c;
            let before = grad.data[r.saturating_sub(NECK_REACH) * w + c];
            let after = grad.data[(r + 1 + NECK_REACH).min(h - 1) * w + c];
            visit(l.data[i], l.data[i + w], 0.5 * (grad.data[i] + grad.data[i + w]), before, after);
        }
    }
    if acc.is_empty() {
        return l.clone();
    }
    let mut parent: Vec<usize> = (0..k).collect();
    fn find(parent: &[usize], mut i: usize) -> usize {
        while parent[i] != i {
            i = parent[i];
        }
        i
    }
    for ((a, b), (cnt, gsum, ridge)) in acc.iter() {
        let (a, b) = (*a as usize, *b as usize);
        let edge = gsum / cnt;
        let mut d2 = 0.0f64;
        for ch in 0..nc {
            let d = colour(a, ch) - colour(b, ch);
            d2 += d * d;
        }
        if edge >= NECK_PROMINENCE * interior[a].max(interior[b]) && edge >= NECK_STEP * d2.sqrt() && 2.0 * ridge > *cnt {
            continue;
        }
        let (ra, rb) = (find(&parent, a), find(&parent, b));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let root: Vec<i32> = (0..k).map(|i| find(&parent, i) as i32).collect();
    Grid { h, w, data: l.data.iter().map(|v| root[*v as usize]).collect() }
}

/// Watershed on the discontinuity map, seeded by the connected smooth areas.
pub fn initial_labels(grad: &Grid<f64>, features: &Image, min_region: usize, g_low: f64, detail: f64) -> Labels {
    let mut t = Timer::new();
    let (smooth, ridge) = seed_parts(features, grad, g_low, 8.0);
    t.lap("    labels/seed_mask");
    let strokes = stroke_seeds(&ridge, &smooth, features, detail, min_region.max(2));
    t.lap("    labels/stroke_seeds");
    if crate::dump::enabled() {
        let ints = |m: &Mask| Grid { h: m.h, w: m.w, data: m.data.iter().map(|b| *b as i32).collect::<Vec<i32>>() };
        crate::dump::labels("seed_mask", &ints(&smooth));
        crate::dump::labels("stroke_seeds", &ints(&strokes));
    }
    let (mut markers, mut origin) = seed_markers(&smooth, min_region.max(2), features, detail, grad, Some(&strokes));
    if crate::dump::enabled() {
        crate::dump::labels("seed_markers", &markers);
    }
    // a stroke seed's basin is a region however small: exempt from the small-region absorb
    let mut protected = vec![false; markers.data.iter().copied().max().unwrap_or(0) as usize + 1];
    for (m, s) in markers.data.iter().zip(strokes.data.iter()) {
        if *s && *m > 0 {
            protected[*m as usize] = true;
        }
    }
    t.lap("    labels/cc");
    if markers.data.iter().copied().max().unwrap_or(0) == 0 {
        // No smooth area at all: seed from the local minima of the gradient so
        // the watershed still has basins.
        let mn = filters::minimum_filter3(grad);
        let minima = Grid {
            h: grad.h,
            w: grad.w,
            data: (0..grad.len()).map(|i| mn.data[i] == grad.data[i]).collect(),
        };
        markers = labels::label_mask(&minima, 1);
        if markers.data.iter().copied().max().unwrap_or(0) == 0 {
            return Grid::filled(grad.h, grad.w, 1);
        }
        origin = vec![0; markers.data.iter().copied().max().unwrap_or(0) as usize + 1];
        protected = Vec::new();
    }
    let l = watershed(grad, &markers);
    t.lap("    labels/watershed");
    let l = rejoin_ramps(&l, &markers, &origin, grad, features);
    let l = absorb_small(&l, grad, min_region, 3, &protected);
    t.lap("    labels/absorb_small");
    labels::relabel_sequential(&l).0
}

#[cfg(test)]
mod stroke_tests {
    use super::*;

    const FLOOR: usize = 40;

    /// `tests/test_vexel_partition.py::_stroke_scene`: a 3 px stroke on rows 18-20
    /// (columns c0..c1-1) with ridge pixels round it; the seed mask is everything
    /// more than one pixel from a ridge.
    fn scene(c0: usize, c1: usize, ink: f64, above: f64, below: f64) -> (Mask, Mask, Image) {
        let (h, w) = (40, 40);
        let mut f = Image::new(h, w, 4);
        for r in 0..h {
            for c in 0..w {
                let px = f.px_mut(r * w + c);
                px[0] = if r < 19 { above } else { below };
                px[3] = 100.0;
            }
        }
        for r in 18..21 {
            for c in c0..c1 {
                f.px_mut(r * w + c)[0] = ink;
            }
        }
        let mut ridge = Grid::filled(h, w, false);
        for c in c0 - 1..=c1 {
            ridge.set(17, c, true);
            ridge.set(21, c, true);
        }
        for r in 17..22 {
            ridge.set(r, c0 - 1, true);
            ridge.set(r, c1, true);
        }
        let smooth = dilate_cross(&ridge).not();
        (ridge, smooth, f)
    }

    /// `_with_body`: a body of L* `colour` at rows 10-29, columns 36-39, joined
    /// to the stroke's right end (its end ridge gone, the body's edges ridge).
    fn with_body(colour: f64) -> (Mask, Mask, Image) {
        let (mut ridge, _, mut f) = scene(5, 36, 20.0, 95.0, 95.0);
        for r in 10..30 {
            for c in 36..40 {
                f.px_mut(r * 40 + c)[0] = colour;
            }
        }
        for r in 17..22 {
            ridge.set(r, 36, false);
        }
        for c in 35..40 {
            ridge.set(9, c, true);
            ridge.set(30, c, true);
        }
        for r in (10..17).chain(22..30) {
            ridge.set(r, 35, true);
        }
        let smooth = dilate_cross(&ridge).not();
        (ridge, smooth, f)
    }

    /// `_trimmed`: what is left of `scene(5, 36, …)`'s stroke (rows row0..row0+2,
    /// in an image `h` rows tall) once it stops `STROKE_SEED_W` short of its
    /// ends. The end pixels are the first and last columns and the corner pixels
    /// one column in on the outer rows (narrowest across a diagonal, so their
    /// stroke runs out along the other diagonal); 5 steps from them the middle
    /// row loses columns 5..10, the outer rows 5..11, and mirrored at the right.
    fn trimmed(h: usize, row0: usize) -> Mask {
        let mut m = Grid::filled(h, 40, false);
        for c in 5 + STROKE_SEED_W + 1..36 - STROKE_SEED_W - 1 {
            m.set(row0 + 1, c, true);
        }
        for c in 5 + STROKE_SEED_W + 2..36 - STROKE_SEED_W - 2 {
            m.set(row0, c, true);
            m.set(row0 + 2, c, true);
        }
        m
    }

    #[test]
    fn a_three_pixel_stroke_is_a_stroke_seed() {
        let (ridge, smooth, f) = scene(5, 36, 20.0, 95.0, 95.0);
        let s = stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR);
        assert!((18..21).all(|r| (12..29).all(|c| *s.get(r, c))));
    }

    #[test]
    fn a_stroke_seed_stops_short_of_its_ends() {
        // row 19 keeps columns 11..29 (19 px), rows 18 and 20 keep 12..28 (17 each)
        let (ridge, smooth, f) = scene(5, 36, 20.0, 95.0, 95.0);
        let s = stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR);
        assert_eq!(s.data, trimmed(40, 18).data);
        assert_eq!(s.count(), 19 + 2 * 17);
    }

    #[test]
    fn a_faint_stroke_is_declined() {
        // 8 levels from its ground: over detail (6), under twice it (12)
        let (ridge, smooth, f) = scene(5, 36, 87.0, 95.0, 95.0);
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).count(), 0);
        let (ridge, smooth, f) = scene(5, 36, 80.0, 95.0, 95.0);
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).data, trimmed(40, 18).data);
    }

    #[test]
    fn a_transparent_seam_is_not_a_stroke() {
        let (ridge, smooth, mut f) = scene(5, 36, 20.0, 95.0, 95.0);
        for i in 0..40 * 40 {
            f.px_mut(i)[3] = 0.0;
        }
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).count(), 0);
        // an opaque stroke on a transparent ground is a stroke
        for r in 18..21 {
            for c in 5..36 {
                f.px_mut(r * 40 + c)[3] = 100.0;
            }
        }
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).data, trimmed(40, 18).data);
    }

    /// `_square_ring`: a 3 px square annulus (outer rows/cols 5..34, counter
    /// 8..31) of L* 20 on a 95 ground, ridge round it on the ground and on the
    /// counter's outer layer, seed everything more than a pixel from a ridge.
    fn square_ring() -> (Mask, Mask, Image, Mask) {
        let (h, w) = (40, 40);
        let inside = |r: usize, c: usize, a: usize, b: usize| (a..b).contains(&r) && (a..b).contains(&c);
        let mut ink = Grid::filled(h, w, false);
        let mut ridge = Grid::filled(h, w, false);
        let mut f = Image::new(h, w, 4);
        for r in 0..h {
            for c in 0..w {
                let band = inside(r, c, 5, 35) && !inside(r, c, 8, 32);
                ink.set(r, c, band);
                ridge.set(r, c, (inside(r, c, 4, 36) && !inside(r, c, 5, 35)) || (inside(r, c, 8, 32) && !inside(r, c, 9, 31)));
                let px = f.px_mut(r * w + c);
                px[0] = if band { 20.0 } else { 95.0 };
                px[3] = 100.0;
            }
        }
        let smooth = dilate_cross(&ridge).not();
        (ridge, smooth, f, ink)
    }

    #[test]
    fn a_square_ring_is_trimmed_only_at_its_corners() {
        // `test_a_square_ring_is_trimmed_only_at_its_corners`: the outer corner
        // pixel and the two beside it are end pixels (narrowest across the
        // corner), and the 27 annulus pixels within 5 steps of them go
        let (ridge, smooth, f, ink) = square_ring();
        let s = stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR);
        for (cy, cx) in [(5usize, 5usize), (5, 34), (34, 5), (34, 34)] {
            let (sy, sx): (isize, isize) = (if cy == 5 { 1 } else { -1 }, if cx == 5 { 1 } else { -1 });
            let ends = [(cy as isize, cx as isize), (cy as isize, cx as isize + sx), (cy as isize + sy, cx as isize)];
            let mut n = 0;
            for r in 0..40 {
                for c in 0..40 {
                    let near = ends.iter().any(|(y, x)| (r as isize - y).abs() + (c as isize - x).abs() <= STROKE_SEED_W as isize);
                    if near && *ink.get(r, c) {
                        n += 1;
                        assert!(!*s.get(r, c), "({r}, {c}) is within 5 steps of a corner's end pixel");
                    }
                }
            }
            assert_eq!(n, 27);
        }
        assert!((0..40 * 40).all(|i| !s.data[i] || ink.data[i]));
        assert_eq!(s.count(), ink.count() - 4 * 27);
        assert!((12..28).all(|k| (5..8).all(|j| *s.get(j, k) && *s.get(k, j) && *s.get(39 - j, k) && *s.get(k, 39 - j))));
    }

    #[test]
    fn a_short_stroke_and_a_two_ground_band_are_not() {
        let (ridge, smooth, f) = scene(5, 11, 20.0, 95.0, 95.0);
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).count(), 0);
        let (ridge, smooth, f) = scene(5, 36, 20.0, 95.0, 50.0);
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).count(), 0);
    }

    #[test]
    fn a_stroke_touching_a_seed_of_its_colour_is_left_to_it() {
        let (ridge, smooth, f) = with_body(20.0);
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).count(), 0);
        let (ridge, smooth, f) = with_body(60.0);
        let s = stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR);
        assert!((18..21).all(|r| (12..29).all(|c| *s.get(r, c))));
        assert!((0..40).all(|r| (0..11).all(|c| !*s.get(r, c))), "seeded, short of its ends");
    }
    /// Rows `r0..` of a scene, as `test_the_beyond_sample_off_the_image_counts_as_ridge`
    /// slices them, with `pad` extra rows on top (ground L*, no ridge).
    fn crop_rows(ridge: &Mask, f: &Image, r0: usize, pad: usize) -> (Mask, Mask, Image) {
        let w = ridge.w;
        let h = ridge.h - r0 + pad;
        let mut r = Grid::filled(h, w, false);
        let mut im = Image::new(h, w, f.c);
        for rr in 0..h {
            let src = if rr < pad { r0 } else { r0 + rr - pad };
            for c in 0..w {
                if rr >= pad {
                    r.set(rr, c, *ridge.get(src, c));
                }
                let px = im.px_mut(rr * w + c);
                px.copy_from_slice(f.px(src * w + c));
                if rr < pad {
                    px[0] = 95.0;
                }
            }
        }
        let s = dilate_cross(&r).not();
        (r, s, im)
    }

    #[test]
    fn a_ridge_on_the_image_edge_has_no_sample_beyond_it() {
        let (ridge, _, f) = scene(5, 36, 20.0, 95.0, 95.0);
        // the ridge is the image's top row: the sample beyond it is off the image, so no seed
        let (r, s, im) = crop_rows(&ridge, &f, 17, 0);
        assert!(r.data[..40].iter().any(|b| *b));
        assert_eq!(stroke_seeds(&r, &s, &im, 6.0, FLOOR).count(), 0);
        // one ground row above the ridge: the same stroke has a sample beyond it and is seeded
        let (r, s, im) = crop_rows(&ridge, &f, 17, 1);
        assert!(r.data[40..80].iter().any(|b| *b) && !r.data[..40].iter().any(|b| *b));
        let out = stroke_seeds(&r, &s, &im, 6.0, FLOOR);
        assert_eq!(out.data, trimmed(24, 2).data);
    }

    #[test]
    fn a_tiny_image_has_no_stroke_seeds() {
        let mut f = Image::new(4, 4, 4);
        for i in 0..16 {
            f.px_mut(i)[3] = 100.0;
        }
        let mut ridge = Grid::filled(4, 4, false);
        for c in 0..4 {
            ridge.set(1, c, true);
        }
        let smooth = ridge.not();
        assert_eq!(stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR).count(), 0);
    }
}
