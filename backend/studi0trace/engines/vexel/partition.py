"""Stage 2: discontinuity map and the initial, edge-bounded partition.

A discontinuity is a *ridge* of the colour gradient, not merely a large
gradient: a steep but smooth ramp (a strong linear gradient) has a large,
flat gradient magnitude and must stay one region, while a hard edge has a
peak. Non-maximum suppression along the local gradient orientation separates
the two, exactly as in Canny.

Seeds for the watershed are the pixels at least one pixel away from any ridge
(dilating the ridge closes the one-pixel gaps NMS leaves at junctions, which
would otherwise let neighbouring regions leak into one seed) **plus** valley
pixels — local gradient minima between two ridges — so that a two-pixel
stroke keeps its own seed instead of being flooded by its surroundings.
"""
from __future__ import annotations

import numpy as np
from scipy import ndimage
from skimage.filters import scharr, scharr_h, scharr_v
from skimage.measure import label as cc_label
from skimage.segmentation import relabel_sequential, watershed

from studi0trace.engines.vexel import dump

_CROSS = ndimage.generate_binary_structure(2, 1)


def discontinuity(features: np.ndarray, sigma: float = 0.7) -> np.ndarray:
    """Per-pixel colour change (ΔE-like units per pixel) across all feature channels."""
    g2 = np.zeros(features.shape[:2], dtype=np.float32)
    for c in range(features.shape[2]):
        g = scharr(features[..., c])
        g2 += g * g
    grad = np.sqrt(g2)
    return ndimage.gaussian_filter(grad, sigma) if sigma > 0 else grad


# The discontinuity floor an edge must stand above is local, not one number
# for every image: `g_low` (1.5) was set for JPEG, whose ringing and block
# edges put 0.5-1.5 ΔE/px of noise inside every smooth area, and it hid every
# crisp step under 2.7 ΔE in a clean render (sixteen tiles 2.9 ΔE apart came
# out as two shapes, their ridge of 1.6 sitting at the threshold and their
# junctions leaking). So where the area is clean outright — the median
# discontinuity over the `NOISE_WINDOW` around the pixel, leaving out the
# pixels within two of any ridge found at `g_low`, is under `CLEAN_FLOOR`, a
# tenth of a level — the ridge threshold is `G_LOW_MIN` and the junction
# test's `G_SEED_MIN`; everywhere else, and under transparent pixels whose
# inpainted colour has seams, they stay `g_low` and `g_seed`. The ridges are
# left out because a window astride two crossing edges is not noisy, it is a
# junction, and read whole its median (0.3-0.4 on the tiles) hid exactly the
# junctions that leaked. A floor scaled by the median instead (three times
# it) let JPEG's ringing, whose median is low and whose peaks are high (0.27
# and 1.45 at the 90th percentile inside a q75 disc), grow ridges that cut
# the disc's seed to pieces.
NOISE_WINDOW = 9
CLEAN_FLOOR = 0.1
G_LOW_MIN = 0.75
G_SEED_MIN = 1.5


def noise_floor(grad: np.ndarray, features: np.ndarray, g_low: float) -> np.ndarray:
    """The local discontinuity floor: the lower-middle median of `grad` over
    the NOISE_WINDOW round each pixel, leaving out the pixels within two of a
    ridge found at `g_low`; `g_low` where the window has nothing else."""
    from numpy.lib.stride_tricks import sliding_window_view

    ridge, _ = ridges_and_valleys(features, grad, g_low, np.full(grad.shape, np.inf, np.float32))
    band = ndimage.binary_dilation(ridge, _CROSS, iterations=2)
    half = NOISE_WINDOW // 2
    values = np.where(band, np.float32(np.inf), grad.astype(np.float32))
    win = sliding_window_view(np.pad(values, half, mode="edge"), (NOISE_WINDOW, NOISE_WINDOW))
    win = win.reshape(grad.shape[0], grad.shape[1], NOISE_WINDOW * NOISE_WINDOW)
    count = sliding_window_view(np.pad((~band).astype(np.int32), half, mode="edge"), (NOISE_WINDOW, NOISE_WINDOW))
    count = count.reshape(grad.shape[0], grad.shape[1], NOISE_WINDOW * NOISE_WINDOW).sum(axis=2)
    median = np.take_along_axis(np.sort(win, axis=2), (np.maximum(count - 1, 0) // 2)[..., None], axis=2)[..., 0]
    return np.where(count > 0, median, np.float32(g_low)).astype(np.float32)


def threshold_at(g_low: float, floor_min: float, floor: np.ndarray, features: np.ndarray) -> np.ndarray:
    """The per-pixel threshold: `floor_min` where the area is clean (its floor
    under CLEAN_FLOOR and the pixel not transparent), `g_low` elsewhere."""
    clean = (features[..., 3] >= 50.0) & (floor < CLEAN_FLOOR)
    return np.where(clean, np.float32(floor_min), np.float32(g_low)).astype(np.float32)


def ridges_and_valleys(features: np.ndarray, grad: np.ndarray, g_low: float,
                       floor: np.ndarray | None = None) -> tuple[np.ndarray, np.ndarray]:
    """(ridge, valley) boolean masks from non-maximum / non-minimum tests along
    the structure-tensor gradient orientation. `floor` is `noise_floor(grad)`;
    the ridge threshold is `g_low` where the floor is high and drops towards
    `G_LOW_MIN` where the image is clean."""
    if floor is None:
        floor = noise_floor(grad, features, g_low)
    g_low_eff = threshold_at(g_low, G_LOW_MIN, floor, features)
    jxx = np.zeros(grad.shape, np.float32)
    jyy = np.zeros(grad.shape, np.float32)
    jxy = np.zeros(grad.shape, np.float32)
    for c in range(features.shape[2]):
        gy = scharr_h(features[..., c])  # derivative along rows (y)
        gx = scharr_v(features[..., c])  # derivative along columns (x)
        jxx += gx * gx
        jyy += gy * gy
        jxy += gx * gy
    for j in (jxx, jyy, jxy):
        j[...] = ndimage.gaussian_filter(j, 1.0)
    theta = 0.5 * np.arctan2(2 * jxy, jxx - jyy)  # dominant gradient direction
    dx, dy = np.cos(theta), np.sin(theta)
    rows, cols = np.mgrid[0 : grad.shape[0], 0 : grad.shape[1]].astype(np.float32)
    g_plus = ndimage.map_coordinates(grad, [rows + dy, cols + dx], order=1, mode="nearest")
    g_minus = ndimage.map_coordinates(grad, [rows - dy, cols - dx], order=1, mode="nearest")
    # Prominence is judged 1.5 px out: a real edge (smoothed peak) has fallen to
    # ≈ 40 % there, while the periodic bumps that 8-bit quantisation puts on a
    # steep ramp are only a few percent high. Sampling 1.5 px also keeps a ridge
    # whose true peak falls between two pixels from vanishing.
    g_plus_far = ndimage.map_coordinates(grad, [rows + 1.5 * dy, cols + 1.5 * dx], order=1, mode="nearest")
    g_minus_far = ndimage.map_coordinates(grad, [rows - 1.5 * dy, cols - 1.5 * dx], order=1, mode="nearest")
    # Prominent against the near samples (a thin stroke's ridge: its far sample is
    # the stroke's other ridge) or against the far samples (a wide edge whose peak
    # straddles two pixels). Quantisation bumps fail both.
    prominent = (grad > 1.10 * np.maximum(g_plus, g_minus)) | (grad > 1.10 * np.maximum(g_plus_far, g_minus_far))
    ridge = (grad > g_low_eff) & (grad >= g_plus) & (grad >= g_minus) & prominent
    valley = (grad <= g_plus) & (grad <= g_minus) & ~ridge
    return ridge, valley


def edge_mask(features: np.ndarray, grad: np.ndarray, g_low: float) -> np.ndarray:
    """Ridge pixels dilated by one (closes NMS gaps). Kept for callers that want the edge band."""
    ridge, _ = ridges_and_valleys(features, grad, g_low)
    return ndimage.binary_dilation(ridge, _CROSS)


def seed_parts(features: np.ndarray, grad: np.ndarray, g_low: float, g_seed: float = 8.0) -> tuple[np.ndarray, np.ndarray]:
    """(smooth, ridge): the seed mask and the ridge mask it was cut by.

    Seeds: pixels clear of the ridge band, or valley pixels, that also have a
    low gradient. NMS loses ridge pixels at T-junctions (where two edges meet the
    orientation estimate flips); those gap pixels still carry a high gradient, so
    the gradient test keeps neighbouring regions from leaking into one seed."""
    floor = noise_floor(grad, features, g_low)
    ridge, valley = ridges_and_valleys(features, grad, g_low, floor)
    band = ndimage.binary_dilation(ridge, _CROSS)
    # Junction gaps sit right next to ridge segments, so the gradient test is only
    # applied within two pixels of the band; a steep but smooth ramp far from any
    # ridge keeps its seeds. Valleys (minima across the edge direction) can never
    # be edge pixels and always seed - that is what keeps thin stroke cores alive.
    near_band = ndimage.binary_dilation(band, _CROSS, iterations=2)
    g_seed_eff = threshold_at(g_seed, G_SEED_MIN, floor, features)
    leaky = near_band & (grad >= g_seed_eff)
    return (~band & ~leaky) | valley, ridge


def seed_mask(features: np.ndarray, grad: np.ndarray, g_low: float, g_seed: float = 8.0) -> np.ndarray:
    """The seed mask alone (`seed_parts`)."""
    return seed_parts(features, grad, g_low, g_seed)[0]


# A 2–4 px stroke loses its seed: the band exclusion above reaches two pixels
# past the ridge on either side, so a stroke narrower than about five pixels
# keeps only fragments of valley, and `seed_markers`' floor zeroes them (6
# under Balanced, 16 under Logo). The watershed then floods the stroke from
# its neighbours: a closed counter's backdrop takes the ring, a bowl joins
# the stem's backdrop. Pooling small fragments and lowering the floor both
# admitted noise everywhere (the wave-lockup plan's try9 and D1); what a thin
# stroke needs is specific to a thin stroke. So after the seed mask, the
# non-ridge pixels that lie between two ridge pixels within `STROKE_SEED_W`
# along one of the four axes — the medial pixels of a thin feature, never the
# halo beside a thick edge, which has a ridge on one side only — are formed
# into 4-connected components, and a component is a stroke seed when it is
# `STROKE_SEED_LEN` long (its pixels over its mean width), one colour (RMS
# spread of its features under `detail`), a different colour from the first
# non-ridge pixel beyond the ridge on both sides by more than `detail` while
# those two sides agree within `detail` (the rim a sharpening leaves inside an
# edge, a JPEG ringing line and a drop shadow's band each lie between a shape
# and its ground, which do not agree), and does not touch a kept seed of its
# own colour, read off the seed's pixels outside the stroke candidates (an acute wedge's tip, the gap between two letters in the ground's
# colour: the flood from that seed is the right answer there). Its pixels join
# the seed mask before the floor, its component is kept whatever its size, and
# its basin is exempt from `_absorb_small`. A 2 px stroke whose two ink
# pixels are both ridge has no medial pixel and is not seeded: a crisp one
# (both pixels peak), and an anti-aliased one at most phases as well (its two
# ink pixels differ from each other by more than `detail`); it is left to the
# rescue, as before.
#
# Three rules keep the seeds to strokes and off their joints:
# - a candidate pixel is at least half opaque (`STROKE_SEED_ALPHA`, on the
#   feature scale, where `threshold_at` reads "clean"). The colour under a
#   transparent pixel is inpainted and means nothing (the seams between
#   inpainted bands on a transparent ground were seeded), and a stroke whose
#   medial pixel is under half covered is thinner than a pixel or translucent.
# - the stroke differs from both sides by more than `STROKE_SEED_CONTRAST`
#   times `detail` (the sides' agreement and the one-colour spread stay at
#   `detail`). JPEG's ringing leaves one-colour streaks of 8–11 ΔE beside an
#   edge in a soft field, long and two-sided; legible text stands far above
#   12 ΔE from its ground.
# - an accepted seed stops `STROKE_SEED_W` short of its ends: every pixel
#   within that many 4-connected steps, inside its component, of an end pixel
#   is removed. An end pixel's hits (positions in its own component) on the
#   ring at Chebyshev distance 2 form at most one circular run, of at most 5
#   positions, and its hits on the ring at distance 3 at most one run
#   (`_ring_runs`): mid-stroke the rings are hit on both sides, at a bend or a
#   junction in separate runs, at an end (or where a stroke ran on into a
#   joint until its width passed STROKE_SEED_W) in one short run. The second
#   ring keeps a bend's outer corner, which looks like a wide stroke's end at
#   distance 2; a ring has no end pixel and stays whole. A stroke's medial
#   pixels run on into the joint with whatever it meets; seeded there, a stem
#   took five rows of its joint with a triangle's base, and the stepped foot
#   read as a wedge tip that walked a sliver round the triangle. Stopped
#   short, the joint is left to the flood, which puts it where it was. A
#   component left with fewer than STROKE_SEED_W pixels is dropped (a speck
#   of seed protected from absorption is worse than none), so a stroke must
#   be about 2·STROKE_SEED_W + STROKE_SEED_W long to keep a seed.
# Spec: 2026-10-09-thin-stroke-seeding-design.md.
STROKE_SEED_W = 5
STROKE_SEED_LEN = 12
STROKE_SEED_ALPHA = 50.0  # a candidate's least opacity, 100·alpha
STROKE_SEED_CONTRAST = 2.0  # a stroke's least difference from either side, in `detail`s
_AXES = ((0, 1), (1, 0), (1, 1), (1, -1))  # (dr, dc): the order ties between equally narrow axes break in


def _ring(radius: int) -> tuple[tuple[int, int], ...]:
    """The positions (dr, dc) at Chebyshev distance `radius`, clockwise from (−r, −r)."""
    r = radius
    return (tuple((-r, c) for c in range(-r, r + 1)) + tuple((y, r) for y in range(-r + 1, r + 1))
            + tuple((r, c) for c in range(r - 1, -r - 1, -1)) + tuple((y, -r) for y in range(r - 1, -r, -1)))


_RING2, _RING3 = _ring(2), _ring(3)


def _ring_runs(comps: np.ndarray, ring) -> tuple[np.ndarray, np.ndarray]:
    """Per pixel: how many positions of `ring` lie in its own component (off the
    image: a miss) and how many circular runs they form (a full ring is one)."""
    hits = [(_shift(comps, 1, dr, dc, 0) == comps) for dr, dc in ring]
    n = np.sum(hits, axis=0)
    starts = np.sum([hits[k] & ~hits[k - 1] for k in range(len(ring))], axis=0)
    return n, starts + (n == len(ring))


def _shift(a: np.ndarray, k: int, dr: int, dc: int, fill) -> np.ndarray:
    """out[r, c] = a[r + k·dr, c + k·dc]; `fill` off the image."""
    h, w = a.shape[:2]
    out = np.full_like(a, fill)
    r0, r1 = max(0, -k * dr), min(h, h - k * dr)
    c0, c1 = max(0, -k * dc), min(w, w - k * dc)
    if r0 < r1 and c0 < c1:
        out[r0:r1, c0:c1] = a[r0 + k * dr:r1 + k * dr, c0 + k * dc:c1 + k * dc]
    return out


def _ridge_reach(ridge: np.ndarray, dr: int, dc: int) -> tuple[np.ndarray, np.ndarray]:
    """Per pixel, along +(dr, dc): the distance 1..W to the nearest ridge pixel
    (0: none within W) and the distance to the first non-ridge pixel on the
    image past that ridge's run, within W more (0: none)."""
    dist = np.zeros(ridge.shape, np.int32)
    for k in range(STROKE_SEED_W, 0, -1):
        dist = np.where(_shift(ridge, k, dr, dc, False), k, dist)
    beyond = np.zeros(ridge.shape, np.int32)
    for k in range(2 * STROKE_SEED_W, 1, -1):
        clear = ~_shift(ridge, k, dr, dc, True)  # off the image counts as ridge: no sample there
        beyond = np.where(clear & (dist > 0) & (k > dist) & (k <= dist + STROKE_SEED_W), k, beyond)
    return dist, beyond


def stroke_seeds(ridge: np.ndarray, smooth: np.ndarray, features: np.ndarray, detail: float, floor: int) -> np.ndarray:
    """The pixels of every stroke seed (the comment above `STROKE_SEED_W`).
    `smooth` is the seed mask, whose components of `floor` pixels or more are
    the kept seeds a stroke must not touch in its own colour."""
    h, w = ridge.shape
    nc = features.shape[-1]
    feats = features.astype(np.float64)
    width = np.zeros((h, w), np.int32)  # the narrowest axis's width; 0: not between two ridges
    rows, cols = np.mgrid[0:h, 0:w]
    out_r = [np.zeros((h, w), np.int64), np.zeros((h, w), np.int64)]  # the samples beyond the + and - ridges
    out_c = [np.zeros((h, w), np.int64), np.zeros((h, w), np.int64)]
    has_out = np.zeros((h, w), bool)
    for dr, dc in _AXES:
        dp, bp = _ridge_reach(ridge, dr, dc)
        dm, bm = _ridge_reach(ridge, -dr, -dc)
        wd = dp + dm - 1
        between = (dp > 0) & (dm > 0) & (wd <= STROKE_SEED_W) & ~ridge
        better = between & ((width == 0) | (wd < width))
        width = np.where(better, wd, width)
        has_out = np.where(better, (bp > 0) & (bm > 0), has_out)
        out_r[0] = np.where(better, rows + bp * dr, out_r[0])
        out_c[0] = np.where(better, cols + bp * dc, out_c[0])
        out_r[1] = np.where(better, rows - bm * dr, out_r[1])
        out_c[1] = np.where(better, cols - bm * dc, out_c[1])
    cand = width > 0
    cand &= features[..., 3] >= STROKE_SEED_ALPHA  # at least half opaque
    if not cand.any():
        return cand
    comps = cc_label(cand, connectivity=1)
    flat = comps.ravel()
    n = int(comps.max()) + 1
    size = np.bincount(flat, minlength=n).astype(np.int64)
    d2 = detail * detail
    # long: pixels over mean width, i.e. size² against the width sum, in integers
    wsum = np.bincount(flat, weights=width.ravel().astype(np.float64), minlength=n).astype(np.int64)
    long_enough = size * size >= STROKE_SEED_LEN * wsum
    # one colour: RMS spread of the features about the component's mean
    sums = np.stack([np.bincount(flat, weights=feats[..., c].ravel(), minlength=n) for c in range(nc)], axis=1)
    mean = sums / np.maximum(size, 1)[:, None]
    dev = feats - mean[comps]
    sq = np.bincount(flat, weights=np.sum(dev * dev, axis=2).ravel(), minlength=n)
    one_colour = sq / np.maximum(size, 1) < d2
    # two-sided: the pixel differs from both samples beyond the ridges by
    # STROKE_SEED_CONTRAST·detail, and they agree within detail
    cd = STROKE_SEED_CONTRAST * detail
    c2 = cd * cd
    sel = cand & has_out
    ys, xs = np.nonzero(sel)
    fp = feats[ys, xs]
    fa = feats[out_r[0][ys, xs], out_c[0][ys, xs]]
    fb = feats[out_r[1][ys, xs], out_c[1][ys, xs]]
    ok = (np.sum((fp - fa) ** 2, axis=1) > c2) & (np.sum((fp - fb) ** 2, axis=1) > c2) & (np.sum((fa - fb) ** 2, axis=1) < d2)
    passing = np.bincount(comps[ys, xs], weights=ok.astype(np.float64), minlength=n).astype(np.int64)
    two_sided = 2 * passing > size
    # not touching a kept seed of its own colour
    kept = cc_label(smooth, connectivity=1)
    kflat = kept.ravel()
    nk = int(kept.max()) + 1
    ksize = np.bincount(kflat, minlength=nk)
    # a kept seed's colour is read off its pixels outside the stroke candidates: a
    # stroke's own valley line is smooth and may join the neighbouring seed in one
    # component, and blending it in would pull that seed's colour toward the stroke's
    own = (smooth & ~cand).ravel()
    kcount = np.bincount(kflat, weights=own.astype(np.float64), minlength=nk).astype(np.int64)
    ksum = np.stack([np.bincount(kflat, weights=np.where(own, feats[..., c].ravel(), 0.0), minlength=nk) for c in range(nc)], axis=1)
    kmean = ksum / np.maximum(kcount, 1)[:, None]
    big = (ksize >= floor) & (kcount > 0)  # a component of the stroke's own pixels alone has no other colour to compare
    big[0] = False
    touching = np.zeros(n, bool)
    pairs = []
    for dr, dc in ((0, 0), (0, 1), (0, -1), (1, 0), (-1, 0)):
        near = _shift(kept, 1, dr, dc, 0) if (dr, dc) != (0, 0) else kept
        m = cand & (near > 0) & big[near]
        pairs.append(comps[m].astype(np.int64) * nk + near[m])
    for key in np.unique(np.concatenate(pairs)):
        comp, k = int(key // nk), int(key % nk)
        d = mean[comp] - kmean[k]
        if float(np.sum(d * d)) < d2:
            touching[comp] = True
    accept = long_enough & one_colour & two_sided & ~touching
    accept[0] = False
    keep = accept[comps]
    # stop short of the ends (the comment above STROKE_SEED_W): the pixels within
    # STROKE_SEED_W four-connected steps, inside the component, of an end pixel go
    n2, runs2 = _ring_runs(comps, _RING2)
    _, runs3 = _ring_runs(comps, _RING3)
    end = keep & ((runs2 == 0) | ((runs2 == 1) & (n2 <= 5))) & (runs3 <= 1)
    trimmed = end
    for _ in range(STROKE_SEED_W):
        trimmed = ndimage.binary_dilation(trimmed, _CROSS, mask=keep)
    seed = keep & ~trimmed
    # a component left with fewer than STROKE_SEED_W pixels is dropped
    left = np.bincount(flat, weights=seed.ravel().astype(np.float64), minlength=n).astype(np.int64)
    return seed & (left >= STROKE_SEED_W)[comps]


def _absorb_small(labels: np.ndarray, grad: np.ndarray, min_region: int, rounds: int = 3,
                  protected: np.ndarray | None = None) -> np.ndarray:
    """Flood regions below `min_region` pixels from their neighbours along low gradient.

    `protected[label]` exempts a label: a stroke seed's basin is a region however small.
    """
    for _ in range(rounds):
        sizes = np.bincount(labels.ravel())
        small = sizes < min_region
        small[0] = False
        if protected is not None:
            small[:protected.size] &= ~protected[:small.size]
        if not small.any():
            break
        markers = labels.copy()
        markers[small[labels]] = 0
        if markers.max() == 0:  # everything was small: keep the largest
            return np.ones_like(labels)
        labels = watershed(grad, markers)
    return labels


# How wide a neck in a seed may be and still be a leak rather than one area:
# the seed mask is eroded this many times (cross), which parts necks up to
# twice as wide, to find the pieces a neck joins. The weaker the edge, the
# further from the strong one its ridge gives out: the collar of a JPEG key
# reached its ring through a neck four pixels wide, and a disc 6 ΔE darker
# than a panel whose edge cuts it, through one of six (the ridge stops eight
# pixels short of the edge, inside the JPEG's ringing along it).
NECK_ERODE = 3
# How far the discontinuity along the boundary between two split pieces must
# stand above the discontinuity inside them for the split to be a step and not
# a ramp (`rejoin_ramps`). On the corpus the steps the split exists for stand
# 15 times and more above their plateaus; the rings of a JPEG glow, the halo
# of a soft logomark and the bands of a JPEG gradient stand at most 2.2 times
# above their own slope.
NECK_PROMINENCE = 3.0
# ... and how much of the colour difference between them it must carry. A step
# of D shows as a ridge of about 0.6·D on a clean edge (`MergeParams.edge_veto`)
# and 0.3–0.4·D where JPEG smears it (0.41 on the moon's craters, 0.30 at the
# least on the leaf's veins). A boundary carrying under a quarter of the
# difference sits on a ramp: the two sides differ by what lies between them.
NECK_STEP = 0.25
# ... and a step is a ridge of the discontinuity: steeper on the boundary than
# beside it. The two tests above compare the boundary with the pieces' middles
# and with their colour difference, and a boundary through a soft band passes
# both — the necks of the nail polish's cap (a ring of seed round a pill
# highlight) lie in the cap's rim band, which is 3 ΔE per pixel steep against
# a flat body, and its two halves, 10 ΔE apart, were seeded apart and drawn
# with a seam through the highlight's ends. So at more than half of the pairs
# along the boundary, the discontinuity must be at least `NECK_RIDGE` times
# the higher of the two `NECK_REACH` pixels to either side of the pair, along
# its axis. A ramp is as steep beside the boundary as on it (1.0 ± noise; the
# cap's necks 0.9), a JPEG-smeared step still peaks (the moon's craters 2.6,
# the key's collar and a heart's outline 1.5), a clean step far more.
NECK_RIDGE = 1.25
NECK_REACH = 3
# Fewest pixels a piece needs, after the erosion, to be seeded apart from the
# rest of its component: a region of its own, not the JPEG ringing a strong
# edge leaves a few pixels inside it (pieces of 8 to 40 pixels along the rim of
# a JPEG radial disc and in the wordmark's ribbon were split off as specks).
NECK_PIECE = 64


def seed_markers(smooth: np.ndarray, floor: int, features: np.ndarray | None = None,
                 detail: float = 6.0, grad: np.ndarray | None = None,
                 strokes: np.ndarray | None = None) -> tuple[np.ndarray, np.ndarray]:
    """The watershed's markers: the seed mask's connected components of at least
    `floor` pixels, each split where a neck joins areas of different colour.
    `strokes` (`stroke_seeds`) joins the mask, and a component holding a stroke
    pixel is kept whatever its size.

    A component is one basin, so a strip of seed across a gap in a ridge makes
    the two sides one region for good: nothing downstream splits a region. The
    gaps are where a weak edge meets a strong one. NMS runs along the strong
    edge's orientation there and drops the weak ridge for a few pixels, and the
    junction test in `seed_mask` (`g_seed`) sits above a weak edge's own
    gradient. JPEG makes it common: its chroma is subsampled and smeared, so a
    colour step a few pixels from a strong edge is a shoulder on that edge's
    ridge, not a ridge of its own. A moon's craters, a key's collar and a
    leaf's veins each shared one seed with the fill around them and were drawn
    as one gradient, and a tile's bevel did the same on a clean render.

    So each component is eroded `NECK_ERODE` times and broken into the pieces
    the erosion leaves (`floor` pixels or more). The pieces are grouped by
    colour, largest first: a piece joins the first group whose first piece's
    mean feature colour is nearer than `detail` — the merge's own threshold
    for two flat areas — and starts a group otherwise, if it has `NECK_PIECE`
    pixels (a smaller one is left to the flood). A component that comes
    out as one group is seeded whole, exactly as before: its pieces are one
    smooth area pinched by noise (JPEG's blocks pinch the seed along every
    diagonal edge into a row of cells). One that comes out as several is
    seeded by its groups, each grown back over the component's own seed
    (a watershed on `grad` inside it), so only the neck is shared out and a
    thin strip of the seed stays with the area it belongs to: a strip of the
    moon's rim between a crater and the limb, left to the flood, went to the
    crater and cut the limb into arcs. `rejoin_ramps` then joins the groups
    again wherever no step divides them.
    Colour sums run in raster order in float64, so both engines group alike.

    Returns (markers, origin): int32 markers numbered in raster order of their
    first pixel, and per marker id the seed component it was split from (0 for
    a component seeded whole).
    """
    comps = cc_label(smooth if strokes is None else (smooth | strokes), connectivity=1)
    if comps.max() == 0:
        return comps.astype(np.int32), np.zeros(1, np.int64)
    sizes = np.bincount(comps.ravel())
    keep = sizes >= floor
    if strokes is not None:
        keep[np.unique(comps[strokes])] = True  # a stroke seed is kept whatever its area
    keep[0] = False
    comps[~keep[comps]] = 0
    if features is None:
        markers = cc_label(comps > 0, connectivity=1).astype(np.int32)
        return markers, np.zeros(int(markers.max()) + 1, np.int64)
    core = ndimage.binary_erosion(comps > 0, _CROSS, iterations=NECK_ERODE, border_value=1)
    pieces = cc_label(core, connectivity=1)
    psize = np.bincount(pieces.ravel())
    n_p = psize.size
    big = psize >= floor
    big[0] = False
    owner = np.zeros(n_p, np.int64)  # the component each piece lies in
    owner[pieces.ravel()] = comps.ravel()
    count = np.bincount(owner[big], minlength=sizes.size)
    group = np.zeros(n_p, np.int64)  # piece -> marker id (0: not a marker)
    out = comps.astype(np.int64)
    split = np.zeros(sizes.size, bool)
    cands = np.nonzero(count >= 2)[0]
    if cands.size:
        flat = pieces.ravel()
        sums = np.stack([np.bincount(flat, weights=features[..., c].ravel().astype(np.float64), minlength=n_p)
                         for c in range(features.shape[-1])], axis=1)
        mean = sums / np.maximum(psize, 1)[:, None]
        next_id = int(sizes.size)
        by_comp: dict[int, list[int]] = {}
        for p in np.nonzero(big)[0]:
            by_comp.setdefault(int(owner[p]), []).append(int(p))
        for c in cands:
            ps = sorted(by_comp[int(c)], key=lambda p: (-int(psize[p]), p))
            anchors: list[int] = []
            ids: list[int] = []
            for p in ps:
                for a, gid in zip(anchors, ids):
                    d = mean[p] - mean[a]
                    if float(np.sum(d * d)) < detail * detail:
                        group[p] = gid
                        break
                else:
                    if not anchors or psize[p] >= NECK_PIECE:
                        anchors.append(p)
                        ids.append(next_id)
                        group[p] = next_id
                        next_id += 1
            if len(anchors) >= 2:
                split[c] = True
            else:
                group[ps] = 0
        if split.any():
            inside = split[comps]
            g = grad if grad is not None else np.zeros(smooth.shape, np.float32)
            grown = watershed(g, np.where(inside, group[pieces], 0), mask=inside)
            out[inside] = grown[inside]
    markers = _raster_order(out)
    origin = np.zeros(int(markers.max()) + 1, np.int64)
    origin[markers.ravel()] = np.where(split[comps.ravel()], comps.ravel(), 0)
    origin[0] = 0
    return markers, origin


def _raster_order(ids: np.ndarray) -> np.ndarray:
    """Renumber the non-zero ids 1..K in raster order of each id's first pixel."""
    flat = ids.ravel()
    uniq, first = np.unique(flat, return_index=True)
    keep = uniq != 0
    uniq, first = uniq[keep], first[keep]
    lut = np.zeros(int(flat.max()) + 1, np.int32)
    lut[uniq[np.argsort(first, kind="stable")]] = np.arange(1, uniq.size + 1, dtype=np.int32)
    return lut[ids].astype(np.int32)


def rejoin_ramps(labels: np.ndarray, markers: np.ndarray, origin: np.ndarray, grad: np.ndarray,
                 features: np.ndarray) -> np.ndarray:
    """Join again the basins of a split seed that no step divides.

    A neck in a seed is as often a smooth area pinched by noise as a leak: the
    rings of a JPEG glow and the halo round a soft logomark are pinched into
    pieces whose colours are far apart only because the ramp between them is
    steep. What tells a step from a ramp is where the colour changes: across a
    step it changes along the boundary between the two, so the discontinuity
    there stands far above that inside either piece; across a ramp it changes
    everywhere, and the boundary is no steeper than the pieces. So two
    adjacent basins split from one component stay apart only when the mean
    discontinuity along their shared boundary (4-neighbour pixel pairs, each
    the mean of its two pixels) is at least `NECK_PROMINENCE` times the median
    discontinuity over the marker of either (its lower middle value, so both
    engines take the same element), and at least `NECK_STEP` times the
    distance between the two markers' mean feature colours, and at more than
    half of its pairs at least `NECK_RIDGE` times the discontinuity
    `NECK_REACH` pixels to either side (the higher of the two, read along the
    pair's axis and held inside the image): a step is a ridge, and a boundary
    through a soft band is as steep beside itself as on itself. The rest are
    joined, and a join is the union of the two basins: the flood is blind to
    labels, so it is what seeding them as one marker would have given. Sums
    run horizontal pairs then vertical ones, each in raster order, in float64.
    """
    if not origin.any():
        return labels
    k = int(origin.size)
    g = grad.astype(np.float64)
    # the interior discontinuity of each split marker
    interior = np.zeros(k)
    flat_m = markers.ravel().astype(np.int64)
    sel = origin[flat_m] > 0
    ms, gs = flat_m[sel], g.ravel()[sel]
    order = np.lexsort((gs, ms))  # by marker, then by value
    ms, gs = ms[order], gs[order]
    ids, first, n = np.unique(ms, return_index=True, return_counts=True)
    interior[ids] = gs[first + (n - 1) // 2]
    count = np.bincount(flat_m, minlength=k)
    colour = np.stack([np.bincount(flat_m, weights=features[..., c].ravel().astype(np.float64), minlength=k)
                       for c in range(features.shape[-1])], axis=1) / np.maximum(count, 1)[:, None]
    h, w = labels.shape
    keys, weights, ridges = [], [], []
    for axis in (1, 0):
        if axis == 1:
            la, lb, ga, gb = labels[:, :-1], labels[:, 1:], g[:, :-1], g[:, 1:]
        else:
            la, lb, ga, gb = labels[:-1, :], labels[1:, :], g[:-1, :], g[1:, :]
        a, b = la.ravel().astype(np.int64), lb.ravel().astype(np.int64)
        m = (a != b) & (origin[a] > 0) & (origin[a] == origin[b])
        a, b = a[m], b[m]
        keys.append(np.minimum(a, b) * k + np.maximum(a, b))
        centre = 0.5 * (ga.ravel()[m] + gb.ravel()[m])
        weights.append(centre)
        # the discontinuity NECK_REACH pixels before the pair and after it
        ys, xs = np.nonzero(m.reshape(la.shape))
        if axis == 1:
            before = g[ys, np.maximum(xs - NECK_REACH, 0)]
            after = g[ys, np.minimum(xs + 1 + NECK_REACH, w - 1)]
        else:
            before = g[np.maximum(ys - NECK_REACH, 0), xs]
            after = g[np.minimum(ys + 1 + NECK_REACH, h - 1), xs]
        ridges.append((centre >= NECK_RIDGE * np.maximum(before, after)).astype(np.float64))
    key = np.concatenate(keys)
    if key.size == 0:
        return labels
    uniq, inv = np.unique(key, return_inverse=True)
    cnt = np.bincount(inv)
    gsum = np.bincount(inv, weights=np.concatenate(weights))
    ridge = np.bincount(inv, weights=np.concatenate(ridges))
    parent = np.arange(k)

    def find(i: int) -> int:
        while parent[i] != i:
            i = int(parent[i])
        return i

    for u, c, s, r in zip(uniq, cnt, gsum, ridge):
        a, b = int(u // k), int(u % k)
        edge = s / c
        d = colour[a] - colour[b]
        if (edge >= NECK_PROMINENCE * max(interior[a], interior[b]) and edge >= NECK_STEP * float(np.sqrt(np.sum(d * d)))
                and 2.0 * r > c):
            continue
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[max(ra, rb)] = min(ra, rb)
    root = np.array([find(i) for i in range(k)])
    return root[labels].astype(np.int32)


def initial_labels(grad: np.ndarray, features: np.ndarray | None = None, min_region: int = 6, g_low: float = 1.5,
                   detail: float = 6.0) -> np.ndarray:
    """Watershed on the discontinuity map, seeded by the connected smooth areas.

    With `features` the seeds come from ridge/valley analysis (steep smooth ramps
    and thin strokes keep their seeds) and are split at necks between areas more
    than `detail` apart (`seed_markers`); without them the legacy `grad < g_low`
    threshold is used. Stroke seeds (`stroke_seeds`) join the seed mask, are kept
    whatever their size and are exempt from the small-region absorb. Returns
    int32 labels 1..K covering every pixel.
    """
    if features is not None:
        smooth, ridge = seed_parts(features, grad, g_low)
        strokes = stroke_seeds(ridge, smooth, features, detail, max(min_region, 2))
    else:
        smooth, strokes = grad < g_low, None
    if dump.enabled():
        dump.labels("seed_mask", smooth.astype(np.int32))
        if strokes is not None:
            dump.labels("stroke_seeds", strokes.astype(np.int32))
    markers, origin = seed_markers(smooth, max(min_region, 2), features, detail, grad, strokes)
    if dump.enabled():
        dump.labels("seed_markers", markers)
    protected = None
    if strokes is not None and strokes.any() and markers.max() > 0:
        protected = np.zeros(int(markers.max()) + 1, bool)
        protected[np.unique(markers[strokes])] = True
        protected[0] = False
    if markers.max() == 0:
        # No smooth area at all (tiny or extremely noisy image): seed from the
        # local minima of the gradient instead so watershed still has basins.
        minima = ndimage.minimum_filter(grad, size=3) == grad
        markers = cc_label(minima, connectivity=1)
        if markers.max() == 0:
            return np.ones(grad.shape, dtype=np.int32)
        origin = np.zeros(int(markers.max()) + 1, np.int64)
    labels = watershed(grad, markers)
    labels = rejoin_ramps(labels, markers, origin, grad, features)
    labels = _absorb_small(labels, grad, min_region, protected=protected)
    labels, _, _ = relabel_sequential(labels)
    return labels.astype(np.int32)
