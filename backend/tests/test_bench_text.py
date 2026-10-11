"""bench.synth's text class: glyph outlines as vector truth, strokes of a set width."""
import io
import re

import numpy as np
from PIL import Image

from bench import geometry
from bench.synth import TEMPLATES, _font, _stem_em, TEXT_FONT, render_png, text_line


def test_text_line_writes_absolute_cubic_paths_only():
    font = _font(TEXT_FONT)
    svg = text_line(font, "OI", 40.0, 10.0, 60.0)
    ds = re.findall(r'd="([^"]+)"', svg)
    assert len(ds) == 2, "one path per glyph"
    for d in ds:
        assert set(re.findall(r"[A-Za-z]", d)) <= {"M", "L", "C", "Z"}
        assert geometry._segments(d) is not None


def test_stem_width_sets_the_size():
    font = _font(TEXT_FONT)
    size = 3.0 / _stem_em(font)
    svg = text_line(font, "I", size, 10.0, 60.0)
    xs = [float(v) for v in re.findall(r"[ML](-?\d+\.\d+) ", svg)]
    assert abs((max(xs) - min(xs)) - 3.0) < 0.05


def test_every_text_template_fits_the_canvas_and_has_strokes_of_its_width():
    names = [n for n, _ in TEMPLATES["text"]]
    assert names == ["sans-2-white", "sans-2-gradient", "sans-3-white", "sans-3-gradient", "sans-4-white", "sans-4-gradient"]
    for name, fn in TEMPLATES["text"]:
        svg = fn(None)
        xs = [float(v) for v in re.findall(r"[MLC](-?\d+\.\d+) ", svg)]
        assert 8.0 <= min(xs) and max(xs) <= 504.0, name
        png = np.asarray(Image.open(io.BytesIO(render_png(svg, 512))).convert("RGBA"))
        ink = png[..., :3].sum(axis=2) < 3 * 128
        stroke = int(name.split("-")[1])
        assert ink.sum() > 500 * stroke, name  # about 1260, 2900 and 5350 px at 2, 3 and 4 px
        # the capital I of "OUTSIDE" is a vertical bar: the narrowest ink run across the caps line is a stem
        row = ink[192]  # through the caps line (baseline 200; the cap height is >= 19 px at every stroke)
        runs = np.diff(np.flatnonzero(np.diff(np.concatenate([[0], row.astype(int), [0]]))))[::2]
        assert stroke - 1 <= runs.min() <= stroke + 1, (name, sorted(runs)[:5])  # +-1 for the anti-aliased phase
