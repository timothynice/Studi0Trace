# Thin-Stroke Seeding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A 2–4 px stroke (a thin letter, a rule, a hairline icon stroke) keeps a watershed seed of its own, so the trace draws it instead of flooding it from its neighbours.

**Architecture:** A new partition step, `stroke_seeds`, runs after `seed_mask` in both engines: it takes the non-ridge pixels that lie between two ridge pixels within `STROKE_SEED_W` along one of four axes, forms 4-connected components of them, and keeps the components that are long (`STROKE_SEED_LEN` pixels over their mean width), one colour, a different colour from what lies beyond the ridge on both sides while those two sides agree, and not touching a kept seed of their own colour. Those pixels are OR'ed into the seed mask before `seed_markers`' floor, kept whatever their size, and their basins are exempt from `_absorb_small`. Bench items for the case come first (a `text` class in the synthetic corpus and two degraded sources), the references are frozen before the change, and `tools/qloop.sh full` gates it.

**Tech Stack:** Python (numpy, scipy.ndimage, skimage) reference engine in `backend/studi0trace/engines/vexel/partition.py`; Rust port in `backend/vexel-rs/src/partition.rs` built with maturin; `tools/diffcheck.py` parity; fontTools for the bench's glyph outlines; the bench (`python -m bench`, `tools/qloop.sh`).

**Spec:** `docs/superpowers/specs/2026-10-09-thin-stroke-seeding-design.md`

## Global Constraints

- Run everything from `backend/` with `.venv/bin/python` (uv venv, already built, `vexel_rs` already installed from this checkout). Rebuild the extension after every Rust change: `.venv/bin/maturin develop --release -m vexel-rs/Cargo.toml` (from `backend/`).
- `VEXEL_BACKEND=rust` is what the bench runs (`qloop.sh` exports it); `VEXEL_BACKEND=python` runs the reference.
- The two engines must agree: `diffcheck labels0`, `merge` and the new `stroke_seeds` stage pass over the whole corpus (`.venv/bin/python -m tools.diffcheck labels0 merge stroke_seeds`).
- Constants, in both engines with these names and values, not parameters (no `Params` field, no UI control): `STROKE_SEED_W` = 5, `STROKE_SEED_LEN` = 12; every colour test is at `detail`.
- Float sums that decide anything run in float64 in raster order in both engines (`np.bincount(..., weights=)` on the Python side, a raster loop on the Rust side); integer tests stay integer. No `set`/`HashMap` iteration order decides anything.
- Nothing in `backend/vexel-rs/src/timing.rs` reads a clock unless `VEXEL_TIMING` is set; the core crate stays wasm-clean (this plan does not touch `crates/`, except by re-exporting its fixtures).
- `tools/qloop.sh full` must pass (no per-item regression on corpus, held-out and degraded against the references frozen in Task 3). The new text items at 512 must have `pinholes`, `slivers`, `thin_strokes` = 0 and `outline_px` ≤ 0.3 after the change.
- After the engine's output changes: `cd backend && .venv/bin/python -m tools.export_core_fixtures --only scorecard,api,auto`, then `cargo test --workspace --release` at the repo root (needs `frontend/dist`, which exists).
- Commit messages: a sentence on what and why, blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Never stage `.superpowers/`, `backend/bench/reports/` or anything under the scratchpad. Tim's three example images never enter git.
- The Python engine's `tests/` run with `cd backend && .venv/bin/python -m pytest` (whole suite at the end of Tasks 6, 8 and 9; targeted files in between).

## Review Focus

1. **A stroke along the image border** (a rule touching the top edge): `ridge_reach` finds no outside sample on the border side, those pixels fail the two-sided test, the component is declined, nothing raises. Pinned in Task 5 (`test_a_stroke_on_the_border_is_left_to_the_flood`).
2. **A stroke on a transparent canvas**: the feature's alpha channel carries the difference (ink alpha 100 against ground alpha 0), so the stroke is seeded and the two transparent sides agree. Pinned in Task 6 (`test_a_ring_on_a_transparent_canvas_keeps_its_seed`).
3. **An image smaller than the search reach** (a 4×4 input): every shift falls off the image, `stroke_seeds` returns an empty mask, `initial_labels` is unchanged. Pinned in Task 5 (`test_a_tiny_image_has_no_stroke_seeds`).
4. **A gradient-inked stroke** (gold script): its colour spread exceeds `detail`, it is declined and flooded as before — the text re-setter's job, not this stage's. Pinned in Task 5 (`test_a_stroke_whose_colour_ramps_is_declined`).
5. **A dashed line** (dashes under 12 px): every dash is too short, nothing is seeded, the flood behaves as today. Pinned in Task 5 (`test_short_dashes_seed_nothing`).

---

## File Structure

- `backend/bench/fonts/Montserrat-Regular.ttf`, `backend/bench/fonts/OFL.txt` — copied from `apps/desktop/src-tauri/tests/fixtures/text/fonts/` (OFL permits).
- `backend/bench/synth.py` — the `text` class: `_SvgPen`, `_font`, `_stem_em`, `text_line`, `_text_template`, `TEMPLATES["text"]`.
- `backend/bench/config.py` — `CLASSES` gains `"text"`.
- `backend/bench/degraded.py` — two text sources.
- `backend/bench/sentinels.txt` — two text lines.
- `backend/studi0trace/engines/vexel/partition.py` — `seed_parts`, `stroke_seeds`, `seed_markers(strokes=)`, `_absorb_small(protected=)`, `initial_labels` wiring, dumps.
- `backend/vexel-rs/src/partition.rs` — the port; `backend/vexel-rs/src/lib.rs` — `_stage_stroke_seeds`.
- `backend/tools/diffcheck.py` — the `stroke_seeds` stage and its tolerance.
- Tests: `backend/tests/test_vexel_partition.py` (new tests), `backend/tests/test_vexel_parity.py` (one parity test), `backend/tests/test_bench_corpus.py`, `backend/tests/test_bench_degraded.py` (counts), Rust unit tests in `partition.rs`.
- Docs: the spec's Status section, `CLAUDE.md` (one bullet), `backend/bench/baselines/vexel.json`.

---

### Task 1: Bench `text` class

**Files:**
- Create: `backend/bench/fonts/Montserrat-Regular.ttf`, `backend/bench/fonts/OFL.txt`
- Modify: `backend/bench/synth.py` (templates dict at the end, helpers before it), `backend/bench/config.py:10`
- Test: `backend/tests/test_bench_corpus.py`, new `backend/tests/test_bench_text.py`

**Interfaces:**
- Produces: `bench.synth.text_line(font, string, size, x, baseline, tracking_em=0.0) -> str` (SVG `<path>` elements, absolute M/L/C/Z only), `bench.synth.TEMPLATES["text"]` with six templates named `sans-{2,3,4}-{white,gradient}`, corpus items `text/sans-3-white-512` etc. (Task 2 and 3 use the ids).

- [ ] **Step 1: Copy the font**

```bash
mkdir -p backend/bench/fonts
cp apps/desktop/src-tauri/tests/fixtures/text/fonts/Montserrat-Regular.ttf backend/bench/fonts/
cp apps/desktop/src-tauri/tests/fixtures/text/fonts/OFL.txt backend/bench/fonts/
```

- [ ] **Step 2: Write the failing tests**

`backend/tests/test_bench_text.py`:

```python
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
        assert ink.sum() > 2000, name
        # the capital I of "OUTSIDE" is a vertical bar: the narrowest ink run across the caps line is a stem
        stroke = int(name.split("-")[1])
        row = ink[192]  # through the caps line (baseline 200; the cap height is ≥ 19 px at every stroke)
        runs = np.diff(np.flatnonzero(np.diff(np.concatenate([[0], row.astype(int), [0]]))))[::2]
        assert stroke - 1 <= runs.min() <= stroke + 1, (name, sorted(runs)[:5])  # ±1 for the anti-aliased phase
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd backend && .venv/bin/python -m pytest tests/test_bench_text.py -q`
Expected: FAIL with `ImportError: cannot import name '_font'`.

- [ ] **Step 4: Add the class to `bench/config.py`**

```python
CLASSES: tuple[str, ...] = ("logo", "flat", "gradient", "shadow", "text")
```

- [ ] **Step 5: Write the text helpers and templates in `bench/synth.py`**

Add to the imports at the top:

```python
from functools import lru_cache

from fontTools.pens.basePen import BasePen
from fontTools.pens.transformPen import TransformPen
```

Extend the module docstring's class list with `  text      a tracked all-caps line and a lowercase line at 2, 3 and 4 px stroke widths, white and pale-gradient ground`.

Insert before `TEMPLATES` (after `shadow_inset_well`):

```python
# --- text ---------------------------------------------------------------------
# A tracked all-caps line and a lowercase line, set so the stems are 2, 3 or 4 px
# wide on the 512 canvas, on white and on a pale gradient: the strokes the
# thin-stroke seeding exists for (`2026-10-09-thin-stroke-seeding-design.md`).
# The glyph outlines are the vector truth. Montserrat Regular is OFL (bench/fonts).

FONT_DIR = Path(__file__).resolve().parent / "fonts"
TEXT_FONT = "Montserrat-Regular.ttf"
TEXT_INK = "#1F2A44"
TEXT_CAPS = "OUTSIDE"
TEXT_LOWER = "brighter days"
TEXT_TRACKING = 0.15  # em, on the caps line
TEXT_CAPS_BASELINE = 200.0
TEXT_LOWER_BASELINE = 330.0


class _SvgPen(BasePen):
    """Absolute M/L/C/Z only, which is what `bench.geometry._segments` reads
    (fontTools' SVGPathPen writes H and V, and a TrueType glyph's quadratics
    are decomposed to cubics by BasePen itself)."""

    def __init__(self, glyph_set):
        super().__init__(glyph_set)
        self.parts: list[str] = []

    def _moveTo(self, p):
        self.parts.append("M%.3f %.3f" % p)

    def _lineTo(self, p):
        self.parts.append("L%.3f %.3f" % p)

    def _curveToOne(self, c1, c2, p):
        self.parts.append("C%.3f %.3f %.3f %.3f %.3f %.3f" % (*c1, *c2, *p))

    def _closePath(self):
        self.parts.append("Z")

    def _endPath(self):
        self.parts.append("Z")


@lru_cache(maxsize=None)
def _font(name: str):
    from fontTools.ttLib import TTFont

    return TTFont(FONT_DIR / name)


def _stem_em(font) -> float:
    """The stem of I as a share of the em: what a stroke width is set from."""
    from fontTools.pens.boundsPen import BoundsPen

    glyphs = font.getGlyphSet()
    pen = BoundsPen(glyphs)
    glyphs[font.getBestCmap()[ord("I")]].draw(pen)
    x0, _, x1, _ = pen.bounds
    return (x1 - x0) / font["head"].unitsPerEm


def text_line(font, string: str, size: float, x: float, baseline: float, tracking_em: float = 0.0) -> str:
    """`string` as one `<path>` per glyph at the font's own advances plus
    `tracking_em` per glyph, no kerning; `size` is the em in canvas units."""
    glyphs, cmap = font.getGlyphSet(), font.getBestCmap()
    s = size / font["head"].unitsPerEm
    out: list[str] = []
    for ch in string:
        name = cmap.get(ord(ch))
        glyph = glyphs[name] if name is not None else None
        if glyph is not None and ch != " ":
            pen = _SvgPen(glyphs)
            glyph.draw(TransformPen(pen, (s, 0, 0, -s, x, baseline)))
            if pen.parts:
                out.append(f'<path d="{"".join(pen.parts)}" fill="{TEXT_INK}"/>')
        x += (glyph.width if glyph is not None else 0.0) * s + tracking_em * size
    return "".join(out)


def _line_width(font, string: str, size: float, tracking_em: float) -> float:
    glyphs, cmap = font.getGlyphSet(), font.getBestCmap()
    s = size / font["head"].unitsPerEm
    return sum(glyphs[cmap[ord(ch)]].width * s for ch in string) + tracking_em * size * (len(string) - 1)


def _text_template(stroke: float, gradient: bool) -> Callable[[random.Random], str]:
    def fn(rng):
        font = _font(TEXT_FONT)
        size = stroke / _stem_em(font)
        caps_x = (CANVAS - _line_width(font, TEXT_CAPS, size, TEXT_TRACKING)) / 2
        lower_x = (CANVAS - _line_width(font, TEXT_LOWER, size, 0.0)) / 2
        body = (text_line(font, TEXT_CAPS, size, caps_x, TEXT_CAPS_BASELINE, TEXT_TRACKING)
                + text_line(font, TEXT_LOWER, size, lower_x, TEXT_LOWER_BASELINE))
        if gradient:
            defs = ('<linearGradient id="g" x1="0" y1="0" x2="1" y2="1">'
                    '<stop offset="0" stop-color="#FBF6EE"/><stop offset="1" stop-color="#E6EDF5"/></linearGradient>')
            return _svg(body, defs=defs, background="url(#g)")
        return _svg(body, background="#FFFFFF")

    return fn
```

And in `TEMPLATES`, after the `"shadow"` list:

```python
    "text": [
        ("sans-2-white", _text_template(2.0, False)), ("sans-2-gradient", _text_template(2.0, True)),
        ("sans-3-white", _text_template(3.0, False)), ("sans-3-gradient", _text_template(3.0, True)),
        ("sans-4-white", _text_template(4.0, False)), ("sans-4-gradient", _text_template(4.0, True)),
    ],
```

`_svg` takes `background` as a fill string, so `url(#g)` works as written. `Path` is already imported in `synth.py`.

- [ ] **Step 6: Run the new tests**

Run: `cd backend && .venv/bin/python -m pytest tests/test_bench_text.py -q`
Expected: PASS. If the stroke-run assertion fails by one pixel on the gradient items, inspect `runs` — anti-aliasing can widen a run by one; the test allows `stroke` or `stroke + 1`. If the caps line at row 180 misses the I (cap height at 2 px stroke is 0.7 × 27 ≈ 19 px, so rows 181–200 are inked), move the sampled row to 190.

- [ ] **Step 7: Update the corpus test counts and regenerate**

In `backend/tests/test_bench_corpus.py`: `n_templates * 3 == 84` → `== 102`, `len(loaded) == 84` → `== 102`, the class set gains `"text"`. Then:

```bash
cd backend && .venv/bin/python -m bench generate
.venv/bin/python -m pytest tests/test_bench_corpus.py tests/test_bench_text.py -q
```

Expected: PASS; `bench/corpus/synthetic/text/` holds 6 SVGs and 18 PNGs; `bench/corpus/manifest.yaml` lists `text/sans-3-white-512` etc. Confirm the existing items' PNG bytes are unchanged: `git status --short bench/corpus | grep -v text/` prints only `manifest.yaml`.

- [ ] **Step 8: Score the new items once to confirm the truth is read**

```bash
cd backend && VEXEL_BACKEND=rust .venv/bin/python -m bench run --engines vexel --classes text --no-media --workers 6 --out /tmp/text-probe
.venv/bin/python - <<'EOF'
import json
r = json.load(open("/tmp/text-probe/results.json"))
for it in r["items"]:
    m = it.get("metrics", {})
    print(it["id"], {k: m.get(k) for k in ("score", "outline_px", "pinholes", "slivers", "thin_strokes")})
EOF
```

Expected: every item has a finite `outline_px` (the truth parsed). Paste the table into the task report: it is the "before" of the gate (expect missing glyphs on the 2 px items, i.e. high `outline_px`).

- [ ] **Step 9: Commit**

```bash
git add backend/bench/fonts backend/bench/synth.py backend/bench/config.py backend/bench/corpus backend/tests/test_bench_text.py backend/tests/test_bench_corpus.py
git commit -m "bench: a text class of six synthetic items, two lines of Montserrat at 2, 3 and 4 px stems on white and a pale gradient, with the glyph outlines as vector truth

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Degraded sources and sentinels for text

**Files:**
- Modify: `backend/bench/degraded.py:325-338` (SOURCES), `backend/bench/sentinels.txt`
- Test: `backend/tests/test_bench_degraded.py:169-178`

- [ ] **Step 1: Update the counts in the test**

In `backend/tests/test_bench_degraded.py`, `52` becomes `60` in each of the four places (`len(dg.SOURCES) * len(dg.CLASSES) == 60`, `60 + 60 + 1 + 1 + 2`, `len(loaded) == 60`).

- [ ] **Step 2: Run to verify it fails**

Run: `cd backend && .venv/bin/python -m pytest tests/test_bench_degraded.py -q -k "generate or manifest"`
Expected: FAIL on the count.

- [ ] **Step 3: Add the sources**

In `SOURCES`, at the END of the tuple, after the `wave-lockup` line (the nn2x row phase is the source's index parity, so inserting earlier would re-render every later source):

```python
    Source("sans-3-white", "corpus", "corpus/synthetic/text/sans-3-white.svg"),
    Source("sans-4-gradient", "corpus", "corpus/synthetic/text/sans-4-gradient.svg"),
```

Update the module docstring's "Thirteen sources" to "Fifteen sources … two text lines of the synthetic corpus".

- [ ] **Step 4: Regenerate and test**

```bash
cd backend && .venv/bin/python -m bench.degraded generate
.venv/bin/python -m pytest tests/test_bench_degraded.py -q
git status --short bench/degraded | grep -v "sans-" 
```

Expected: PASS; the last command prints only `manifest.yaml` and `SOURCES.md` (existing renders are byte-identical).

- [ ] **Step 5: Sentinels**

Append to `backend/bench/sentinels.txt`:

```
corpus text/sans-3-white-512          # 3 px stems: thin-stroke seeding
degraded nn2x/sans-3-white-512        # 1.5 px stems doubled to crisp 3 px
```

- [ ] **Step 6: Commit**

```bash
git add backend/bench/degraded.py backend/bench/degraded backend/bench/sentinels.txt backend/tests/test_bench_degraded.py
git commit -m "bench: the degraded set and the sentinels cover the 3 px and 4 px text items

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: Freeze the references before the change

**Files:** none in git (`backend/bench/reports/` is ignored).

- [ ] **Step 1: Freeze all three sets**

```bash
cd backend && time tools/qloop.sh ref
```

Expected: three lines `reference: bench/reports/keep-2026-10-05-wave-lockup/ref-<set>/results.json`. Takes minutes; `WORKERS=6` by default.

- [ ] **Step 2: Prove the gate is quiet on an unchanged engine**

```bash
cd backend && tools/qloop.sh sentinels
```

Expected: exit 0, no regressions (the sentinels re-run against the fresh references).

- [ ] **Step 3: Record the text items' reference metrics**

```bash
cd backend && .venv/bin/python - <<'EOF'
import json
r = json.load(open("bench/reports/keep-2026-10-05-wave-lockup/ref-corpus/results.json"))
for it in r["items"]:
    if it["id"].startswith("text/") and it["engine"] == "vexel":
        m = it["metrics"]; print(it["id"], {k: m.get(k) for k in ("score", "outline_px", "pinholes", "slivers", "thin_strokes", "artifact_index")})
d = json.load(open("bench/reports/keep-2026-10-05-wave-lockup/ref-degraded/results.json"))
for it in d["items"]:
    if "sans-" in it["id"] and it["engine"] == "vexel":
        m = it["metrics"]; print(it["id"], {k: m.get(k) for k in ("score", "outline_px", "pinholes", "slivers", "thin_strokes")})
EOF
```

Put the table in the task report (it goes into the spec's Status in Task 10). No commit.

---

### Task 4: Dump hooks and the diagnosis

**Files:**
- Modify: `backend/studi0trace/engines/vexel/partition.py:409-432` (`initial_labels`), `backend/vexel-rs/src/partition.rs:437-464` (`initial_labels`)
- Scratchpad: `<scratchpad>/stroke-seeds/diagnose.py` (not in git)

**Interfaces:**
- Produces: `VEXEL_DUMP` files `seed_mask.txt` and `seed_markers.txt` from both engines (`dump.labels` format: `h w` then one row per line).

- [ ] **Step 1: Python dumps**

In `partition.py`, add `from studi0trace.engines.vexel import dump` to the imports, and in `initial_labels` after `smooth = …` and after `markers, origin = …`:

```python
    dump.labels("seed_mask", smooth.astype(np.int32))
    markers, origin = seed_markers(smooth, max(min_region, 2), features, detail, grad)
    dump.labels("seed_markers", markers)
```

- [ ] **Step 2: Rust dumps**

In `partition.rs` `initial_labels`, after `let smooth = …`:

```rust
    crate::dump::labels("seed_mask", &Grid { h: smooth.h, w: smooth.w, data: smooth.data.iter().map(|b| *b as i32).collect() });
```

and after `seed_markers(...)`: `crate::dump::labels("seed_markers", &markers);`. Rebuild: `cd backend && .venv/bin/maturin develop --release -m vexel-rs/Cargo.toml`.

- [ ] **Step 3: Verify the dumps agree**

```bash
cd backend && mkdir -p /tmp/dump-py /tmp/dump-rs
VEXEL_DUMP=/tmp/dump-py VEXEL_BACKEND=python .venv/bin/python -c "
from studi0trace.engines.vexel.engine import VexelEngine, VexelParams
from studi0trace.imaging.intake import load_upload
VexelEngine().trace(load_upload(open('bench/corpus/synthetic/text/sans-3-white-512.png','rb').read(), max_bytes=1<<30, max_pixels=1<<30), VexelParams())"
VEXEL_DUMP=/tmp/dump-rs VEXEL_BACKEND=rust .venv/bin/python -c "
from studi0trace.engines.vexel.engine import VexelEngine, VexelParams
from studi0trace.imaging.intake import load_upload
VexelEngine().trace(load_upload(open('bench/corpus/synthetic/text/sans-3-white-512.png','rb').read(), max_bytes=1<<30, max_pixels=1<<30), VexelParams())"
cmp /tmp/dump-py/seed_mask.txt /tmp/dump-rs/seed_mask.txt && cmp /tmp/dump-py/seed_markers.txt /tmp/dump-rs/seed_markers.txt && echo SAME
```

Expected: `SAME`.

- [ ] **Step 4: Diagnose per glyph**

Write `<scratchpad>/stroke-seeds/diagnose.py`: for each PNG given on the command line, trace with `VEXEL_DUMP` set (Rust backend, Balanced preset `VexelParams()` and Logo `VexelParams(min_region=16)`), read `seed_mask.txt` and `seed_markers.txt`, take the ink mask (pixels darker than the mean of the image by 60 levels, or alpha-weighted for Tim's logos), label its 8-connected components (glyphs), and print per glyph: pixels, seed-mask pixels inside, marker pixels inside, and `NO MARKER` when zero. Run it on `bench/corpus/synthetic/text/sans-{2,3,4}-white-512.png` and on Tim's three logos (`$STUDI0TRACE_TEXT_EXAMPLES/{1,2,3}.png`; the controller sets the variable to the scratchpad's `text/` folder).

- [ ] **Step 5: Report**

In the task report, give the counts: glyphs with no marker per item and preset, and whether the seed-mask fragments inside the thin glyphs are below the floor (6 or 16). This confirms the spec's mechanism; if the dumps show something else (for example the glyphs have markers but lose them later), say so — the controller decides before Task 5.

- [ ] **Step 6: Commit the hooks**

```bash
git add backend/studi0trace/engines/vexel/partition.py backend/vexel-rs/src/partition.rs
git commit -m "partition: dump the seed mask and the seed markers under VEXEL_DUMP in both engines

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Python `seed_parts` and `stroke_seeds`

**Files:**
- Modify: `backend/studi0trace/engines/vexel/partition.py` (after `seed_mask`, before `_absorb_small`)
- Test: `backend/tests/test_vexel_partition.py`

**Interfaces:**
- Produces: `seed_parts(features, grad, g_low, g_seed=8.0) -> tuple[np.ndarray, np.ndarray]` = `(smooth, ridge)`, bool arrays; `seed_mask` becomes `seed_parts(...)[0]`. `stroke_seeds(ridge, smooth, features, detail, floor) -> np.ndarray` bool mask of the accepted stroke-seed pixels. Constants `STROKE_SEED_W = 5`, `STROKE_SEED_LEN = 12`, `_AXES`.

- [ ] **Step 1: Write the failing tests**

Append to `backend/tests/test_vexel_partition.py` (add `stroke_seeds, seed_parts` to the partition import):

```python
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
```

Why the scene is built this way: the body's seed (its pixels more than one from a ridge, columns 37–39 rows 11–28, 54 px ≥ FLOOR) is 4-adjacent to the stroke component's last column (column 36, between the ridge rows 17 and 21), with no ridge between — as a wedge's body seed meets the tip's medial pixels where the width passes `STROKE_SEED_W`. The ground is its own kept seed on the other side of the ridges and never touches the stroke.

Add `seed_mask` to the partition import too.

- [ ] **Step 2: Run to verify they fail**

Run: `cd backend && .venv/bin/python -m pytest tests/test_vexel_partition.py -q -k "stroke or dashes or band or ramps or border or tiny or seed_parts"`
Expected: FAIL with `ImportError: cannot import name 'stroke_seeds'`.

- [ ] **Step 3: Implement**

In `partition.py`, replace `seed_mask` with `seed_parts` plus a wrapper, and add `stroke_seeds` after it:

```python
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
# own colour (an acute wedge's tip, the gap between two letters in the ground's
# colour: the flood from that seed is the right answer there). Its pixels join
# the seed mask before the floor, its component is kept whatever its size, and
# its basin is exempt from `_absorb_small`. A crisp stroke exactly two pixels
# wide is all ridge (both of its pixels peak) and has no medial pixel: it is
# left to the rescue, as before. Spec: 2026-10-09-thin-stroke-seeding-design.md.
STROKE_SEED_W = 5
STROKE_SEED_LEN = 12
_AXES = ((0, 1), (1, 0), (1, 1), (1, -1))  # (dr, dc): the order ties between equally narrow axes break in


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
    # two-sided: the pixel differs from both samples beyond the ridges, which agree
    sel = cand & has_out
    ys, xs = np.nonzero(sel)
    fp = feats[ys, xs]
    fa = feats[out_r[0][ys, xs], out_c[0][ys, xs]]
    fb = feats[out_r[1][ys, xs], out_c[1][ys, xs]]
    ok = (np.sum((fp - fa) ** 2, axis=1) > d2) & (np.sum((fp - fb) ** 2, axis=1) > d2) & (np.sum((fa - fb) ** 2, axis=1) < d2)
    passing = np.bincount(comps[ys, xs], weights=ok.astype(np.float64), minlength=n).astype(np.int64)
    two_sided = 2 * passing > size
    # not touching a kept seed of its own colour: the kept components of `smooth`
    # at their full size, their colour read off their pixels outside the stroke
    # candidates (a stroke's own valley line runs into a neighbouring seed and
    # would blend its colour into that seed's); a kept component with no such
    # pixel is the stroke's own valley and is not another seed
    kept = cc_label(smooth, connectivity=1)
    kflat = kept.ravel()
    nk = int(kept.max()) + 1
    ksize = np.bincount(kflat, minlength=nk)
    outside = kflat * (~cand.ravel())
    kcount = np.bincount(outside, minlength=nk)
    kmean = np.stack([np.bincount(outside, weights=feats[..., c].ravel(), minlength=nk) for c in range(nc)], axis=1) / np.maximum(kcount, 1)[:, None]
    big = (ksize >= floor) & (kcount > 0)
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
    return accept[comps]
```

- [ ] **Step 4: Run the tests**

Run: `cd backend && .venv/bin/python -m pytest tests/test_vexel_partition.py -q`
Expected: all PASS (the existing seed tests too: `seed_mask` is unchanged in effect).

- [ ] **Step 5: Commit**

```bash
git add backend/studi0trace/engines/vexel/partition.py backend/tests/test_vexel_partition.py
git commit -m "partition: stroke_seeds finds the medial pixels of a thin feature between two ridges and keeps the long, one-colour, two-sided components as seeds; seed_parts exposes the ridge the seed mask was cut by

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: Wire the stroke seeds into the Python partition

**Files:**
- Modify: `backend/studi0trace/engines/vexel/partition.py` (`seed_markers`, `_absorb_small`, `initial_labels`)
- Test: `backend/tests/test_vexel_partition.py`

**Interfaces:**
- Produces: `seed_markers(smooth, floor, features=None, detail=6.0, grad=None, strokes=None)` — components containing a `strokes` pixel are kept whatever their size; `_absorb_small(labels, grad, min_region, rounds=3, protected=None)` — `protected[label]` True exempts the label; `initial_labels` dumps `stroke_seeds` as well.

- [ ] **Step 1: Write the failing tests**

Append to `backend/tests/test_vexel_partition.py`:

```python
def _ring_png(stroke: float, size: int = 96, ground: str | None = "#FFFFFF") -> bytes:
    bg = f'<rect width="{size}" height="{size}" fill="{ground}"/>' if ground else ""
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">{bg}'
           f'<circle cx="48" cy="48" r="24" fill="none" stroke="#1F2A44" stroke-width="{stroke}"/>'
           f'<rect x="10" y="84" width="60" height="{stroke}" fill="#1F2A44"/></svg>')
    return bytes(resvg_py.svg_to_bytes(svg_string=svg, width=size, height=size))


def _labels_of(png: bytes, **kw) -> np.ndarray:
    a = np.ascontiguousarray(np.asarray(Image.open(io.BytesIO(png)).convert("RGBA")))
    p = prepare(a)
    return initial_labels(discontinuity(p.features), p.features, **kw)


def test_a_three_pixel_ring_keeps_its_seed_and_its_counter():
    labels = _labels_of(_ring_png(3.0), min_region=16, detail=6.0)
    ring, inside, outside = labels[48, 24], labels[48, 48], labels[4, 4]
    assert ring not in (inside, outside) and inside != outside
    assert labels[48, 72] == ring and labels[24, 48] == ring, "one label round the ring"


def test_a_thin_bar_survives_absorb_small():
    # the bar is 60 × 3 = 180 px, under a min_region of 200: without the protection it is flooded by the ground
    labels = _labels_of(_ring_png(3.0), min_region=200, detail=6.0)
    assert labels[85, 40] != labels[4, 4]
    assert labels[85, 40] == labels[85, 20] == labels[85, 60]


def test_a_ring_on_a_transparent_canvas_keeps_its_seed():
    labels = _labels_of(_ring_png(3.0, ground=None), min_region=16, detail=6.0)
    ring, inside, outside = labels[48, 24], labels[48, 48], labels[4, 4]
    assert ring not in (inside, outside)


def test_seed_markers_keep_a_small_component_that_holds_a_stroke_pixel():
    smooth = np.zeros((20, 40), bool)
    smooth[8:11, 5:35] = True  # 90 px: over a floor of 6, under 100
    feats = np.zeros((20, 40, 4), np.float32)
    feats[..., 3] = 100.0
    strokes = smooth.copy()
    m, _ = seed_markers(smooth, 100, feats, 6.0, np.zeros((20, 40), np.float32))
    assert m.max() == 0
    m, _ = seed_markers(smooth, 100, feats, 6.0, np.zeros((20, 40), np.float32), strokes)
    assert m.max() == 1 and np.array_equal(m > 0, smooth)
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd backend && .venv/bin/python -m pytest tests/test_vexel_partition.py -q -k "ring or bar or keep_a_small"`
Expected: FAIL (`seed_markers` rejects the `strokes` argument; the ring's label equals the outside's).

- [ ] **Step 3: Implement**

`seed_markers`: add the parameter `strokes: np.ndarray | None = None` after `grad`, and change the top:

```python
    comps = cc_label(smooth if strokes is None else (smooth | strokes), connectivity=1)
    if comps.max() == 0:
        return comps.astype(np.int32), np.zeros(1, np.int64)
    sizes = np.bincount(comps.ravel())
    keep = sizes >= floor
    if strokes is not None:
        keep[np.unique(comps[strokes])] = True  # a stroke seed is kept whatever its area
    keep[0] = False
```

Extend its docstring: "`strokes` (`stroke_seeds`) joins the mask, and a component holding a stroke pixel is kept whatever its size."

`_absorb_small`: add `protected: np.ndarray | None = None` and after `small[0] = False`:

```python
        if protected is not None:
            small[:protected.size] &= ~protected[:small.size]
```

Docstring: "`protected[label]` exempts a label: a stroke seed's basin is a region however small."

`initial_labels`:

```python
    if features is not None:
        smooth, ridge = seed_parts(features, grad, g_low)
        strokes = stroke_seeds(ridge, smooth, features, detail, max(min_region, 2))
    else:
        smooth, strokes = grad < g_low, None
    dump.labels("seed_mask", smooth.astype(np.int32))
    if strokes is not None:
        dump.labels("stroke_seeds", strokes.astype(np.int32))
    markers, origin = seed_markers(smooth, max(min_region, 2), features, detail, grad, strokes)
    dump.labels("seed_markers", markers)
    protected = None
    if strokes is not None and strokes.any() and markers.max() > 0:
        protected = np.zeros(int(markers.max()) + 1, bool)
        protected[np.unique(markers[strokes])] = True
        protected[0] = False
    if markers.max() == 0:
        ...  # unchanged fallback
    labels = watershed(grad, markers)
    labels = rejoin_ramps(labels, markers, origin, grad, features)
    labels = _absorb_small(labels, grad, min_region, protected=protected)
```

Update the docstring: "Stroke seeds (`stroke_seeds`) join the seed mask, are kept whatever their size and are exempt from the small-region absorb."

- [ ] **Step 4: Run the partition tests, then the whole Python suite**

```bash
cd backend && .venv/bin/python -m pytest tests/test_vexel_partition.py -q
VEXEL_BACKEND=python .venv/bin/python -m pytest -q -x
```

Expected: PASS. If an engine golden test fails because a trace changed where a thin stroke is now its own region, look at the trace: a thin stroke now drawn is the intended change — update the expectation and say so in the report; anything else is a bug.

- [ ] **Step 5: Commit**

```bash
git add backend/studi0trace/engines/vexel/partition.py backend/tests/test_vexel_partition.py
git commit -m "partition: stroke seeds join the seed mask before the floor, are kept whatever their size and are exempt from the small-region absorb, so a 3 px ring keeps its counter and a thin bar survives Logo's min_region

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: Rust port

**Files:**
- Modify: `backend/vexel-rs/src/partition.rs` (`seed_parts`, `seed_mask`, `stroke_seeds`, `seed_markers`, `absorb_small`, `initial_labels`), `backend/vexel-rs/src/lib.rs` (`_stage_stroke_seeds`, registration)
- Test: `#[cfg(test)] mod stroke_tests` at the end of `partition.rs`

**Interfaces:**
- Produces: `pub fn seed_parts(features: &Image, grad: &Grid<f64>, g_low: f64, g_seed: f64) -> (Mask, Mask)`; `pub fn stroke_seeds(ridge: &Mask, smooth: &Mask, features: &Image, detail: f64, floor: usize) -> Mask`; `pub fn seed_markers(smooth, floor, features, detail, grad, strokes: Option<&Mask>)`; `fn absorb_small(labels, grad, min_region, rounds, protected: &[bool])`; `pub const STROKE_SEED_W: usize = 5; pub const STROKE_SEED_LEN: usize = 12;`; Python-visible `vexel_rs._stage_stroke_seeds(rgba, h, w, min_region, detail) -> Vec<i32>`.

- [ ] **Step 1: Write the failing Rust tests**

At the end of `partition.rs`:

```rust
#[cfg(test)]
mod stroke_tests {
    use super::*;

    const FLOOR: usize = 40;

    /// `tests/test_vexel_partition.py::_stroke_scene`: a 3 px stroke on rows 18–20
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

    /// `_with_body`: a body of L* `colour` at rows 10–29, columns 36–39, joined
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

    #[test]
    fn a_three_pixel_stroke_is_a_stroke_seed() {
        let (ridge, smooth, f) = scene(5, 36, 20.0, 95.0, 95.0);
        let s = stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR);
        assert_eq!(s.count(), 3 * 31);
        assert!((18..21).all(|r| (5..36).all(|c| *s.get(r, c))));
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
        // a body 8 levels from the ink: another colour at detail 6, even though the
        // stroke's own valley line runs into the body's seed component
        let (ridge, smooth, f) = with_body(28.0);
        let s = stroke_seeds(&ridge, &smooth, &f, 6.0, FLOOR);
        assert!((18..21).all(|r| (5..36).all(|c| *s.get(r, c))));
    }

    #[test]
    fn a_ridge_on_the_image_edge_has_no_sample_beyond_it() {
        // the Python's `test_a_ridge_on_the_border_has_no_sample_beyond_it`: ridge on row 0,
        // stroke on rows 1–3 → no sample above → declined; one ground row above → seeded
        let (ridge, smooth, f) = scene(5, 36, 20.0, 95.0, 95.0);
        let crop = |m: &Mask, from: usize| -> Mask {
            Grid { h: 40 - from, w: 40, data: m.data[from * 40..].to_vec() }
        };
        let mut fc = Image::new(40 - 17, 40, 4);
        fc.data.copy_from_slice(&f.data[17 * 40 * 4..]);
        assert_eq!(stroke_seeds(&crop(&ridge, 17), &crop(&smooth, 17), &fc, 6.0, FLOOR).count(), 0);
        let mut fp = Image::new(40 - 16, 40, 4);
        fp.data.copy_from_slice(&f.data[16 * 40 * 4..]);
        let s = stroke_seeds(&crop(&ridge, 16), &crop(&smooth, 16), &fp, 6.0, FLOOR);
        assert!((2..5).all(|r| (5..36).all(|c| *s.get(r, c))));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd backend/vexel-rs && cargo test --release stroke_tests`
Expected: compile error, `stroke_seeds` not found.

- [ ] **Step 3: Implement**

Replace `seed_mask` with:

```rust
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
const AXES: [(isize, isize); 4] = [(0, 1), (1, 0), (1, 1), (1, -1)];

/// Along +(dr, dc) from (r, c): the distance 1..W to the nearest ridge pixel
/// (0: none within W) and the distance to the first non-ridge pixel on the
/// image past that ridge's run, within W more (0: none).
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
    let mut dist = 0;
    for k in 1..=STROKE_SEED_W {
        if at(k) == Some(true) {
            dist = k;
            break;
        }
    }
    if dist == 0 {
        return (0, 0);
    }
    for k in dist + 1..=dist + STROKE_SEED_W {
        if at(k) == Some(false) {
            return (dist, k);
        }
    }
    (dist, 0)
}

/// The pixels of every stroke seed (`partition.stroke_seeds`): the non-ridge
/// pixels between two ridge pixels within `STROKE_SEED_W` along one of four
/// axes, in 4-connected components that are `STROKE_SEED_LEN` long (pixels
/// over mean width), one colour (RMS spread under `detail`), a different
/// colour from the first non-ridge pixel beyond the ridge on both sides by
/// more than `detail` while those two agree within `detail`, and not touching
/// a kept seed (a component of `smooth` of `floor` pixels or more) of their
/// own colour. Sums run in raster order in f64, as the Python's do.
pub fn stroke_seeds(ridge: &Mask, smooth: &Mask, features: &Image, detail: f64, floor: usize) -> Mask {
    let (h, w) = (ridge.h, ridge.w);
    let nc = features.c;
    let d2 = detail * detail;
    let mut width = vec![0usize; h * w];
    let mut out = vec![(usize::MAX, usize::MAX); h * w]; // the samples beyond the + and - ridges
    for r in 0..h {
        for c in 0..w {
            let i = r * w + c;
            if ridge.data[i] {
                continue;
            }
            for (dr, dc) in AXES {
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
                let idx = |k: usize, sr: isize, sc: isize| -> usize {
                    ((r as isize + k as isize * sr) as usize) * w + (c as isize + k as isize * sc) as usize
                };
                out[i] = if bp > 0 && bm > 0 { (idx(bp, dr, dc), idx(bm, -dr, -dc)) } else { (usize::MAX, usize::MAX) };
            }
        }
    }
    let cand = Grid { h, w, data: width.iter().map(|v| *v > 0).collect() };
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
            if da > d2 && db > d2 && ds < d2 {
                passing[k] += 1;
            }
        }
    }
    // kept seeds: components of `smooth` of `floor` pixels or more (their full
    // size), whose colour is the mean of their pixels outside the stroke
    // candidates — a stroke's own valley line runs into a neighbouring seed and
    // would blend its colour into that seed's (`partition.py` says why; a kept
    // component with no such pixel is the stroke's own valley and is skipped)
    let kept = labels::label_mask(smooth, 1);
    let nk = kept.data.iter().copied().max().unwrap_or(0) as usize + 1;
    let ksize = labels::bincount(&kept);
    let mut kcount = vec![0usize; nk];
    let mut ksums = vec![0.0f64; nk * nc];
    for i in 0..h * w {
        let k = kept.data[i] as usize;
        if k == 0 || cand.data[i] {
            continue;
        }
        kcount[k] += 1;
        let px = features.px(i);
        for ch in 0..nc {
            ksums[k * nc + ch] += px[ch];
        }
    }
    let kmean = |k: usize, ch: usize| ksums[k * nc + ch] / (kcount[k].max(1) as f64);
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
                if kk != 0 && ksize[kk] >= floor && kcount[kk] > 0 {
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
    Grid { h, w, data: comps.data.iter().map(|k| accept[*k as usize]).collect() }
}
```

Note the Python's mean is `sums / max(size, 1)` and its spread test `sq / max(size, 1) < d2`: the same expressions, in the same order, in f64.

`seed_markers`: add `strokes: Option<&Mask>` as the last parameter; at the top:

```rust
    let joined: Mask = match strokes {
        Some(s) => smooth.or(s),
        None => smooth.clone(),
    };
    let mut comps = labels::label_mask(&joined, 1);
    ...
    let sizes = labels::bincount(&comps);
    let mut keep: Vec<bool> = sizes.iter().enumerate().map(|(i, s)| i != 0 && *s >= floor).collect();
    if let Some(s) = strokes {
        for (c, is_stroke) in comps.data.iter().zip(s.data.iter()) {
            if *is_stroke && *c > 0 {
                keep[*c as usize] = true;
            }
        }
    }
```

`absorb_small`: add `protected: &[bool]` and build `small` as `i != 0 && *s < min_region && !protected.get(i).copied().unwrap_or(false)`.

`initial_labels`:

```rust
    let (smooth, ridge) = seed_parts(features, grad, g_low, 8.0);
    t.lap("    labels/seed_mask");
    let strokes = stroke_seeds(&ridge, &smooth, features, detail, min_region.max(2));
    t.lap("    labels/stroke_seeds");
    crate::dump::labels("seed_mask", &Grid { h: smooth.h, w: smooth.w, data: smooth.data.iter().map(|b| *b as i32).collect() });
    crate::dump::labels("stroke_seeds", &Grid { h: strokes.h, w: strokes.w, data: strokes.data.iter().map(|b| *b as i32).collect() });
    let (mut markers, mut origin) = seed_markers(&smooth, min_region.max(2), features, detail, grad, Some(&strokes));
    crate::dump::labels("seed_markers", &markers);
    let mut protected = vec![false; markers.data.iter().copied().max().unwrap_or(0) as usize + 1];
    for (m, s) in markers.data.iter().zip(strokes.data.iter()) {
        if *s && *m > 0 {
            protected[*m as usize] = true;
        }
    }
    ...
    let l = absorb_small(&l, grad, min_region, 3, &protected);
```

(In the fallback branch where markers are rebuilt from minima, reset `protected = vec![]`.)

`lib.rs`: next to `_stage_seed`:

```rust
    #[pyfunction]
    fn _stage_stroke_seeds(rgba: Vec<u8>, h: usize, w: usize, min_region: usize, detail: f64) -> Vec<i32> {
        let prep = prepare::prepare(&rgba, h, w);
        let grad = partition::discontinuity(&prep.features, 0.7);
        let (smooth, ridge) = partition::seed_parts(&prep.features, &grad, 1.5, 8.0);
        partition::stroke_seeds(&ridge, &smooth, &prep.features, detail, min_region.max(2)).data.iter().map(|b| *b as i32).collect()
    }
```

and `m.add_function(wrap_pyfunction!(_stage_stroke_seeds, m)?)?;` beside `_stage_seed`'s registration. Check `grep -n "seed_markers(" vexel-rs/src/` for any other caller and give it `None`.

- [ ] **Step 4: Build and test**

```bash
cd backend/vexel-rs && cargo test --release stroke_tests && cargo clippy --release --no-deps -- -D warnings 2>&1 | tail -3
cd .. && .venv/bin/maturin develop --release -m vexel-rs/Cargo.toml
.venv/bin/python -m pytest tests/test_vexel_partition.py -q
```

Expected: PASS; clippy clean (if clippy was not clean before this task, add nothing new).

- [ ] **Step 5: Commit**

```bash
git add backend/vexel-rs/src/partition.rs backend/vexel-rs/src/lib.rs
git commit -m "vexel-rs: port the stroke seeds (seed_parts, stroke_seeds, kept components, the protected absorb) and expose _stage_stroke_seeds

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: Parity — the `stroke_seeds` diffcheck stage and a parity test

**Files:**
- Modify: `backend/tools/diffcheck.py:72` (TOLERANCE), stages after `labels0`
- Test: `backend/tests/test_vexel_partition.py`

- [ ] **Step 1: Write the failing parity test**

Append to `backend/tests/test_vexel_partition.py`:

```python
@pytest.mark.skipif(vexel._vexel_rs is None, reason="the vexel_rs extension is not built")
@pytest.mark.parametrize("stroke", [2.0, 3.0, 4.0])
def test_the_engines_seed_strokes_alike(stroke):
    from studi0trace.engines.vexel.partition import seed_parts, stroke_seeds
    a = np.ascontiguousarray(np.asarray(Image.open(io.BytesIO(_ring_png(stroke))).convert("RGBA")))
    p = prepare(a)
    grad = discontinuity(p.features)
    smooth, ridge = seed_parts(p.features, grad, 1.5)
    py = stroke_seeds(ridge, smooth, p.features, 6.0, 16)
    rs = np.asarray(vexel._vexel_rs._stage_stroke_seeds(a.tobytes(), 96, 96, 16, 6.0), np.int32).reshape(96, 96)
    assert np.array_equal(py.astype(np.int32), rs)
    assert py.sum() > 100, "the ring and the bar are seeded"
    py_l = initial_labels(grad, p.features, min_region=16, detail=6.0)
    rs_l = np.asarray(vexel._vexel_rs._stage_labels0(a.tobytes(), 96, 96, 16, 6.0), np.int32).reshape(96, 96)
    assert np.array_equal(py_l, rs_l)
```

(`vexel._vexel_rs` is how `test_the_engines_split_seeds_alike` reaches the extension; keep that.)

- [ ] **Step 2: Run it**

Run: `cd backend && .venv/bin/python -m pytest tests/test_vexel_partition.py -q -k alike`
Expected: PASS already if Task 7 is right; if the masks differ, print `np.argwhere(py != rs)` and fix the port — the usual culprit is the order of the `better` test (strict `<`) or the off-image rule in `ridge_reach`.

- [ ] **Step 3: The diffcheck stage**

In `tools/diffcheck.py`, after `"labels0": ("max", 0.0, 0.002),`:

```python
    # The stroke seeds alone (`partition.stroke_seeds`): decisions at `detail`
    # on feature colours the two engines agree on to 1e-4, so a component whose
    # spread or side difference sits at the threshold can go either way; the
    # same slack as `labels0`.
    "stroke_seeds": ("max", 0.0, 0.002),
```

After the `labels0` stage function:

```python
@stage
def stroke_seeds(path):
    from studi0trace.engines.vexel.partition import seed_parts, stroke_seeds as _stroke_seeds

    a = load(path)
    h, w = a.shape[:2]
    prep = prepare(a)
    grad = discontinuity(prep.features)
    smooth, ridge = seed_parts(prep.features, grad, 1.5)
    py = _stroke_seeds(ridge, smooth, prep.features, 6.0, 6).astype(np.int32)
    rs = np.asarray(vexel_rs._stage_stroke_seeds(a.tobytes(), h, w, 6, 6.0), dtype=np.int32).reshape(h, w)
    return py, rs
```

Give `_stage_stroke_seeds` the same `#[pyo3(signature = (...))]` default for `detail` that `_stage_labels0` has in `lib.rs` (the `labels0` stage calls it with four arguments), so the two stages are called alike.

- [ ] **Step 4: Run the parity stages over the corpus**

```bash
cd backend && .venv/bin/python -m tools.diffcheck stroke_seeds labels0 merge 2>&1 | tail -15
```

Expected: every item `ok`. A `FAIL` on `stroke_seeds` with a handful of pixels is within tolerance; a whole component differing means an order or threshold difference — fix the port, not the tolerance.

- [ ] **Step 5: Run the whole Python suite on both backends**

```bash
cd backend && VEXEL_BACKEND=rust .venv/bin/python -m pytest -q -x && VEXEL_BACKEND=python .venv/bin/python -m pytest -q -x tests/test_vexel_partition.py tests/test_vexel_parity.py tests/test_vexel_backends.py
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/tools/diffcheck.py backend/tests/test_vexel_partition.py
git commit -m "diffcheck: a stroke_seeds stage holds the two engines' stroke seeds together over the corpus; a parity test on a ring and a bar at 2, 3 and 4 px

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: The gate, the fixtures and the workspace tests

**Files:**
- Modify (regenerated): `crates/studi0trace-core/tests/fixtures/**` (by `export_core_fixtures`), `backend/bench/baselines/vexel.json`

- [ ] **Step 1: Run the full gate**

```bash
cd backend && time tools/qloop.sh full 2>&1 | tee /tmp/qloop-full.txt | tail -60
```

Expected: exit 0 with `== corpus`, `== heldout`, `== degraded` each reporting no regressions, and improvements on the `text/` items. Report the whole gate output (regressions, improvements, per class) in the task report.

- [ ] **Step 2: The text items' acceptance**

```bash
cd backend && .venv/bin/python - <<'EOF'
import glob, json
latest = sorted(glob.glob("bench/reports/keep-2026-10-05-wave-lockup/full-*-corpus/results.json"))[-1]
for it in json.load(open(latest))["items"]:
    if it["id"].startswith("text/") and it["engine"] == "vexel":
        m = it["metrics"]; print(it["id"], {k: m.get(k) for k in ("score", "outline_px", "pinholes", "slivers", "thin_strokes", "artifact_index")})
EOF
```

Expected on every `text/*-512` and `-512-ds` item: `pinholes`, `slivers`, `thin_strokes` = 0 and `outline_px` ≤ 0.3. If a 2 px item misses, look at its trace by eye (`bench run` with media for that id) and say which glyphs are missing and why (the crisp-2-px limit described in the partition comment is the one expected shortfall at `-ds`; report it rather than tuning).

- [ ] **Step 3: On a regression**

Do not change the constants. For each regressed item: trace it with `VEXEL_DUMP`, find the stroke seed that caused it (`stroke_seeds.txt` against the image), and report which of the five tests let it through. Stop and report; the controller decides the fix.

- [ ] **Step 4: Re-export the core fixtures and run the workspace tests**

```bash
cd backend && .venv/bin/python -m tools.export_core_fixtures --only scorecard,api,auto
cd .. && cargo test --workspace --release 2>&1 | grep -E "^test result|FAILED|panicked" | head -20
```

Expected: every `test result: ok`. The fixture files that changed are the traces that now contain a thin stroke; `git diff --stat crates/studi0trace-core/tests/fixtures` shows them.

- [ ] **Step 5: Baselines**

```bash
cd backend && VEXEL_BACKEND=rust .venv/bin/python -m bench run --engines vexel --no-media --workers 6 --update-baseline --out /tmp/baseline-run
git diff --stat bench/baselines/vexel.json
```

- [ ] **Step 6: Commit**

```bash
git add crates/studi0trace-core/tests/fixtures backend/bench/baselines/vexel.json
git commit -m "fixtures and baseline after the stroke seeds: the traces the core's tests hold now draw the thin strokes

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Tim's logos, the spec's Status and CLAUDE.md

**Files:**
- Modify: `docs/superpowers/specs/2026-10-09-thin-stroke-seeding-design.md` (new `## Status` section at the end), `CLAUDE.md` (one bullet under Conventions, after the seed-area bullet "A seed area is one region only while it is one colour…")
- Scratchpad: `<scratchpad>/stroke-seeds/logos.py`

- [ ] **Step 1: Measure the tagline crops before and after**

Write `<scratchpad>/stroke-seeds/logos.py`: it takes a list of PNGs, and for each one crops the tagline (the boxes are in `<scratchpad>/text/crop.py`; copy them), traces the crop at Balanced (`VexelParams()`) and at Logo (`VexelParams(min_region=16)`) through `VexelEngine().trace(load_upload(...), params)`, writes the SVG beside a copy of the crop under `<scratchpad>/stroke-seeds/<label>/`, and prints the artifact index of each (`from bench.artifacts import …` as `bench/cli.py`'s `cmd_artifacts` does, or run `python -m bench artifacts OUT.svg CROP.png` on each pair).

"After" is this checkout with the Rust backend: `cd backend && VEXEL_BACKEND=rust .venv/bin/python <scratchpad>/stroke-seeds/logos.py after $STUDI0TRACE_TEXT_EXAMPLES/{1,2,3}.png`.

"Before" is the engine as it was at `d7de82d`, run as Python (no build needed; the two engines are held together by diffcheck, so the Python before is the Rust before):

```bash
git worktree add <scratchpad>/before d7de82d
cd <scratchpad>/before/backend && VEXEL_BACKEND=python PYTHONPATH=. <worktree>/backend/.venv/bin/python <scratchpad>/stroke-seeds/logos.py before $STUDI0TRACE_TEXT_EXAMPLES/{1,2,3}.png
git -C <worktree> worktree remove <scratchpad>/before
```

Report per logo and preset: artifact index before → after, and by eye (open both SVGs beside the crop) whether letters that were missing are now present and whether anything new is wrong. The renders stay in the scratchpad.

- [ ] **Step 2: Spec Status**

Append to the spec:

```markdown
## Status (2026-10-11)

Built as `docs/superpowers/plans/2026-10-11-thin-stroke-seeding.md`. What was built as the spec says, and where it differs:

- `stroke_seeds` in both engines; `diffcheck stroke_seeds` holds them together at `labels0`'s slack.
- The "between two ridge pixels" test runs along the four pixel axes (0°, 90°, 45°, 135°) with the narrowest axis deciding, not along the structure-tensor orientation: the orientation flips at junctions, which is what fragmented the valley seeds in the first place.
- Two tests the spec did not name: the two sides beyond the ridge must agree within `detail` (without it a sharpened edge's rim, a JPEG ringing line and a drop shadow's band qualified: each is one colour, long, and differs from both of its sides), and a component touching a kept seed of its own colour is dropped (a wedge tip, the ground between two letters).
- Length is pixels over mean width (an integer test), not a skeleton.
- A crisp stroke exactly two pixels wide has no medial pixel (both of its pixels are ridge) and is not seeded; anti-aliased 2 px strokes are.
- Gate: <the qloop full summary>; text items: <the table>; Tim's logos: <the crops' artifact index before → after>.
```

Fill the angle brackets from the task reports.

- [ ] **Step 3: CLAUDE.md**

Add after the seed-area bullet:

```markdown
- A thin stroke is seeded as a stroke (`partition.stroke_seeds`, both engines, `diffcheck stroke_seeds`):
  the non-ridge pixels between two ridge pixels within `STROKE_SEED_W` (5) along one of the four axes,
  in components `STROKE_SEED_LEN` (12) long over their mean width, one colour, differing from what lies
  beyond the ridge on both sides while those sides agree, and not touching a kept seed of their own
  colour; kept whatever their size and exempt from `_absorb_small`. Lowering the seed floor or pooling
  fragments admitted noise everywhere (the wave-lockup plan's try9 and D1): do not go back to either.
  The bench's `text` class (`bench/synth.py`, Montserrat at 2, 3 and 4 px stems) and two degraded
  sources hold it.
```

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-10-09-thin-stroke-seeding-design.md CLAUDE.md
git commit -m "docs: the thin-stroke seeding's status (what differs from the spec, the gate, the logos) and its CLAUDE.md bullet

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```
