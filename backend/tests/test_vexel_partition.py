"""The partition's seeds: a neck in a seed between two colours is two seeds.

A watershed basin per seed component means a strip of seed across a gap in a
ridge makes two areas one region for good. The gaps are where a weak edge meets
a strong one, and JPEG makes them common: the held-out moon's craters, a key's
collar and a leaf's veins were each traced as one gradient with the fill around
them (`partition.seed_markers`).
"""
from __future__ import annotations

import io
from pathlib import Path

import numpy as np
import pytest
import resvg_py
from PIL import Image
from scipy import ndimage
from skimage.color import lab2rgb, rgb2lab
from skimage.measure import label as cc_label

from studi0trace.engines.vexel import engine as vexel
from studi0trace.engines.vexel.engine import VexelEngine, VexelParams
from studi0trace.engines.vexel.partition import discontinuity, initial_labels, seed_markers, seed_mask, seed_parts, stroke_seeds
from studi0trace.engines.vexel.prepare import prepare
from studi0trace.imaging.intake import load_upload

LIMITS = dict(max_bytes=1 << 30, max_pixels=1 << 30)
HELDOUT = Path(__file__).resolve().parents[1] / "bench" / "heldout" / "noto"


def render(svg: str, w: int, h: int) -> np.ndarray:
    png = resvg_py.svg_to_bytes(svg_string=svg, width=w, height=h)
    return np.asarray(Image.open(io.BytesIO(bytes(png))).convert("RGBA")).astype(float)


def trace(data: bytes) -> str:
    return VexelEngine().trace(load_upload(data, **LIMITS), VexelParams()).svg


def _necked(right_l: float) -> tuple[np.ndarray, np.ndarray]:
    """Two 20 px blocks of seed joined by a corridor two pixels wide; the right
    block's L* is `right_l`, the left one's 50."""
    smooth = np.zeros((30, 56), bool)
    smooth[5:25, 3:23] = True
    smooth[14:16, 23:33] = True
    smooth[5:25, 33:53] = True
    feats = np.zeros((30, 56, 4), np.float32)
    feats[..., 0] = 50.0
    feats[:, 28:, 0] = right_l
    feats[..., 3] = 100.0
    return smooth, feats


def test_a_neck_between_two_colours_splits_the_seed():
    smooth, feats = _necked(60.0)
    m, origin = seed_markers(smooth, 6, feats, detail=6.0)
    assert m.max() == 2 and origin[1] == origin[2] == 1
    assert m[15, 12] != m[15, 42] and m[15, 12] > 0 and m[15, 42] > 0
    assert np.array_equal(m > 0, smooth), "the whole seed is shared out between the two"
    assert (m[5:25, 3:23] == m[15, 12]).all() and (m[5:25, 33:53] == m[15, 42]).all()


def test_a_neck_within_one_colour_leaves_the_seed_whole():
    smooth, feats = _necked(53.0)  # nearer than `detail`
    m, origin = seed_markers(smooth, 6, feats, detail=6.0)
    assert np.array_equal(m, cc_label(smooth, connectivity=1)), "unsplit, the markers are the components as before"
    assert not origin.any()


def _disc_cut_by_an_edge(de: float) -> tuple[bytes, np.ndarray, np.ndarray]:
    """A disc `de` ΔE darker than the linear gradient it sits in, cut by the
    panel's edge against a dark field, saved as JPEG q75. Returns the JPEG and
    masks of the disc's core and of a ring of gradient round it, below the edge."""
    size, cx, cy, r, edge = 256, 150.0, 120.0, 36.0, 100.0
    ys, xs = np.mgrid[0:size, 0:size] + 0.5
    t = (xs + 0.4 * ys) / (size * 1.4)
    lab = rgb2lab((np.array([200, 150, 90]) * (1 - t[..., None]) + np.array([240, 215, 160]) * t[..., None]) / 255.0)
    disc = np.zeros((size, size))
    panel = np.zeros((size, size))
    for oy in range(4):
        for ox in range(4):
            px, py = xs - 0.5 + (ox + 0.5) / 4, ys - 0.5 + (oy + 0.5) / 4
            disc += np.hypot(px - cx, py - cy) <= r
            panel += py >= edge
    disc, panel = disc / 16, panel / 16
    dark = lab.copy()
    dark[..., 0] -= de
    rgb = lab2rgb(lab) * (1 - disc[..., None]) + lab2rgb(dark) * disc[..., None]
    rgb = rgb * panel[..., None] + np.array([40, 40, 60]) / 255.0 * (1 - panel[..., None])
    buf = io.BytesIO()
    Image.fromarray(np.clip(np.round(rgb * 255), 0, 255).astype(np.uint8)).save(buf, "JPEG", quality=75)
    d = np.hypot(xs - cx, ys - cy)
    below = ys > edge + 5
    return buf.getvalue(), (d < r - 4) & below, (d > r + 4) & (d < r + 10) & below


@pytest.mark.parametrize("de", [6.0, 8.0, 10.0])
def test_a_low_contrast_disc_cut_by_a_strong_edge_survives_jpeg(de):
    """The disc's ridge gives out a few pixels short of the panel's edge (NMS
    runs along the strong edge there), so its seed and the gradient's were one
    and the disc was drawn as part of the ramp: its step came out at 16 %."""
    data, core, ring = _disc_cut_by_an_edge(de)
    src = np.asarray(Image.open(io.BytesIO(data)).convert("RGB")).astype(float)
    out = render(trace(data), 256, 256)[..., :3]
    step_src = src[ring].mean(0) - src[core].mean(0)
    step_out = out[ring].mean(0) - out[core].mean(0)
    assert np.linalg.norm(step_out) > 0.8 * np.linalg.norm(step_src)
    assert np.abs(out[core] - src[core]).mean() < 3.0


@pytest.mark.skipif(not HELDOUT.exists(), reason="held-out corpus not present")
def test_moon_craters_survive_jpeg():
    """The held-out moon at JPEG q75 (the upper-left of it: the big crater and
    the limb it runs along). The crater's edge six pixels inside the limb is a
    shoulder on the limb's smeared chroma ridge, so its seed ran along the limb
    into the body's: one region, one gradient, the crater gone (outline error
    11 px on the whole image, every other engine under 0.9)."""
    jpg = np.asarray(Image.open(HELDOUT / "u1f315-512-q75.jpg").convert("RGBA"))[0:384, 0:256]
    clean = np.asarray(Image.open(HELDOUT / "u1f315-512.png").convert("RGBA")).astype(float)[0:384, 0:256]
    buf = io.BytesIO()
    Image.fromarray(jpg).save(buf, "PNG")
    out = render(trace(buf.getvalue()), 256, 384)
    opaque = clean[..., 3] > 250
    crater = ndimage.binary_erosion(opaque & (np.linalg.norm(clean[..., :3] - [241, 198, 136], axis=-1) < 4), iterations=4)
    body = ndimage.binary_erosion(opaque & (np.linalg.norm(clean[..., :3] - [252, 229, 172], axis=-1) < 4), iterations=4)
    src = jpg.astype(float)
    step_src = src[body][:, :3].mean(0) - src[crater][:, :3].mean(0)
    step_out = out[body][:, :3].mean(0) - out[crater][:, :3].mean(0)
    assert np.linalg.norm(step_out) > 0.9 * np.linalg.norm(step_src)
    assert np.abs(out[crater][:, :3] - src[crater][:, :3]).mean() < 2.0


GLOW = Path(__file__).resolve().parents[1] / "bench" / "corpus" / "real" / "shadow" / "glow-512-q75.jpg"


@pytest.mark.skipif(not GLOW.exists(), reason="bench corpus not present")
def test_a_glow_pinched_by_jpeg_noise_stays_one_region():
    """JPEG's blocks pinch the seed of a steep radial glow into rings whose
    colours are far apart; split at those necks the disc came out as a blocky
    core inside a ring (artifact index 11 -> 106). No step divides them — the
    boundary is no steeper than the ramp either side — so `rejoin_ramps` puts
    them back together."""
    a = np.asarray(Image.open(GLOW).convert("RGBA"))
    p = prepare(a)
    labels = initial_labels(discontinuity(p.features), p.features, min_region=6, detail=6.0)
    ys, xs = np.mgrid[0:512, 0:512]
    inside = labels[np.hypot(xs - 256, ys - 256) < 150]
    assert np.bincount(inside).max() > 0.99 * inside.size


TILES = Path(__file__).resolve().parents[1] / "bench" / "corpus" / "synthetic" / "flat" / "low-contrast-512.png"


@pytest.mark.skipif(not TILES.exists(), reason="bench corpus not present")
def test_subtle_crisp_tiles_in_a_clean_render_are_separate_seeds():
    """Sixteen flat tiles 2.9 ΔE apart. Their ridge (1.6) sat at `g_low` and
    NMS drops it where two edges cross, so five seeds leaked across the
    junctions. Where the area is clean — the median discontinuity round the
    pixel, ridges left out, under a tenth of a level — the ridge threshold
    and the junction test drop to `G_LOW_MIN` and `G_SEED_MIN`; JPEG's
    ringing keeps `g_low` (`test_a_low_contrast_disc_cut_by_a_strong_edge_survives_jpeg`)."""
    a = np.asarray(Image.open(TILES).convert("RGBA"))
    p = prepare(a)
    labels = initial_labels(discontinuity(p.features), p.features, min_region=12, detail=6.0)
    assert labels.max() >= 14, labels.max()
    centres = labels[64::128, 64::128]  # one sample per tile
    assert len(np.unique(centres)) >= 14, centres


NAIL = Path(__file__).resolve().parents[1] / "bench" / "heldout" / "fluent-color" / "nail-polish-512.png"


@pytest.mark.skipif(not NAIL.exists(), reason="held-out corpus not present")
def test_a_ring_of_seed_round_a_highlight_is_not_split_at_its_necks():
    """The held-out nail polish's cap: one gradient with a pill highlight in
    the middle, so its seed is a ring whose two necks (above and below the
    highlight) lie in the cap's soft rim band, where the ramp is steeper than
    the flat body. Split there, the cap was two regions whose seam ran into
    the highlight's ends and drew them as cusps. The boundary at the necks is
    no steeper than the pixels beside it — a step is a ridge, a ramp is not."""
    a = np.asarray(Image.open(NAIL).convert("RGBA"))
    p = prepare(a)
    labels = initial_labels(discontinuity(p.features), p.features, min_region=12, detail=6.0)
    body = [labels[y, x] for y, x in ((60, 405), (60, 450), (38, 410), (38, 450), (92, 410), (92, 450))]
    assert len(set(body)) == 1, body


@pytest.mark.skipif(vexel._vexel_rs is None, reason="the vexel_rs extension is not built")
@pytest.mark.parametrize("detail", [3.0, 6.0, 14.0])
def test_the_engines_split_seeds_alike(detail):
    data, _, _ = _disc_cut_by_an_edge(8.0)
    a = np.ascontiguousarray(np.asarray(Image.open(io.BytesIO(data)).convert("RGBA")))
    p = prepare(a)
    py = initial_labels(discontinuity(p.features), p.features, min_region=6, detail=detail)
    rs = np.asarray(vexel._vexel_rs._stage_labels0(a.tobytes(), 256, 256, 6, detail), np.int32).reshape(256, 256)
    assert np.array_equal(py, rs)


# --- stroke seeds ---------------------------------------------------------------


FLOOR = 40  # the kept-seed floor in these scenes: above the stroke's own 31 px valley line, below the ground


def _stroke_scene(cols=(5, 36), ink_l=20.0, above_l=95.0, below_l=95.0, size=(40, 40)):
    """A 3 px horizontal stroke (rows 18–20, columns cols[0]..cols[1]-1) of L*
    `ink_l` with ridge pixels on the rows and columns round it; the ground is
    `above_l` above it and `below_l` below. The seed mask is everything more
    than one pixel from the ridge, as a crisp scene's is: the stroke's middle
    row is seed too (a valley line of 31 px, under FLOOR)."""
    h, w = size
    feats = np.zeros((h, w, 4), np.float32)
    feats[..., 3] = 100.0
    feats[:19, :, 0] = above_l
    feats[19:, :, 0] = below_l
    c0, c1 = cols
    feats[18:21, c0:c1, 0] = ink_l
    ridge = np.zeros((h, w), bool)
    ridge[17, c0 - 1:c1 + 1] = True
    ridge[21, c0 - 1:c1 + 1] = True
    ridge[17:22, c0 - 1] = True
    ridge[17:22, c1] = True
    smooth = ~ndimage.binary_dilation(ridge)
    return ridge, smooth, feats


def test_a_three_pixel_stroke_is_a_stroke_seed():
    ridge, smooth, feats = _stroke_scene()
    s = stroke_seeds(ridge, smooth, feats, 6.0, FLOOR)
    assert s[18:21, 5:36].all()
    assert s.sum() == 3 * 31, "the stroke's own pixels and nothing else"


def test_a_short_stroke_is_not():
    ridge, smooth, feats = _stroke_scene(cols=(5, 11))  # 6 px long: 18 pixels over a mean width near 2
    assert not stroke_seeds(ridge, smooth, feats, 6.0, FLOOR).any()


def test_short_dashes_seed_nothing():
    ridge, smooth, feats = _stroke_scene(cols=(5, 11))
    for c0 in (14, 23, 32):
        r2, s2, _ = _stroke_scene(cols=(c0, c0 + 6))
        ridge |= r2
        smooth &= s2
        feats[18:21, c0:c0 + 6, 0] = 20.0
    assert not stroke_seeds(ridge, smooth, feats, 6.0, FLOOR).any()


def test_a_band_between_two_different_grounds_is_declined():
    # a sharpened edge's rim, a JPEG ringing line: between a shape and its ground
    ridge, smooth, feats = _stroke_scene(above_l=95.0, below_l=50.0)
    assert not stroke_seeds(ridge, smooth, feats, 6.0, FLOOR).any()


def test_a_stroke_whose_colour_ramps_is_declined():
    ridge, smooth, feats = _stroke_scene()
    feats[18:21, 5:36, 0] = np.linspace(20.0, 60.0, 31)[None, :]
    assert not stroke_seeds(ridge, smooth, feats, 6.0, FLOOR).any()


def _with_body(colour: float):
    """The scene with a body of L* `colour` at rows 10–29, columns 36–39, joined
    to the stroke's right end (the end ridge is gone, the body's own edges are
    ridge): a wedge's tip meeting its body, or a stem meeting a bowl."""
    ridge, smooth, feats = _stroke_scene()
    feats[10:30, 36:, 0] = colour
    ridge[17:22, 36] = False
    ridge[9, 35:] = True
    ridge[30, 35:] = True
    ridge[10:17, 35] = True
    ridge[22:30, 35] = True
    smooth = ~ndimage.binary_dilation(ridge)
    return ridge, smooth, feats


def test_a_stroke_touching_a_seed_of_its_own_colour_is_left_to_that_seed():
    ridge, smooth, feats = _with_body(20.0)
    assert not stroke_seeds(ridge, smooth, feats, 6.0, FLOOR).any()


def test_a_stroke_touching_a_seed_of_another_colour_keeps_its_own():
    ridge, smooth, feats = _with_body(60.0)
    assert stroke_seeds(ridge, smooth, feats, 6.0, FLOOR)[18:21, 5:36].all()


def test_a_stroke_on_the_border_is_left_to_the_flood():
    ridge, smooth, feats = _stroke_scene()
    feats = np.ascontiguousarray(feats[18:])  # the stroke's top row is the image's top row
    ridge = np.ascontiguousarray(ridge[18:])
    smooth = np.ascontiguousarray(smooth[18:])
    assert not stroke_seeds(ridge, smooth, feats, 6.0, FLOOR).any()


def test_a_tiny_image_has_no_stroke_seeds():
    feats = np.zeros((4, 4, 4), np.float32)
    feats[..., 3] = 100.0
    ridge = np.zeros((4, 4), bool)
    ridge[1] = True
    assert not stroke_seeds(ridge, ~ridge, feats, 6.0, FLOOR).any()


def test_seed_parts_agrees_with_seed_mask():
    data, _, _ = _disc_cut_by_an_edge(8.0)
    a = np.ascontiguousarray(np.asarray(Image.open(io.BytesIO(data)).convert("RGBA")))
    p = prepare(a)
    grad = discontinuity(p.features)
    smooth, ridge = seed_parts(p.features, grad, 1.5)
    assert np.array_equal(smooth, seed_mask(p.features, grad, 1.5))
    assert ridge.dtype == bool and ridge.shape == smooth.shape and ridge.any()
