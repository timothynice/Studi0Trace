# Thin-stroke seeding — design

Status: approved by Tim 2026-10-09 in chat, to be built after `2026-10-09-text-resetting-design.md`.
An engine quality loop: Python and Rust together, `tools/diffcheck.py`, core fixtures re-exported,
`tools/qloop.sh full` as the gate.

## Why

A 2–3 px stroke loses its seed. `seed_mask` (partition.py:130-145, partition.rs:158-172) dilates
the ridge by 1 px and drops every pixel within 2 px of that band whose gradient is at least
`g_seed_eff`, so a stroke narrower than about 5 px keeps only fragments of valley; `seed_markers`
(py:246-249, rs:224-230) then zeroes every component under `floor = max(min_region, 2)` — 6 under
Balanced, 16 under Logo. The watershed floods the unseeded stroke from its neighbours: a closed
counter's backdrop takes the ring ("0", "O", "D"), a bowl joins the stem's backdrop. On Tim's
three logos the 17–31 px taglines lose a letter in three, and the wave-lockup plan recorded the
same mechanism as D1 (`2026-10-05-wave-lockup-focus-loop.md:1049`). The text re-setter takes
recognised lines away from the trace, but a declined line (script lettering, gradient ink, no
font), a thin rule, a hairline icon stroke and any text the reader misses still go through here.

## What was already tried, and must not be repeated as is

Both attempts are in the plan at lines 1093–1142 and 1174–1269:

- **Pooling same-colour smooth fragments** (try9): 26 regressions in 8 corpus items and 44 in 17
  held-out, 14 of 16 degraded items worse. A transparent notch tip pooled to 6 px and was seeded
  (the notch closed); a JPEG ringing line 8 ΔE from its fill pooled to 6 px and became a speck.
- **A fixed seed floor of 6 under Logo**: worse on the focus asset itself (tip shards seeded,
  AI 44.4 → 57.3).

The lesson is that a lower floor or a pooled seed admits noise everywhere. The seed a thin stroke
needs is specific to a thin stroke: an elongated, consistent-colour, two-sided ridge structure.

## Hypothesis

Seed a thin stroke as a stroke. After `seed_mask`, take the pixels the band exclusion removed
that lie between two ridge pixels across their own gradient direction within `STROKE_SEED_W`
(5 px) — the medial pixels of a thin feature, not the halo beside a thick edge — and form
components of them. A component is a **stroke seed** when it is at least `STROKE_SEED_LEN`
(12 px) long along its medial axis, its width never exceeds `STROKE_SEED_W`, its colour is
consistent (feature spread under `detail`) and it differs from the colour on both sides of the
ridge by more than `detail`. A stroke seed is added to the markers at its own label whatever its
area, before the `floor` test, and it is exempt from `_absorb_small`'s re-flood. Everything else
is unchanged: the floor stays, pooling is not reintroduced, a notch tip (short, one-sided) and a
ringing line (same colour as its side within `detail`, or one-sided) do not qualify.

Closed counters follow: a ring seeded as a stroke keeps its counter's backdrop as a separate
basin, since the ring's seed now separates the two.

## Protocol

1. **Bench items first.** Add to `bench/synth.py` a `text` class: a template that renders a
   tracked all-caps line and a lowercase line in a bundled OFL font (the text re-setter's fixture
   fonts) at 2, 3 and 4 px stroke widths on white and on a pale gradient, with the glyph outlines
   as vector truth, at 512 and 128 px plus `-ds`. Add the equivalent to `bench/degraded.py`'s
   sources so sharpen/nn2x/combo cover it. Freeze references with `qloop.sh ref` before the
   change so the new items are gated too.
2. **Diagnose on the new items and on Tim's three logos** (kept out of git, under the scratchpad):
   dump `seed_mask` and `seed_markers` output (`VEXEL_DUMP`) and count per glyph the seed pixels
   kept and the glyphs with no marker. Confirm the mechanism before touching anything.
3. **Implement in Python** (`partition.py`, new `stroke_seeds`), then port to Rust
   (`partition.rs`), with `diffcheck labels0` holding the two to the bit and a new `diffcheck`
   stage `stroke_seeds` on the seed map alone.
4. **Gate**: `tools/qloop.sh full` passes on corpus, held-out and degraded; the new text items
   show every glyph present (`pinholes`, `slivers`, `thin_strokes` 0, `outline_px` ≤ 0.3 at 512)
   and the artifact index of the three logos' tagline crops (measured by `bench artifacts` on
   the crop) falls. Then `export_core_fixtures --only scorecard,api,auto` and
   `cargo test --workspace --release`; `tests/test_vexel_parity.py` gains a tie rule if the
   stroke seed introduces one (component order by raster index of the first pixel).
5. **Record** in `backend/bench/baselines/` and the plan's status section what moved.

## Constants

`STROKE_SEED_W` 5, `STROKE_SEED_LEN` 12, colour tests at `detail`; all three in both engines with
the same names. They are not parameters: no UI control, no `Params` field.

## Out of scope

Regularising the fitted letters (that is `text-as-shapes`); strokes thinner than 1.5 px, which the
stroke stage already draws as strokes when they survive, and which `upsample` handles below 192 px.

## Status (2026-10-11)

Built as `docs/superpowers/plans/2026-10-11-thin-stroke-seeding.md`: `stroke_seeds` in both engines,
`diffcheck stroke_seeds` exact on 114/114 corpus items, the bench `text` class
(`bench/synth.py`) and two degraded sources, the references frozen before the change. The spec's
literal gate is not met, and the stage does not repair the three logos' taglines; Tim decides
whether the trade ships.

**Diagnosis before the change (Task 4).** Every marker-less glyph had only sub-floor seed
fragments (41 item/preset rows, no exception). Real taglines lose markers on 19 % of their glyphs
at Balanced and 61 % at Logo. The glyphs were not missing from the trace: the watershed handed
them to the ground and `rescue_features` clawed them back as residual-shaped patches, so the
stage's benefit is the shape of those glyphs, not their presence.

**Where the realisation differs from the hypothesis.** Each is a ruling recorded in the SDD ledger.

1. "Between two ridge pixels" runs along the four pixel axes (0°, 90°, 45°, 135°) with the
   narrowest axis deciding, not along the structure-tensor orientation: the orientation flips at
   junctions, which is what fragmented the valley seeds in the first place.
2. Length is pixels over mean width (an integer test), not a skeleton.
3. Two tests the spec did not name: the two sides beyond the ridge must agree within `detail`
   (without it a sharpened edge's rim, a JPEG ringing line and a drop shadow's band qualify: each
   is one colour, long, and differs from both of its sides), and a component touching a kept seed
   of its own colour (its colour read off the seed's pixels outside the stroke candidates) is left
   to that seed's flood (a wedge tip, the ground between two letters).
4. Three constants beyond the spec's two, all from the gate. `STROKE_SEED_ALPHA` 50: a candidate
   is at least half opaque (the inpainted colour under a transparent pixel means nothing and was
   seeded as strokes in `sharpen/thin-mark-512`'s ground). `STROKE_SEED_CONTRAST` 2: the stroke
   stands 2·detail above both sides (JPEG ringing streaks of 8-11 ΔE in a soft field qualified at
   `detail`). And a seed stops `STROKE_SEED_W` short of its ends (an end pixel: its component's
   hits on the radius-2 ring form at most one run of at most 5 and on the radius-3 ring at most one
   run; rings and bends have no ends), with four-connected remnants under `STROKE_SEED_W` dropped:
   a seed reaching into a junction gave the stem of `thin-mark` a stepped foot that
   `topology._extend_wedges` read as a 63° taper, walking a 1,647 px sliver round the triangle. A
   stroke therefore needs about 3·`STROKE_SEED_W` px to keep a seed.
5. The 2 px limit is wider than "crisp": an anti-aliased 2 px stroke's two ink pixels are both
   ridge (and differ from each other by more than `detail` at most phases), so it has no
   one-colour medial pixel and is not seeded. All four `sans-2` items are bit-identical to their
   references.

**Gate** (`tools/qloop.sh full` at 930737d). Heldout is clean; corpus and degraded are not. Three
residual regressions, each on a legitimate seed and a downstream sensitivity, are left for their
own loops:

- `logo/thin-mark-512` seam_ppm 11399 → 13961: the seeded 2 px ring's anti-aliased rims stay
  transparent because the rescue's `boundary_band` excludes region boundaries, so three of its
  four stroke groups fit width 1.96-1.98 and split into pieces.
- `text/sans-3-gradient-512` and `-512-ds`: the seeded O claims 11 one-pixel edge nubs, its stroke
  fidelity moves 0.1301 → 0.1240 across the 0.13 gate and it is drawn as two open centrelines
  (artifact index +0.5).
- `degraded/sharpen/thin-mark-512` artifact index 2.53 → 4.42: wobble where the stem meets the
  triangle's apex, the wedge extension.

Improvements: 31 text lines (sans-4 outline_px 0.42 → 0.13 and 0.50 → 0.19 at 512, 0.75 → 0.33 and
0.63 → 0.38 at -ds; sans-3-white-512 0.43 → 0.25), `small/sticker-176` artifact index 28.5 → 5.0,
`noto/u1f469-200d-1f33e-512-q75` 43.1 → 40.1. Text acceptance bar (pinholes = slivers =
thin_strokes = 0, outline_px ≤ 0.3 at 512): 2 of 12 (`sans-4-gradient-512`, `sans-4-white-512`);
the sans-3 items keep their pre-existing 1-4 slivers, the sans-4 -ds items miss on outline alone,
the sans-2 items are unchanged.

**Tim's logos.** The three images traced whole at Balanced and Logo, before (`d7de82d`, the
Python engine) and after (this branch, the Python engine again, `VEXEL_BACKEND=python`; the
Rust engine's figures are the third column of each preset, for reference), and their taglines
compared by eye (`trace_examples.py`'s tagline boxes). The engines are compared like with like:
the two differ end to end wherever an allowed tolerance sits at a decision threshold, so only the
Python-before to Python-after change is the stage's. The taglines are not repaired. Rendered,
they differ by 0.00 % of their pixels at both presets, except the "FIND WHAT MATTERS" line of
image 2 at Logo (0.15 %, mean error 6.18 → 6.22 with the Rust engine, 4.64 → 4.66 by the
Python-after render's measure); the letters that were missing or broken before (the O's of
TOMORROW, the first I of IDEAS and the O of OUTSIDE, the L, F and E of LIFE, the B and E of
BETTER) are missing or broken in the same way after. Whole-image artifact index, Python before →
Python after (Rust after):

| logo | Balanced | Logo |
|---|---|---|
| 1 | 32.51 → 32.51 (32.51) | 23.75 → 23.75 (23.75) |
| 2 | 113.77 → 113.77 (113.76) | 61.58 → 61.92 (61.92) |
| 3 | 67.65 → 68.14 (68.14) | 54.31 → 54.37 (54.37) |

The Python engine's after traces equal the Rust engine's to the second decimal in five of six
cells (the sixth, image 2 at Balanced, is 0.01 apart, the engines' noise), so the moves are the
stage's: image 1 at both presets and image 2 at Balanced are byte-identical to the before trace.

The stage fires on the whole images (13, 871 and 129 stroke-seed pixels in 1, 2 and 3) but
almost none of it lands on the taglines: their stems are one to two pixels at half-darkness (a
profile of 208, 21, 91 across a stem is typical), the case item 5 leaves unseeded. Tracing a
tagline crop on its own seeds none of 2's and 3's lines (13 pixels in 1's). The changes elsewhere
are a few hundred pixels per logo, mostly neutral; the clearest are small specks gone from the
edge of the dark mass in 3 at Logo, and a lighter smear on the left stem of the serif N in image 2
at Logo (error 9.35 → 11.28 over its 42×44 px with the Rust engine; the Python-after trace has
the same smear, 7.40 → 8.72 by the render measure above). Nothing new is wrong at the taglines: no seam, no
sliver, no filled bowl. The artifact index of an SVG cropped to a tagline box counts the shapes
outside it too, so the whole-image index is the honest figure; the crop-only trace of the box is
byte-identical before and after for every crop.

**Not pursued, measured and rejected during the loop.** A cap on wedge runs (`WEDGE_MAX_RUN` ≈ 128
fixes the mechanism but worsens `combo/thin-mark-512` and `combo/u2049-512`), tightening the wedge
trigger (40-110 items move, more worse than better), pooling or a lower floor (the spec's own
warning, confirmed).
