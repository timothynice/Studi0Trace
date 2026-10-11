# Soft-Disc Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A soft-edged disc inside a gradient (the Horizon logo's pale sun in its sunset sky) stays one region with its own radial fill instead of merging into the sky, when its boundary is closed, long and carries a colour step over more than half its length.

**Architecture:** One new test, `closed_soft_boundaries(labels, features, grad, detail)` in `merge.py` (Rust `merge.rs`), returns the set of adjacent pairs whose shared boundary is at least `SOFT_RING_MIN` pairs long, encloses the smaller region (its other neighbours hold at most `SOFT_RING_OPEN` of its outline), and at more than half of its pairs carries a feature-colour difference above `SOFT_STEP · detail` between the samples `SOFT_REACH` pixels to either side. `partition.rejoin_ramps` keeps such a pair apart and `refine.refine_merge` never joins it. Bench items for the case come first, references are frozen at the current engine, a diagnosis-and-survey task measures the test over every split pair of the three bench sets BEFORE the engines change (the controller rules on the test's final form from that survey), then Python, Rust, `diffcheck` (`labels0`, `merge`, and a new `refine` stage), the gate, fixtures, and the Horizon check.

**Tech Stack:** Python (numpy, scipy, skimage) reference engine; Rust port built with maturin; `tools/diffcheck.py`; the bench (`python -m bench`, `tools/qloop.sh`); resvg for the synthetic items.

**Spec:** `docs/superpowers/specs/2026-10-09-soft-disc-merge-design.md`

## Global Constraints

- Run from `backend/` with `.venv/bin/python`; rebuild the extension after every Rust change: `.venv/bin/maturin develop --release -m vexel-rs/Cargo.toml`.
- Constants in both engines with these names: `SOFT_RING_MIN` = 60, `SOFT_RING_OPEN` = 0.25, `SOFT_REACH` = 4, `SOFT_STEP` = 2.0 (a multiple of `detail`). Not parameters. (The survey task may show the controller a better `SOFT_REACH` or an added far-ridge test; any change is a recorded ruling before Task 4, never a tune at the gate.)
- Determinism: float sums in float64 raster order (`np.bincount` / a raster loop); the "smaller region" of a pair is the one with fewer pixels, ties to the HIGHER label; no `set`/`HashMap` iteration decides anything.
- Parity: `.venv/bin/python -m tools.diffcheck labels0 merge refine stroke_seeds` passes over the whole corpus (`refine` is new in Task 6).
- `tools/qloop.sh full` against references frozen in Task 2 (at the current engine, after the bench items): the soft-disc items at 512 must have `outline_px` ≤ 0.5 and the disc present as one region; the Horizon sun one shape with a radial fill by eye.
- After the engine's output changes: `.venv/bin/python -m tools.export_core_fixtures --only scorecard,api,auto` (with `--force` if the provenance's Python version differs, as in the previous plan), then `cargo test --workspace --release` at the repo root.
- Commits: a sentence on what and why, blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Never stage `.superpowers/`, `backend/bench/reports/` or the scratchpad. Tim's example images never enter git.
- Lessons carried from the thin-stroke plan: a gate regression is diagnosed to its stage before any rule moves; residuals on legitimate decisions are ruled on and recorded, never tuned away; crop-only traces can mislead (trace whole images).

## Review Focus

1. **A highlight on a shaded sphere** (a bright blob with a soft rim, enclosed by its body — the nail-polish cap): the test as specified cannot tell it from the sun, and keeping it apart would draw a seam through a focal radial. The survey (Task 3) measures it; Task 4's test `test_a_highlight_inside_its_body_stays_joined` pins whatever the ruling decides.
2. **A disc touching the image frame**: the frame is not a neighbour, so the enclosure share is taken over the pairs that exist; a half-disc at the edge still qualifies when the sky holds ≥ 75 % of its outline. Pinned in Task 4 (`test_a_disc_cut_by_the_frame_is_still_enclosed`).
3. **A glow (halo ring) between a core and a backdrop**: two neighbours each with about half its outline — not enclosed by either; the halo joins as today. Pinned in Task 4 (`test_a_halo_is_not_enclosed_by_either_side`).
4. **A boundary shorter than `SOFT_RING_MIN`** (a small soft dot, a highlight's neck): not a ring; joins as today. Pinned in Task 4 (`test_a_short_soft_boundary_is_not_a_ring`).
5. **A soft disc whose contrast fades over MORE than half its outline**: out of the spec's scope; it merges as today, and the item must not regress (the σ = 3 item with the fade on one side is the boundary case). Pinned by the gate and Task 4 (`test_a_disc_with_contrast_on_under_half_its_ring_is_joined`).

---

## File Structure

- `backend/bench/synth.py` — `soft_disc` templates (`_soft_disc_template(sigma)`), class `gradient` (three templates named `soft-disc-1`, `soft-disc-2`, `soft-disc-3`).
- `backend/bench/degraded.py` — one source (`soft-disc-2`).
- `backend/studi0trace/engines/vexel/merge.py` — constants + `closed_soft_boundaries`.
- `backend/studi0trace/engines/vexel/partition.py` — `rejoin_ramps(..., detail=6.0)` consults it.
- `backend/studi0trace/engines/vexel/refine.py` — `refine_merge(..., features=None, detail=None)` consults it; `engine.py` passes `prep.features, p.detail`.
- `backend/vexel-rs/src/{merge.rs, partition.rs, refine.rs, engine.rs, lib.rs}` — the port and `_stage_refine`.
- `backend/tools/diffcheck.py` — `refine` stage.
- Tests: `backend/tests/test_vexel_merge.py` (the new test), `test_vexel_partition.py`, `test_vexel_refine_merge.py`, `test_bench_corpus.py`, `test_bench_degraded.py`; Rust tests in `merge.rs`.
- Docs: the spec's Status, a `CLAUDE.md` bullet, `backend/bench/baselines/vexel.json`.

---

### Task 1: Bench items — the soft disc

**Files:**
- Modify: `backend/bench/synth.py` (gradient templates), `backend/bench/degraded.py` (SOURCES, at the end), `backend/bench/sentinels.txt`
- Test: `backend/tests/test_bench_corpus.py` (counts), `backend/tests/test_bench_degraded.py` (counts)

**Interfaces:**
- Produces: corpus items `gradient/soft-disc-{1,2,3}-{512,128,512-ds}`, degraded items `{nn2x,sharpen,small,combo}/soft-disc-2-*`.

- [ ] **Step 1: Counts first (RED)**

`test_bench_corpus.py`: `n_templates * 3 == 102` → `== 111`, `len(loaded) == 102` → `== 111`. `test_bench_degraded.py`: every `60` → `64`. Run both files: FAIL on the counts.

- [ ] **Step 2: The template**

In `synth.py`, after `grad_alpha_fade`:

```python
def _soft_disc_template(sigma: float) -> Callable[[random.Random], str]:
    """A pale disc with a soft edge (Gaussian σ `sigma` px at 512) inside a
    vertical sky gradient. The disc's radial fill runs from its own colour at
    the centre to the sky's colour at the disc's lower edge, so the contrast
    across its edge fades to nothing along its lower third and is full along
    the upper two thirds: the Horizon logo's sun (spec
    `2026-10-09-soft-disc-merge-design.md`). The vector truth is the circle
    with its radial fill; the blur is part of the truth render, as the shadow
    class's filters are."""
    def fn(rng):
        sky_top, sky_bottom, sun = "#F2A65A", "#F9DCC4", "#FFF4D6"
        # the sky's colour at the disc's lower edge (y = 346 of 512): the sun's rim colour
        t = 346 / 512
        rim = "#%02X%02X%02X" % tuple(
            int(round((1 - t) * int(sky_top[i:i + 2], 16) + t * int(sky_bottom[i:i + 2], 16))) for i in (1, 3, 5)
        )
        defs = (
            f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">{_stops([sky_top, sky_bottom])}</linearGradient>'
            f'<radialGradient id="sun">{_stops([sun, rim])}</radialGradient>'
            f'<filter id="soft" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="{sigma}"/></filter>'
        )
        return _svg(
            '<circle cx="256" cy="236" r="110" fill="url(#sun)" filter="url(#soft)"/>',
            defs=defs, background="url(#sky)",
        )
    return fn
```

and in `TEMPLATES["gradient"]` append `("soft-disc-1", _soft_disc_template(1.0)), ("soft-disc-2", _soft_disc_template(2.0)), ("soft-disc-3", _soft_disc_template(3.0))`.

- [ ] **Step 3: Generate, check the renders by eye**

```bash
cd backend && .venv/bin/python -m bench generate
git status --short bench/corpus | grep -v soft-disc   # only manifest.yaml
```

Open `bench/corpus/synthetic/gradient/soft-disc-2-512.png` with the Read tool: a pale disc, upper edge visible against the orange sky, lower edge fading into the pale sky. If the rim colour does not meet the sky at the bottom (a visible step along the lower edge), fix the `t` arithmetic before going on.

- [ ] **Step 4: Degraded source and sentinels**

`SOURCES` gains, at the END: `Source("soft-disc-2", "corpus", "corpus/synthetic/gradient/soft-disc-2.svg"),`; docstring count "Fifteen" → "Sixteen". `python -m bench.degraded generate`; `git status --short bench/degraded | grep -v soft-disc` prints only `manifest.yaml` and `SOURCES.md`. Append to `sentinels.txt`: `corpus gradient/soft-disc-2-512       # soft-edged disc inside a gradient: closed soft boundary`.

- [ ] **Step 5: Tests green, commit**

```bash
cd backend && .venv/bin/python -m pytest tests/test_bench_corpus.py tests/test_bench_degraded.py -q
git add backend/bench/synth.py backend/bench/degraded.py backend/bench/sentinels.txt backend/bench/corpus backend/bench/degraded backend/tests/test_bench_corpus.py backend/tests/test_bench_degraded.py
git commit -m "bench: three soft-disc items (a blurred pale disc inside a sky gradient, σ 1, 2 and 3 px) and a degraded source for the closed-soft-boundary loop

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: Freeze the references at the current engine

- [ ] **Step 1:** `cd backend && time tools/qloop.sh ref` (all three sets; ~10 min). Then `tools/qloop.sh sentinels` → gate ok.
- [ ] **Step 2:** Record the soft-disc items' reference metrics (score, outline_px, delta_e_mean, artifact_index, elements) from `bench/reports/keep-2026-10-05-wave-lockup/ref-corpus/results.json` and the degraded `soft-disc` items from `ref-degraded`; put the table in the task report. No commit.

---

### Task 3: Diagnose and survey — before any engine change

**Files:** scratchpad only (`<scratchpad>/soft-disc/`); nothing committed.

- [ ] **Step 1: Where the disc is lost.** Trace `gradient/soft-disc-{1,2,3}-512.png` and the Horizon logo (`$STUDI0TRACE_TEXT_EXAMPLES/3.png`) with `VEXEL_DUMP` (Python engine, `VexelParams()` and `VexelParams(min_region=16)`): from `seed_markers.txt`, `labels_merge.txt`, `labels_rescue.txt`, `labels_refine.txt` say per item and preset at which stage the disc stops being its own region (seed split never happens / `rejoin_ramps` joins it / `merge_regions` joins it / `refine_merge` joins it), with the region ids and pixel counts. The expected answer per the spec is rejoin and refine; if `merge_regions` is the stage, say so — Task 4 then gains a hook there (the controller rules).

- [ ] **Step 2: The boundary profile.** For the disc/sky pair before it is joined (the label map of the stage just before the join), measure along the boundary: the mean discontinuity, the share of ridge pairs (`merge.boundary_ridges`), and the colour difference between the feature samples 4 px and 8 px either side (the spec's `SOFT_REACH` = 4 and a far reach of 8), as the share of pairs above `2·detail`; and the enclosure share (the disc's boundary pairs with the sky over all its boundary pairs).

- [ ] **Step 3: The survey.** Write `<scratchpad>/soft-disc/survey.py`: for every item of corpus, heldout and degraded (Rust engine for speed, `VEXEL_DUMP` on), take the label map `rejoin_ramps` receives (dump `labels_watershed` temporarily in a scratch copy of the Python engine if needed — or run the Python's `initial_labels` pieces directly: `seed_parts` → `stroke_seeds` → `seed_markers` → `watershed`, then compute the pairs with `origin[a] == origin[b] > 0`) and the label map `refine_merge` receives (`labels_rescue.txt`), and list every pair that the spec's test would hold apart (length ≥ 60, enclosure ≥ 75 %, colour step at ±4 px > 2·detail over more than half) that the current engine joins. For each such pair: item, stage, the two regions' sizes, the boundary length, the step share at ±4 and at ±8, the ridge share, and a 3-line crop description (what the two regions are: a sun, a highlight, a glow's core, a shadow band, …) — render the crop and look. Also the count of pairs that would be held apart per item.

- [ ] **Step 4: Report.** A table of all qualifying pairs with the by-eye reading, and a recommendation: the spec's test as written; or with `SOFT_REACH` at 8; or with an added far-ridge test (the boundary's discontinuity at least `NECK_RIDGE` × the discontinuity 8 px either side); or an added rule for highlights. The controller rules before Task 4 and records it in the ledger and in this plan's Task 4 text.

---

### Task 4: Python — `closed_soft_boundaries` and the two hooks

**Files:**
- Modify: `backend/studi0trace/engines/vexel/merge.py` (after `boundary_ridges`), `partition.py` (`rejoin_ramps`, `initial_labels`), `refine.py` (`refine_merge`), `engine.py` (the two `refine_merge` calls)
- Test: `backend/tests/test_vexel_merge.py`, `test_vexel_partition.py`, `test_vexel_refine_merge.py`

**Interfaces:**
- Produces: `closed_soft_boundaries(labels: np.ndarray, features: np.ndarray, detail: float) -> set[tuple[int, int]]` (pairs a < b); `rejoin_ramps(labels, markers, origin, grad, features, detail=6.0)`; `refine_merge(..., rescued=None, features=None, detail=None)`; constants `SOFT_RING_MIN = 60`, `SOFT_RING_OPEN = 0.25`, `SOFT_REACH = 4`, `SOFT_STEP = 2.0` in `merge.py`. (Amend per the Task 3 ruling before dispatch.)

- [ ] **Step 1: Tests (RED)**

`tests/test_vexel_merge.py`, appended:

```python
from studi0trace.engines.vexel.merge import closed_soft_boundaries


def _disc_scene(radius=40, size=128, contrast_rows=None, l_sky=70.0, l_disc=90.0):
    """A disc of label 2 inside a sky of label 1, features L* `l_disc` inside and
    `l_sky` outside, except on `contrast_rows` (a slice of rows) where the disc's
    L* equals the sky's: the contrast fades there. Labels follow the circle;
    the step sits on the label boundary."""
    yy, xx = np.mgrid[0:size, 0:size]
    inside = (yy - size / 2) ** 2 + (xx - size / 2) ** 2 <= radius * radius
    labels = np.where(inside, 2, 1).astype(np.int32)
    feats = np.zeros((size, size, 4), np.float32)
    feats[..., 3] = 100.0
    feats[..., 0] = np.where(inside, l_disc, l_sky)
    if contrast_rows is not None:
        rows = np.zeros(size, bool)
        rows[contrast_rows] = True
        feats[rows[:, None] & inside, 0] = l_sky
    return labels, feats


def test_a_closed_soft_disc_is_a_soft_boundary():
    labels, feats = _disc_scene()
    assert closed_soft_boundaries(labels, feats, 6.0) == {(1, 2)}


def test_a_disc_with_contrast_on_under_half_its_ring_is_joined():
    # the lower 60 % of the disc matches the sky: the step holds on under half the ring
    labels, feats = _disc_scene(contrast_rows=slice(56, 128))
    assert closed_soft_boundaries(labels, feats, 6.0) == set()


def test_a_disc_with_contrast_on_its_upper_two_thirds_qualifies():
    labels, feats = _disc_scene(contrast_rows=slice(92, 128))
    assert closed_soft_boundaries(labels, feats, 6.0) == {(1, 2)}


def test_a_short_soft_boundary_is_not_a_ring():
    labels, feats = _disc_scene(radius=5)  # a digital disc's 4-connected outline is ≈ 8·r = 40 pairs < SOFT_RING_MIN (r = 8 would be 64)
    assert closed_soft_boundaries(labels, feats, 6.0) == set()


def test_a_halo_is_not_enclosed_by_either_side():
    yy, xx = np.mgrid[0:128, 0:128]
    r2 = (yy - 64) ** 2 + (xx - 64) ** 2
    labels = np.where(r2 <= 20 * 20, 3, np.where(r2 <= 40 * 40, 2, 1)).astype(np.int32)
    feats = np.zeros((128, 128, 4), np.float32)
    feats[..., 3] = 100.0
    feats[..., 0] = np.where(labels == 3, 95.0, np.where(labels == 2, 80.0, 60.0))
    soft = closed_soft_boundaries(labels, feats, 6.0)
    assert (1, 2) not in soft, "the halo's outline is half core, half backdrop: enclosed by neither"
    assert (2, 3) in soft, "the core inside the halo is enclosed by it"


def test_a_disc_cut_by_the_frame_is_still_enclosed():
    labels, feats = _disc_scene(radius=40)
    labels, feats = labels[:70], feats[:70]  # the disc's lower part is off the image
    assert closed_soft_boundaries(labels, feats, 6.0) == {(1, 2)}


def test_the_smaller_region_is_the_enclosed_one_and_ties_go_to_the_higher_label():
    labels, feats = _disc_scene(radius=40)
    # swap labels: the disc is label 1, the sky label 2; the disc is still the smaller
    labels = np.where(labels == 1, 2, 1).astype(np.int32)
    assert closed_soft_boundaries(labels, feats, 6.0) == {(1, 2)}
```

`tests/test_vexel_partition.py`, appended:

```python
def test_rejoin_ramps_keeps_a_closed_soft_boundary_apart():
    from studi0trace.engines.vexel.partition import rejoin_ramps
    yy, xx = np.mgrid[0:128, 0:128]
    inside = (yy - 64) ** 2 + (xx - 64) ** 2 <= 40 * 40
    labels = np.where(inside, 2, 1).astype(np.int32)
    markers = labels.copy()
    origin = np.array([0, 1, 1])  # both split from seed component 1
    feats = np.zeros((128, 128, 4), np.float32)
    feats[..., 3] = 100.0
    # a 30 ΔE disc blurred by σ = 4: at NECK_REACH (3 px) the discontinuity beside the
    # boundary is ~75 % of the boundary's own, so the existing ridge test reads a ramp
    # and rejoins; the samples 4 px either side still differ by ~20 ΔE > SOFT_STEP·detail
    feats[..., 0] = ndimage.gaussian_filter(np.where(inside, 95.0, 65.0), 4.0)
    grad = np.abs(ndimage.sobel(feats[..., 0])).astype(np.float32)
    out = rejoin_ramps(labels, markers, origin, grad, feats, detail=6.0)
    assert out[64, 64] != out[4, 4], "the disc stays its own basin"
    # the same disc 2 ΔE above its sky: under SOFT_STEP, joined as before
    feats[..., 0] = ndimage.gaussian_filter(np.where(inside, 67.0, 65.0), 4.0)
    grad = np.abs(ndimage.sobel(feats[..., 0])).astype(np.float32)
    out = rejoin_ramps(labels, markers, origin, grad, feats, detail=6.0)
    assert out[64, 64] == out[4, 4]
```

`tests/test_vexel_refine_merge.py`, appended (use the file's `_inputs`/`_fitted` helpers; a disc with L* 90 inside a flat L* 70 ground, blurred by σ = 2 so no boundary pair is a ridge):

```python
def test_a_closed_soft_disc_is_never_joined_by_the_refit():
    from scipy import ndimage
    yy, xx = np.mgrid[0:SIZE, 0:SIZE]
    inside = (yy - 80) ** 2 + (xx - 80) ** 2 <= 50 * 50
    l = np.where(inside, 230.0, 170.0)
    l = ndimage.gaussian_filter(l, 2.0)
    rgb = np.stack([l, l, l], axis=-1).astype(np.uint8)
    xs, ys, rgba255, grad = _inputs(rgb)
    labels = np.where(inside, 2, 1).astype(np.int32)
    params = FitParams(gradients=True, max_stops=4, tol=3.0)
    fills = _fitted(labels, xs, ys, rgba255, params)
    from studi0trace.engines.vexel.prepare import prepare
    rgba = np.concatenate([rgb, np.full(rgb.shape[:2] + (1,), 255)], axis=-1).astype(np.uint8)
    feats = prepare(rgba).features
    out, _, changed = refine_merge(labels, xs, ys, rgba255, grad, fills, params, edge_limit=3.6, features=feats, detail=6.0)
    assert out[80, 80] != out[2, 2] and not changed
    out2, _, _ = refine_merge(labels, xs, ys, rgba255, grad, fills, params, edge_limit=3.6)
    # without the soft test the union (one radial over a flat ground) may or may not fit; the point is the hook, not the fit
```

Replace the last two lines with an assertion only if the baseline behaviour is known: run the test without `features` first and record in the report whether the refit joins the pair (if it does, assert `changed` for the no-features call to pin that the hook is what prevents it; if it does not, drop the second call).

- [ ] **Step 2: Run (RED)** — ImportError / TypeError on the new arguments.

- [ ] **Step 3: Implement `closed_soft_boundaries`** in `merge.py` after `boundary_ridges`:

```python
# A soft boundary that is closed, consistent and long is evidence of a shape,
# however shallow: a pale sun with a soft edge (σ ≈ 2 px) inside a sunset's
# gradient has no ridge at `NECK_REACH` — a soft edge is as steep beside
# itself as on itself — and its contrast fades to nothing along its lower
# third where the two gradients meet, so `rejoin_ramps` joined what the seed
# split and `refine_merge` fitted one radial over the union within its bar.
# A boundary at least `SOFT_RING_MIN` pairs long that encloses the smaller of
# its two regions (that region's other neighbours hold at most
# `SOFT_RING_OPEN` of its outline; the image frame is no neighbour) and at
# more than half of whose pairs the feature colours `SOFT_REACH` pixels to
# either side differ by more than `SOFT_STEP · detail` is such a boundary:
# the two stages keep its regions apart. A glow's halo has two neighbours of
# about half its outline each and is enclosed by neither; a highlight's neck
# is short; a disc whose contrast is gone over more than half its ring is out
# of scope (`2026-10-09-soft-disc-merge-design.md`).
SOFT_RING_MIN = 60
SOFT_RING_OPEN = 0.25
SOFT_REACH = 4
SOFT_STEP = 2.0  # × detail


def closed_soft_boundaries(labels: np.ndarray, features: np.ndarray, detail: float) -> set[tuple[int, int]]:
    """{(a, b)}, a < b: the pairs whose shared boundary is a closed soft
    boundary (the comment above). Sums run horizontal pairs then vertical,
    each in raster order, in float64; the smaller region is the one with
    fewer pixels, a tie going to the higher label."""
    h, w = labels.shape
    k = int(labels.max()) + 1
    f = features.astype(np.float64)
    step2 = (SOFT_STEP * detail) ** 2
    keys, hits = [], []
    for axis in (1, 0):
        la, lb = (labels[:, :-1], labels[:, 1:]) if axis == 1 else (labels[:-1, :], labels[1:, :])
        m = la != lb
        ys, xs = np.nonzero(m)
        a, b = la[m].astype(np.int64), lb[m].astype(np.int64)
        if axis == 1:
            before = f[ys, np.maximum(xs - SOFT_REACH, 0)]
            after = f[ys, np.minimum(xs + 1 + SOFT_REACH, w - 1)]
        else:
            before = f[np.maximum(ys - SOFT_REACH, 0), xs]
            after = f[np.minimum(ys + 1 + SOFT_REACH, h - 1), xs]
        keys.append(np.minimum(a, b) * k + np.maximum(a, b))
        hits.append((np.sum((before - after) ** 2, axis=1) > step2).astype(np.float64))
    key = np.concatenate(keys)
    if key.size == 0:
        return set()
    uniq, inv = np.unique(key, return_inverse=True)
    cnt = np.bincount(inv).astype(np.int64)
    hit = np.bincount(inv, weights=np.concatenate(hits)).astype(np.int64)
    pa, pb = (uniq // k).astype(np.int64), (uniq % k).astype(np.int64)
    total = np.bincount(np.concatenate([pa, pb]), weights=np.concatenate([cnt, cnt]), minlength=k).astype(np.int64)
    size = np.bincount(labels.ravel(), minlength=k).astype(np.int64)
    smaller = np.where(size[pa] < size[pb], pa, np.where(size[pb] < size[pa], pb, np.maximum(pa, pb)))
    enclosed = cnt * 1.0 >= (1.0 - SOFT_RING_OPEN) * total[smaller]
    ok = (cnt >= SOFT_RING_MIN) & enclosed & (2 * hit > cnt)
    return {(int(x), int(y)) for x, y in zip(pa[ok], pb[ok])}
```

- [ ] **Step 4: The hooks.**

`rejoin_ramps(labels, markers, origin, grad, features, detail=6.0)`: before the union loop, `soft = closed_soft_boundaries(labels, features, detail)`; in the loop, `if (a, b) in soft: continue` (a soft ring is a step) — put it beside the existing three-part test (`... or (a, b) in soft`). `initial_labels` passes `detail`. Docstring: one sentence.

`refine_merge(..., rescued=None, features=None, detail=None)`: after `ridges = boundary_ridges(labels, grad)`, `soft = closed_soft_boundaries(labels, features, detail) if features is not None and detail is not None else set()`, and the `pairs` comprehension gains `and (a, b) not in soft`. `engine.py`'s two calls pass `features=prep.features, detail=p.detail`. Docstring: one sentence.

Apply the Task 3 ruling here if it amended the test (the controller edits this step's text before dispatch).

- [ ] **Step 5: Run** the three test files, then the whole Python suite (`VEXEL_BACKEND=python .venv/bin/python -m pytest -q -x`, minutes). A golden test that changes where a soft disc is now its own region is the intended change — update and report; anything else is a bug.

- [ ] **Step 6: Commit** — `merge.py`, `partition.py`, `refine.py`, `engine.py`, the three test files: "merge: closed_soft_boundaries keeps a long, enclosing, consistent soft boundary apart in rejoin_ramps and refine_merge, so a soft-edged disc inside a gradient stays its own region" + trailer.

---

### Task 5: Rust port

**Files:** `backend/vexel-rs/src/merge.rs` (constants + `closed_soft_boundaries` returning `HashSet<(i32, i32)>`), `partition.rs` (`rejoin_ramps(..., detail: f64)` consults it; `initial_labels` passes `detail`), `refine.rs` (`refine_merge(..., features: Option<&Image>, detail: Option<f64>)`), `engine.rs` (the calls pass `Some(&prep.features), Some(params.detail)`), `lib.rs` (`_stage_labels0`/`_stage_merge` unchanged in signature; any `rejoin_ramps` caller updated). Tests: `#[cfg(test)] mod soft_tests` in `merge.rs` mirroring the Python's `_disc_scene` cases (closed disc, under-half contrast, short ring, halo, frame cut, tie rule).

- [ ] **Step 1:** Tests (RED: compile error). **Step 2:** port — a raster loop over horizontal then vertical pairs accumulating `(cnt, hit)` per `(min, max)` key in a `BTreeMap`, totals per label, sizes via `labels::bincount`, the smaller/tie rule, then the three conditions; the colour test `Σ_ch (before − after)² > step2` in f64 channel order. **Step 3:** `cd vexel-rs && cargo test --release soft_tests`, clippy adds nothing, maturin rebuild, `pytest tests/test_vexel_merge.py tests/test_vexel_partition.py tests/test_vexel_refine_merge.py -q`. **Step 4:** commit (`merge.rs`, `partition.rs`, `refine.rs`, `engine.rs`, `lib.rs`).

---

### Task 6: Parity — the `refine` diffcheck stage

**Files:** `backend/tools/diffcheck.py` (stage `refine`, tolerance `("max", 0.0, 0.002)` with a comment: a union fit's RMS against a bar is a decision at a threshold on fills the two engines agree on to a colour level), `backend/vexel-rs/src/lib.rs` (`_stage_refine(rgba, h, w, labels: Vec<i32>, detail: f64) -> Vec<i32>`: given the label map, fit the fills as `_fills_for` does, run `refine_merge` with `edge_limit = 0.6·detail`, `max_attempts` 60, `rescued` None, `features = Some(&prep.features)`, `detail = Some(detail)`, return the labels canonicalised as the `merge` stage canonicalises them). Python side: the same inputs — the label map from `merge_regions` of `initial_labels` at `min_region=6, detail=6.0` (as the `merge` stage builds it), fills fitted per label with `FitParams(gradients=True, max_stops=VexelParams().max_stops, tol=max(2.0, 6.0 / 2.0))` on the core weights (`weights.interior`) exactly as `engine.py` fits them before its first `refine_merge` call (read those lines and copy the construction), `rgba255` and `xs, ys` as `engine.py` builds them.

- [ ] **Steps:** write the stage; `.venv/bin/python -m tools.diffcheck refine labels0 merge stroke_seeds` over the corpus — every item ok (a whole-region difference is a port bug); a parity test in `test_vexel_refine_merge.py` on the Task 4 disc scene comparing `refine_merge` with `_stage_refine` (skip without the extension); commit.

---

### Task 7: The gate, fixtures, baseline

- [ ] `cd backend && tools/qloop.sh full` (background, tee'd to the scratchpad). Report every REGRESSED line per set verbatim, the improvement lines for `soft-disc` items, and the soft-disc acceptance (outline_px ≤ 0.5 at 512; the disc one region: count the trace's elements over the disc's area by rendering an id map, or by eye). On any regression: diagnose to the stage with `VEXEL_DUMP` (which pair was newly held apart, what the two regions are, the step share, the enclosure share) and STOP for the controller's ruling. Otherwise: `export_core_fixtures --only scorecard,api,auto` (+`--force` if provenance complains), `cargo test --workspace --release`, `bench run --engines vexel --no-media --workers 6 --update-baseline --out <scratchpad>/soft-disc/baseline`, commit fixtures + baseline.

---

### Task 8: The Horizon sun, the spec's Status, CLAUDE.md

- [ ] Trace `$STUDI0TRACE_TEXT_EXAMPLES/3.png` at Balanced and Logo, before (a throwaway worktree at this plan's base commit, Python engine) and after (Rust); render the sun's area beside the source and LOOK: is the sun one shape with a radial fill; what changed around it; the artifact index of the whole image before → after. Write the spec's `## Status (date)`: what was built, the rulings (from the ledger), the survey's findings (which pairs qualify across the sets and what they are), the gate's result in full honesty (residuals named), the Horizon verdict. Add a CLAUDE.md bullet after the seed-area bullet: "A soft boundary that is closed, consistent and long is a step (`merge.closed_soft_boundaries`: ≥ `SOFT_RING_MIN` pairs, enclosing the smaller region but for `SOFT_RING_OPEN` of its outline, a colour step above `SOFT_STEP·detail` between the samples `SOFT_REACH` px either side at more than half its pairs): `rejoin_ramps` and `refine_merge` keep its regions apart, so a soft-edged disc inside a gradient stays its own region. A glow's halo is enclosed by neither side; `diffcheck refine` holds the refit merge between the engines." Commit docs.
