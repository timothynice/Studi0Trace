# Text-as-Shapes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A row of same-coloured glyph regions that the trace draws as shapes gets the regularity a person expects of a word: one baseline, one cap height (and one x-height), stems of one width, and repeated letters that are the same shape.

**Architecture:** A new stage, `vexel/lettering.py` (Rust `vexel-rs/src/lettering.rs`), runs inside `topology.build` after `_imprint` and before `_bleed_arcs`, on the boundary graph (arcs with placed vertices and fitted segments shared between neighbours): it finds **lines** (≥ 3 regions of one ink colour in a row whose bottoms agree), snaps each region's bottom-edge geometry onto the line's median baseline and its top-edge geometry onto the cluster it shares (cap height, x-height), every move bounded by `LINE_SNAP`, nodes moved through `_move_node` so neighbours follow; optionally equalises stem widths within `STEM_SNAP`; and returns the line membership so `reuse.emit` writes two glyphs that agree within `REPEAT_TOL` as one `<use>`. Bench item first, references frozen, a diagnosis of the actual scatter before any rule is built (the controller rules which rules are worth building), then Python, Rust, parity through `diffcheck arcs`, the gate, fixtures, the logos.

**Tech Stack:** Python (numpy) reference engine; Rust port (maturin); `tools/diffcheck.py`; the bench.

**Spec:** `docs/superpowers/specs/2026-10-09-text-as-shapes-design.md`

## Global Constraints

- Run from `backend/` with `.venv/bin/python`; rebuild the extension after every Rust change (`.venv/bin/maturin develop --release -m vexel-rs/Cargo.toml`).
- Constants in both engines: `LINE_SNAP` = 0.6, `STEM_SNAP` = 0.3, `REPEAT_TOL` = 0.25 (the spec's), plus `LINE_INK` = 12.0 (RGB-255 distance within which two regions' fill colours are one ink — a needed detail the spec leaves open; recorded as a deviation), `LINE_MIN` = 3 regions, `LINE_GAP` = 2.0 (× the line's median box width). Not parameters.
- No node moves further than `LINE_SNAP`; no segment end moves further than `SNAP_END_MOVE` (0.15) by the stem rule; a node another line region also holds is left.
- Determinism: regions ordered by left edge then label; medians are the lower-middle element; float work in f64 in the same order in both engines; no `set`/`HashMap` iteration decides anything.
- Parity: `.venv/bin/python -m tools.diffcheck arcs labels0 merge stroke_seeds` passes (the `arcs` stage runs `topology.build` in both engines on one label map, so it covers the lettering stage; a separate `lettering` stage is not needed — recorded as a deviation from the spec's protocol wording).
- Gate: `tools/qloop.sh full` against references frozen in Task 2; on the text items `wobble_deg_100px` falls and `outline_px` rises by at most 0.02; the artifact index of the three logos' taglines falls (manual, out of git).
- Fixtures: `export_core_fixtures --only scorecard,api,auto` (+`--force` if the provenance's Python differs), `cargo test --workspace --release`.
- Commits: a sentence on what and why, trailer `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`; never stage `.superpowers/`, `bench/reports/`, the scratchpad; Tim's images never enter git.
- Lessons from the two previous loops: measure before building (Task 3's ruling decides which sub-rules are built); a gate regression is diagnosed to its stage before any constant moves; residuals are ruled on and recorded, never tuned away.

## Review Focus

1. **A lowercase line with ascenders and descenders** ("brighter days"): the spec's "median top fits two thirds" fails on its own example (7 x-height tops of 11); the line test must rest on the bottoms, with descenders (g, y) allowed as the minority, and tops snapped per cluster. Pinned in Task 4 (`test_a_lowercase_line_is_a_line_by_its_baseline`).
2. **Two lines stacked** (a tagline under a wordmark of the same ink): y-overlap under half parts them into two lines; neither snaps to the other's baseline. Pinned in Task 4 (`test_two_rows_are_two_lines`).
3. **Glyphs that touch** (kerned pairs sharing a node): the node both regions hold is left; each region's other bottom geometry still snaps. Pinned in Task 4 (`test_a_node_two_glyphs_share_is_left`).
4. **A round-bottomed glyph** (o, e, s): no node lies on its bottom; the segment points within `LINE_SNAP` of its bottom shift instead, so the bowl's lowest point lands on the baseline. Pinned in Task 4 (`test_a_bowl_lands_on_the_baseline`).
5. **A glyph whose bottom is further than `LINE_SNAP` from the median** (a descender, a dropped letter, a badly placed one): left alone. Pinned in Task 4 (`test_a_descender_is_left_alone`).

---

## File Structure

- `backend/bench/synth.py` — `text/sans-3-repeat` template (`"letters settle better"` lowercase at 3 px, white).
- `backend/studi0trace/engines/vexel/lettering.py` — `Line`, `find_lines`, `snap_lines`, `equalise_stems`, `regularise_lettering(bnd, labels, fill_at, params) -> dict[int, int]`.
- `backend/studi0trace/engines/vexel/topology.py` — `build(...)` calls it after `_imprint` and returns the membership on `Boundary.lines`.
- `backend/studi0trace/engines/vexel/reuse.py` — `emit(pending, precision, first_id=1, loose=frozenset())` with `REPEAT_TOL` for pairs in `loose`; `engine.py`'s `_emit` passes the line glyphs.
- `backend/vexel-rs/src/lettering.rs`, `topology.rs`, `reuse.rs`, `engine.rs` — the port.
- Tests: `backend/tests/test_vexel_lettering.py` (new), `test_vexel_reuse.py` (if present; else in the new file), `test_bench_corpus.py`; Rust tests in `lettering.rs`.
- Docs: the spec's Status, a `CLAUDE.md` bullet, `backend/bench/baselines/vexel.json`.

---

### Task 1: Bench item — a lowercase line with repeated letters

**Files:** `backend/bench/synth.py` (one template in `TEMPLATES["text"]`), `backend/tests/test_bench_corpus.py` (counts +3: 111 → 114), `backend/tests/test_bench_text.py` (the template-name list gains `"sans-3-repeat"`).

- [ ] **Step 1 (RED):** change the counts and the name list; run the two test files → FAIL.
- [ ] **Step 2:** in `synth.py`, after `_text_template`:

```python
def _text_repeat_template(stroke: float) -> Callable[[random.Random], str]:
    """One lowercase line with repeated letters ("letters settle better": six
    e's, five t's, three s's, two l's, two r's) at `stroke` px stems on white:
    the text-as-shapes loop's item (`2026-10-09-text-as-shapes-design.md`)."""
    def fn(rng):
        font = _font(TEXT_FONT)
        size = stroke / _stem_em(font)
        string = "letters settle better"
        x = (CANVAS - _line_width(font, string, size, 0.0)) / 2
        return _svg(text_line(font, string, size, x, 270.0), background="#FFFFFF")
    return fn
```

and `("sans-3-repeat", _text_repeat_template(3.0))` appended to `TEMPLATES["text"]`. `python -m bench generate`; `git status --short bench/corpus | grep -v repeat` prints only `manifest.yaml`; look at `text/sans-3-repeat-512.png` (one line, centred, 3 px stems).
- [ ] **Step 3:** tests green; commit (`synth.py`, the corpus files, the two tests): "bench: a lowercase line with repeated letters at 3 px stems for the text-as-shapes loop" + trailer.

---

### Task 2: Freeze the references

- [ ] `cd backend && tools/qloop.sh ref` (all sets, ~7 min) then `tools/qloop.sh sentinels` → ok. Record every `text/` item's `wobble_deg_100px`, `outline_px`, `artifact_index`, `elements` and `uses` (if the metrics carry a `<use>` count; else count `<use` in the trace SVGs under the run directory) from `ref-corpus/results.json` in the report. No commit.

---

### Task 3: Diagnose — the scatter before any rule

**Files:** scratchpad only.

- [ ] **Step 1:** Write `<scratchpad>/lettering/scatter.py`: trace (Python engine, `VEXEL_DUMP` on) each `text/*-512` item, `text/sans-3-repeat-512`, and the three logos (`$STUDI0TRACE_TEXT_EXAMPLES/{1,2,3}.png`, Balanced and Logo); read `arcs.txt` (every arc's `pts`, pair, segments — `dump.arcs` format) and the final label map; for every region that is a glyph (ink-coloured, in the tagline rows — select by colour and the tagline box; on the synthetic items every non-ground region), compute from its rings' vertices: bottom = max y, top = min y, the stem widths (horizontal distance between facing vertical `Line` segments of the same region, |angle − 90°| ≤ 1°), and for each pair of regions in a row the outline agreement after translation (sample the segments as `reuse._sample` does; mean nearest-neighbour distance both ways).
- [ ] **Step 2:** Per line (group by row by eye or by the Task 4 rule prototyped in the script): the baseline scatter (std and max |bottom − median| over the regions within 1 px of the median), the cap scatter, the x-height scatter, the stem-width scatter per cluster, and the number of region pairs whose outlines agree within 0.25 px but not within 0.1 px (the repeats `reuse` misses today). Also what the `-ds` and 128 items show.
- [ ] **Step 3:** Report tables and a recommendation: which rules move anything (a scatter under 0.1 px means the rule is a no-op), what the expected wobble gain is, and whether the stem rule (the riskiest) is worth building. The controller rules before Task 4: which of the sub-rules (baseline/top snap; repeats; stems) are built.

---

### Task 4: Python — lines, baseline and top snap

**Files:**
- Create: `backend/studi0trace/engines/vexel/lettering.py`
- Modify: `backend/studi0trace/engines/vexel/topology.py` (`Boundary` gains `lines: dict[int, int]`; `build` calls `regularise_lettering` after `_imprint`), `backend/tests/test_vexel_lettering.py` (new)

**Interfaces:**
- Produces: `regularise_lettering(bnd: Boundary, labels: np.ndarray, fill_at: FillAt, params: CurveParams) -> dict[int, int]` (label → line id, 1-based, lines ordered by their median baseline then left edge); it mutates `bnd.arcs` (pts and segments) within `LINE_SNAP`. Constants `LINE_SNAP`, `LINE_INK`, `LINE_MIN`, `LINE_GAP`. `Boundary.lines` carries the result for `engine._emit`.

- [ ] **Step 1: Tests (RED).** `tests/test_vexel_lettering.py` builds label maps of glyph-like rectangles and bowls on a ground and runs `topology.build` with a flat `fill_at` (ink colour for the glyph labels, white for the ground), then reads the glyphs' bottoms/tops from `bnd.rings`/`bnd.segments`:

```python
"""`lettering`: a row of glyph regions of one ink shares one baseline, one cap
height and (lowercase) one x-height, within LINE_SNAP; nodes two glyphs share
are left; a descender is left alone; two rows are two lines."""
from __future__ import annotations

import numpy as np

from studi0trace.engines.vexel import topology
from studi0trace.engines.vexel.curves import CurveParams, Line, Cubic, CircArc
from studi0trace.engines.vexel.lettering import LINE_SNAP, find_lines, regularise_lettering

INK = np.array([30.0, 30.0, 30.0, 255.0])
WHITE = np.array([255.0, 255.0, 255.0, 255.0])


def _scene(boxes: list[tuple[float, float, float, float]], size=(80, 200)):
    """Glyph boxes (x0, y0, x1, y1) in sub-pixel units, rendered as coverage
    into a label map (a pixel is a glyph when its centre is inside the box) and
    an rgb image with box-area anti-aliasing, so the placement lands the
    outline at the fractional edges."""
    h, w = size
    labels = np.ones((h, w), np.int32)
    rgb = np.full((h, w, 3), 255.0)
    yy, xx = np.mgrid[0:h, 0:w]
    for i, (x0, y0, x1, y1) in enumerate(boxes, start=2):
        inside = (xx + 0.5 >= x0) & (xx + 0.5 < x1) & (yy + 0.5 >= y0) & (yy + 0.5 < y1)
        labels[inside] = i
        cov = (np.clip(np.minimum(xx + 1, x1) - np.maximum(xx, x0), 0, 1)
               * np.clip(np.minimum(yy + 1, y1) - np.maximum(yy, y0), 0, 1))
        rgb = rgb * (1 - cov[..., None]) + INK[:3] * cov[..., None]
    alpha = np.ones((h, w))
    return labels, rgb, alpha


def _fill_at(labels):
    def f(lab, qx, qy):
        c = WHITE if lab == 1 else INK
        return np.tile(c, (len(qx), 1))
    return f


def _bottom_top(bnd, lab):
    pts = np.vstack([bnd.polyline(r) for r in bnd.rings(frozenset({lab}))])
    return float(pts[:, 1].max()), float(pts[:, 1].min())


def _build(boxes, size=(80, 200)):
    labels, rgb, alpha = _scene(boxes, size)
    bnd = topology.build(labels, rgb, alpha, _fill_at(labels), CurveParams(), extend=False)
    return bnd


def test_a_row_of_stems_shares_one_baseline_and_cap():
    # bottoms scattered within LINE_SNAP of 60.0: 59.7, 60.2, 60.0, 59.8; tops within LINE_SNAP of 30
    boxes = [(20, 30.2, 23, 59.7), (40, 29.8, 43, 60.2), (60, 30.0, 63, 60.0), (80, 30.3, 83, 59.8)]
    bnd = _build(boxes)
    assert set(bnd.lines.values()) == {1} and set(bnd.lines) == {2, 3, 4, 5}
    bottoms = [_bottom_top(bnd, lab)[0] for lab in (2, 3, 4, 5)]
    tops = [_bottom_top(bnd, lab)[1] for lab in (2, 3, 4, 5)]
    assert max(bottoms) - min(bottoms) < 0.05, bottoms
    assert max(tops) - min(tops) < 0.05, tops


def test_a_descender_is_left_alone():
    boxes = [(20, 30, 23, 60), (40, 30, 43, 60), (60, 30, 63, 60), (80, 30, 83, 68)]  # the last drops 8 px
    bnd = _build(boxes)
    assert bnd.lines.get(5) == 1, "a descender is still in the line (its top agrees)"
    assert abs(_bottom_top(bnd, 5)[0] - 68.0) < 0.2


def test_a_glyph_more_than_line_snap_off_is_not_moved():
    boxes = [(20, 30, 23, 60), (40, 30, 43, 60), (60, 30, 63, 60), (80, 30, 83, 61.0)]
    bnd = _build(boxes)
    assert abs(_bottom_top(bnd, 5)[0] - 61.0) < 0.2


def test_two_rows_are_two_lines():
    boxes = [(20, 10, 23, 30), (40, 10, 43, 30), (60, 10, 63, 30), (20, 50, 23, 70), (40, 50, 43, 70), (60, 50, 63, 70)]
    bnd = _build(boxes)
    assert bnd.lines[2] == bnd.lines[3] == bnd.lines[4] and bnd.lines[5] == bnd.lines[6] == bnd.lines[7]
    assert bnd.lines[2] != bnd.lines[5]


def test_fewer_than_three_regions_are_no_line():
    bnd = _build([(20, 30, 23, 60), (40, 30, 43, 60)])
    assert bnd.lines == {}


def test_a_lowercase_line_is_a_line_by_its_baseline():
    # tops: 4 ascenders at 20, 7 x-height at 35; bottoms: 9 on 60, 2 descenders at 68
    boxes = []
    x = 10
    for i in range(11):
        top = 20.0 if i in (0, 3, 6, 9) else 35.0
        bottom = 68.0 if i in (4, 8) else 60.0 + (0.3 if i % 2 else -0.2)
        boxes.append((x, top + (0.2 if i % 3 == 0 else -0.1), x + 3, bottom))
        x += 15
    bnd = _build(boxes, size=(90, 200))
    assert len(set(bnd.lines.values())) == 1 and len(bnd.lines) == 11
    bottoms = [_bottom_top(bnd, lab)[0] for lab in range(2, 13) if lab - 2 not in (4, 8)]
    assert max(bottoms) - min(bottoms) < 0.05
    xtops = [_bottom_top(bnd, lab)[1] for lab in range(2, 13) if lab - 2 not in (0, 3, 6, 9)]
    atops = [_bottom_top(bnd, lab)[1] for lab in range(2, 13) if lab - 2 in (0, 3, 6, 9)]
    assert max(xtops) - min(xtops) < 0.05 and max(atops) - min(atops) < 0.05


def test_a_bowl_lands_on_the_baseline():
    labels = np.ones((80, 200), np.int32)
    rgb = np.full((80, 200, 3), 255.0)
    yy, xx = np.mgrid[0:80, 0:200]
    for i, cx in enumerate((30, 70, 110), start=2):
        r2 = (xx + 0.5 - cx) ** 2 + (yy + 0.5 - 45.0) ** 2
        labels[r2 <= 14.8 ** 2] = i  # bowls of radius 14.8: bottom 59.8
        cov = np.clip(15.3 - np.sqrt(r2), 0, 1)
        rgb = rgb * (1 - cov[..., None]) + INK[:3] * cov[..., None]
    labels[30:60, 150:153] = 5  # a stem with bottom 60.0
    rgb[30:60, 150:153] = INK[:3]
    bnd = topology.build(labels, rgb, np.ones((80, 200)), _fill_at(labels), CurveParams(), extend=False)
    bottoms = [_bottom_top(bnd, lab)[0] for lab in (2, 3, 4, 5)]
    assert max(bottoms) - min(bottoms) < 0.1, bottoms


def test_a_node_two_glyphs_share_is_left():
    boxes = [(20, 30, 23, 60), (40, 30, 43, 60), (60, 30.3, 63, 60.4), (63, 30, 66, 59.6)]  # the last two touch
    bnd = _build(boxes)
    # the shared node (63, ~60) is held: neither glyph's shared corner moves, the free corners still snap
    free_bottoms = [_bottom_top(bnd, lab)[0] for lab in (2, 3)]
    assert max(free_bottoms) - min(free_bottoms) < 0.05
```

The scene helper's anti-aliasing is what lets the placement find the fractional edges; if `topology.build` places the stems' bottoms at the box edges to within 0.05 px without any lettering (check this first on the first test with the stage disabled: the bottoms should read 59.7/60.2/60.0/59.8 ± 0.05), the tests are sound. If the placement scatters them by more than 0.1 px, replace the box edges with values two tenths apart (59.6/60.2/…) so the assertions stay meaningful, and say so in the report.

- [ ] **Step 2: Implement `lettering.py`.**

```python
"""Stage 7c: a row of glyph regions is one word.

A line of text the trace draws as shapes (the text re-setter declined it, or
the reader never saw it) comes out with baselines and cap heights that wander
by a fraction of a pixel and two "e"s that are not the same shape: a person
reads that as a badly drawn word. After the fit, regions of one ink colour in a
row (`find_lines`) share one baseline: every region whose bottom is within
LINE_SNAP of the row's median bottom has its bottom geometry — the placed
vertices and the fitted segments' points within LINE_SNAP of its own bottom —
moved onto the median, nodes through `_move_node` so the neighbours follow; the
tops snap the same way per cluster (cap height, x-height: a cluster needs three
regions within LINE_SNAP of its median), descenders and ascenders being the
minorities that fall outside. No point moves further than LINE_SNAP, below the
placement's own accuracy, and a node another region of the line also holds is
left. Script lettering, whose glyphs join into one region, never forms a line.
Spec: 2026-10-09-text-as-shapes-design.md.
"""
from __future__ import annotations

from dataclasses import dataclass

import numpy as np

from studi0trace.engines.vexel.curves import Cubic, CurveParams

LINE_SNAP = 0.6   # px: the furthest any vertex, segment point or node moves
LINE_INK = 12.0   # RGB-255 distance within which two regions' fills are one ink
LINE_MIN = 3      # regions a line needs
LINE_GAP = 2.0    # × the line's median box width: the widest gap inside a line


@dataclass
class Glyph:
    label: int
    x0: float
    x1: float
    top: float
    bottom: float
    colour: np.ndarray


def _median(values: list[float]) -> float:
    """The lower-middle element, as every median in this engine."""
    s = sorted(values)
    return s[(len(s) - 1) // 2]


def glyphs_of(bnd, labels: np.ndarray, fill_at) -> list[Glyph]:
    """Every label with one ring whose fill is one colour, with its box from the placed vertices."""
    out = []
    for lab in (int(v) for v in np.unique(bnd.padded) if v != 0):
        rings = bnd.rings(frozenset({lab}))
        if len(rings) < 1:
            continue
        pts = np.vstack([bnd.polyline(r) for r in rings])
        ys, xs = np.nonzero(labels == lab)
        if xs.size == 0:
            continue
        cx, cy = float(xs.mean()) + 0.5, float(ys.mean()) + 0.5
        c_mid = fill_at(lab, np.array([cx]), np.array([cy]))[0]
        c_corner = fill_at(lab, np.array([float(pts[:, 0].min())]), np.array([float(pts[:, 1].min())]))[0]
        if float(np.linalg.norm(c_mid[:3] - c_corner[:3])) > LINE_INK or c_mid[3] < 128:
            continue  # a gradient or a translucent fill is not an ink
        out.append(Glyph(lab, float(pts[:, 0].min()), float(pts[:, 0].max()), float(pts[:, 1].min()), float(pts[:, 1].max()), c_mid[:3]))
    out.sort(key=lambda g: (g.x0, g.label))
    return out


def find_lines(glyphs: list[Glyph]) -> list[list[Glyph]]:
    """Rows of at least LINE_MIN glyphs of one ink whose boxes overlap in y by
    more than half the shorter height, with gaps under LINE_GAP × the row's
    median width, and whose bottoms agree: the median bottom fits at least two
    thirds of them within LINE_SNAP (descenders are the minority). Glyphs are
    taken in order of left edge then label; a glyph joins the first open row it
    fits, so the result does not depend on anything but that order."""
    rows: list[list[Glyph]] = []
    for g in glyphs:
        placed = False
        for row in rows:
            last = row[-1]
            if float(np.linalg.norm(g.colour - last.colour)) > LINE_INK:
                continue
            overlap = min(g.bottom, last.bottom) - max(g.top, last.top)
            if overlap <= 0.5 * min(g.bottom - g.top, last.bottom - last.top):
                continue
            widths = [r.x1 - r.x0 for r in row]
            if g.x0 - last.x1 > LINE_GAP * _median(widths):
                continue
            row.append(g)
            placed = True
            break
        if not placed:
            rows.append([g])
    lines = []
    for row in rows:
        if len(row) < LINE_MIN:
            continue
        base = _median([g.bottom for g in row])
        on = sum(1 for g in row if abs(g.bottom - base) <= LINE_SNAP)
        if 3 * on >= 2 * len(row):
            lines.append(row)
    lines.sort(key=lambda row: (_median([g.bottom for g in row]), row[0].x0))
    return lines


def _clusters(values: list[tuple[int, float]]) -> list[tuple[float, list[int]]]:
    """(target, members) for every group of at least LINE_MIN values within
    LINE_SNAP of the group's median; values sorted, grown greedily from the
    lowest, a value joining the open group while it is within LINE_SNAP of
    the group's first value."""
    out = []
    group: list[tuple[int, float]] = []
    for lab, v in sorted(values, key=lambda t: (t[1], t[0])):
        if group and v - group[0][1] > LINE_SNAP:
            if len(group) >= LINE_MIN:
                out.append((_median([t[1] for t in group]), [t[0] for t in group]))
            group = []
        group.append((lab, v))
    if len(group) >= LINE_MIN:
        out.append((_median([t[1] for t in group]), [t[0] for t in group]))
    return out


def _shift_edge(bnd, lab: int, held: set[int], at: float, target: float, line_labels: set[int]) -> None:
    """Move the bottom (or top) edge of `lab` from `at` to `target`: every
    placed vertex and segment point of its rings within LINE_SNAP of `at`
    shifts in y by target − at; a node moves through `_move_node` (every arc
    there follows) unless another region of the line holds it (`held`)."""
    from studi0trace.engines.vexel.topology import _move_node

    delta = target - at
    if abs(delta) <= 1e-9 or abs(delta) > LINE_SNAP:
        return
    rings = bnd.rings(frozenset({lab}))
    ring_arcs = {idx for ring in rings for idx, _r in ring}
    for ring in rings:
        for idx, _rev in ring:
            arc = bnd.arcs[idx]
            if not arc.closed:
                for end in (0, -1):
                    p = arc.pts[end]
                    if abs(p[1] - at) <= LINE_SNAP and int(round(p[0] * 1e6)) * 7 + int(round(p[1] * 1e6)) not in held:
                        q = p.copy()
                        q[1] += delta
                        _move_node(bnd.arcs, p, q, set())
            arc.pts = arc.pts.copy()
            near = np.abs(arc.pts[:, 1] - at) <= LINE_SNAP
            arc.pts[near, 1] += delta
            for seg in arc.segments:
                for name in ("p0", "p1", "c1", "c2"):
                    pt = getattr(seg, name, None)
                    if pt is not None and abs(pt[1] - at) <= LINE_SNAP:
                        pt[1] += delta


def _held_nodes(bnd, line_labels: set[int]) -> set[int]:
    """Nodes where two regions of the line meet, keyed as `_shift_edge` keys them."""
    held = set()
    for idx, arc in enumerate(bnd.arcs):
        a, b = arc.pair
        if a in line_labels and b in line_labels and not arc.closed:
            for end in (0, -1):
                p = arc.pts[end]
                held.add(int(round(p[0] * 1e6)) * 7 + int(round(p[1] * 1e6)))
    return held


def snap_lines(bnd, lines: list[list[Glyph]]) -> None:
    for row in lines:
        line_labels = {g.label for g in row}
        held = _held_nodes(bnd, line_labels)
        base = _median([g.bottom for g in row])
        for g in row:
            if abs(g.bottom - base) <= LINE_SNAP:
                _shift_edge(bnd, g.label, held, g.bottom, base, line_labels)
        for target, members in _clusters([(g.label, g.top) for g in row]):
            for g in row:
                if g.label in members:
                    _shift_edge(bnd, g.label, held, g.top, target, line_labels)


def regularise_lettering(bnd, labels: np.ndarray, fill_at, params: CurveParams) -> dict[int, int]:
    glyphs = glyphs_of(bnd, labels, fill_at)
    lines = find_lines(glyphs)
    snap_lines(bnd, lines)
    return {g.label: i for i, row in enumerate(lines, start=1) for g in row}
```

The node key (`x·1e6·7 + y·1e6`) is a stand-in: use whatever `topology._arcs_at` keys nodes by (read it) so `held` and `_move_node` agree on identity; if `_arcs_at` compares coordinates with a tolerance, do the same. The bottom snap runs before the top snap, each reading the glyph's box as measured before any move (the `Glyph` values), which is what keeps every move ≤ `LINE_SNAP`.

In `topology.py`: `Boundary` gains `lines: dict[int, int] = field(default_factory=dict)`; in `build`, after `_imprint(whole, params, rect_arcs)`: `lines = regularise_lettering(whole, labels, fill_at, params)` and the returned `Boundary(..., lines=lines)`. `curves.Cubic`'s `c1/c2` and the segments' `p0/p1` are numpy arrays mutated in place — check that `Line`/`CircArc` have `p0`/`p1` arrays (they do: `reverse_segments` swaps them) and that `CircArc` has no `c1` (`getattr(..., None)` handles it).

- [ ] **Step 3:** the tests, then the whole Python suite; then `diffcheck arcs` will FAIL until the Rust lands — expected; do not run it here. Commit: "lettering: a row of glyph regions of one ink shares one baseline and one cap or x-height, within LINE_SNAP, after the fit and before the bleed" + trailer.

---

### Task 5: Repeats — `reuse` at `REPEAT_TOL` for line glyphs

**Files:** `backend/studi0trace/engines/vexel/reuse.py` (`emit(pending, precision, first_id=1, loose=frozenset())`; `_same(a, b, pa, pb, tol)`; pairs with both indices in `loose` use `REPEAT_TOL = 0.25`, others `USE_TOL`), `engine.py` (`_emit` computes `loose` = the indices of pending shapes whose record's `member` holds a label in `bnd.lines`; a stroke string is never loose), tests in `tests/test_vexel_lettering.py` (two paths 0.2 px apart emit one `<use>` when loose, two paths when not).

- [ ] Steps: test (RED) → implement → the reuse tests (`tests/test_vexel_reuse.py` if present) and the new one → commit.

---

### Task 6: Stems — only if Task 3's ruling asks for it

**Files:** `lettering.py` (`equalise_stems(bnd, lines)`), tests.

- [ ] Rule: within a line, every fitted `Line` segment within 1° of vertical (after `regularity`'s snap) is a side; two sides of one region facing each other (their x-ranges overlap in y by more than half, the region lies between them: the arc's `normal` points from the region outward on both) form a stem of width = the horizontal distance between the sides' midpoints; widths are clustered as `_clusters` does with `STEM_SNAP` in place of `LINE_SNAP`; each stem of a cluster takes the cluster's median width, each side moving half the difference in x (≤ `STEM_SNAP`/2 = 0.15 = `SNAP_END_MOVE`), its two end points and the neighbouring segments' shared points moving with it (a cubic's arm too), nodes through `_move_node` unless held. Tests: three stems 3.0/3.2/3.3 wide → all 3.2; a stem 3.6 wide (over `STEM_SNAP` from the cluster) left alone; a side ending at a shared node left.
- [ ] Commit: "lettering: stems of one line take one width within STEM_SNAP" + trailer.

---

### Task 7: Rust port and parity

**Files:** `backend/vexel-rs/src/lettering.rs` (new; `pub fn regularise_lettering(bnd: &mut Boundary, labels: &Labels, fill_at: &FillAt, params: &CurveParams) -> BTreeMap<i32, usize>`), `topology.rs` (`Boundary.lines`, the call after `imprint`), `reuse.rs` (`emit(pending, precision, first_id, loose: &HashSet<usize>)`), `engine.rs` (`loose` from the records), `lib.rs` (nothing new: `_stage_arcs` runs `build`). Tests in `lettering.rs` mirroring Task 4's scenes (built from the same label maps and anti-aliased rgb).

- [ ] Port with the same order (glyphs by left edge then label; rows grown greedily; medians lower-middle; `_clusters` sorted by value then label; the bottom snap before the top snap; node identity as the Rust topology keys nodes). Then `cargo test --release lettering`, maturin, the Python lettering tests, and `.venv/bin/python -m tools.diffcheck arcs labels0 merge stroke_seeds` over the corpus — every item ok (the `arcs` tolerance is RMS 0.05 px: a snap applied in one engine and not the other fails outright on the vertex count or by far more than 0.05). Commit.

---

### Task 8: The gate, fixtures, baseline

- [ ] `tools/qloop.sh full` (background, tee'd); the text items' `wobble_deg_100px` and `outline_px` before → after; `<use>` counts on `sans-3-repeat-512` before → after (count `<use` in the trace SVGs). On any regression: diagnose to the stage (which glyph moved, by how much, which rule) and STOP for the ruling. Otherwise fixtures (`--only scorecard,api,auto`, `--force` as needed), `cargo test --workspace --release`, baseline, commit.

---

### Task 9: The logos, the spec's Status, CLAUDE.md

- [ ] Trace the three logos before (a throwaway worktree at this plan's base, Python engine) and after (Python engine at HEAD — same engine, as the previous plan's final review required), whole images; crop the taglines for the eye; artifact index before → after per logo and preset; by eye: are baselines straighter, repeated letters identical, anything newly wrong. Write the spec's Status (what was built, the rulings from the ledger including the deviations: baseline-only line test, `LINE_INK`, parity via `arcs`, which sub-rules were built and why; the gate in full; the logos). CLAUDE.md bullet after the regularity bullet: "A row of glyph regions of one ink is one word (`vexel/lettering.py`, `lettering.rs`, after `_imprint` and before the bleed): ≥ 3 regions in a row whose bottoms agree share one baseline and, per cluster of three, one cap or x-height, every move ≤ `LINE_SNAP` (0.6 px) and nodes moved through `_move_node` so neighbours follow; two glyphs of a line whose outlines agree within `REPEAT_TOL` (0.25 px) are written as one `<use>` (`reuse.emit(..., loose)`). `diffcheck arcs` holds it between the engines." Commit docs.
