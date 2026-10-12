# Text as shapes — design

Status: approved by Tim 2026-10-09 in chat, to be built last, after `2026-10-09-soft-disc-merge-design.md`.
An engine quality loop with the same ceremony.

## Why

The text re-setter declines a line it cannot set: script lettering ("Awaits", "Creative",
"Explore" in Tim's logos), gradient ink, a string the ink disagrees with, no font that fits. Those
lines, and any text the reader never saw, are traced as shapes. Once thin-stroke seeding keeps
their letters, what remains wrong with them is regularity: baselines and cap heights that wander
by a pixel, stems of one letter thicker than the next, two "e"s that are not the same shape.
A person reads that as a badly drawn word. The fix is to treat a row of glyph regions as one
thing after the fit.

## Hypothesis

A new stage after `regularity` and before `reuse`, `vexel/lettering.py` and
`vexel-rs/src/lettering.rs`, with `diffcheck lettering`:

1. **Lines.** Group regions of one colour whose bounding boxes overlap in y by more than half
   their height, lie in a row (gaps under 2 × the median box width) and number at least 3. The
   group is a **line** when the regions' heights agree: the median of the top edges and of the
   bottom edges each fit at least two thirds of the regions within `LINE_SNAP` (0.6 px). Script
   lettering, whose glyphs join into one region, never forms a line and is left alone.
2. **Baseline and cap.** The bottom edge of every region whose bottom is within `LINE_SNAP` of
   the median bottom is moved onto it, by moving the nodes on that edge in y; the same for the
   top edges against the median top (x-height letters snap to their own median when at least
   three share it). Nodes never move more than `LINE_SNAP`; a node a neighbour also holds is
   left, as `regularity` leaves nodes.
3. **Stems.** Straight vertical runs (from `curves.line_runs`) within one line are clustered by
   width in pairs (the two sides of a stem, measured across); clusters whose widths agree within
   `STEM_SNAP` (0.3 px) take the median width, each side moving half the difference, along its
   normal, ends held as `SNAP_END_MOVE` allows.
4. **Repeats.** Two regions of one line whose outlines agree within `REPEAT_TOL` (0.25 px) after
   translation are made identical (the earlier one's outline is used for both), so `reuse.py`
   writes them once as `<use>`. The existing 0.1 px rule in `reuse` stays for everything else.

All moves are bounded by their snap constants, which are below the placement's own accuracy, so
the fidelity metrics cannot fall by more than the snap; the artifact scorecard's wobble and the
repeat count should fall.

## Protocol

1. **Bench items.** The `text` class from thin-stroke seeding already carries lines with vector
   truth; add one item of lowercase with repeated letters ("letters settle better") at 3 px
   strokes. Freeze references.
2. **Diagnose** on those items and the taglines of Tim's logos (out of git): the per-region
   baseline scatter, cap scatter, stem width scatter and the repeat count before.
3. **Implement in Python, then Rust**, `diffcheck lettering` to the bit, `tests/test_vexel_parity.py`
   for any tie (regions ordered by left edge then label).
4. **Gate**: `qloop.sh full` passes; on the text items wobble falls and `outline_px` does not
   rise by more than 0.02; on the three logos' tagline crops the artifact index falls (manual).
   Fixtures re-exported, `cargo test --workspace --release`.

## Constants

`LINE_SNAP` 0.6, `STEM_SNAP` 0.3, `REPEAT_TOL` 0.25; both engines, not parameters.

## Out of scope

Reading the text (no OCR in the engine), recognising fonts, rotated or curved baselines, lines of
fewer than three regions.

## Status (2026-10-11)

Measured, and not built. The bench item `text/sans-3-repeat` is in (lowercase "letters settle
better" at 3 px stems, `bench/synth.py`, references frozen), and a scatter diagnosis ran on it, on
the rest of the `text` class (the 512, `-ds` and 128 variants) and on the taglines of Tim's three
logos. The lettering stage is not built: Tasks 4–9 of `docs/superpowers/plans/2026-10-11-text-as-shapes.md`
were cancelled on the diagnosis. The diagnosis report (`task-3-report.md`) is out of git; what it
found is here.

**Why.** The scatter the stage would remove is below the placement's own accuracy, so each rule
either moves nothing or moves the trace away from the truth.

- **Baseline.** The trace's bottoms scatter 0.05–0.13 px (std) on clean lines, up to 0.39 on the
  downsampled items. Most of it is the font's own overshoot: round glyphs sit 0.13–0.32 px below the
  flat ones in the truth, and the trace reproduces that. Snapping to the median erases the
  overshoot, so the error against the truth grows: 0.082 → 0.129 px at Balanced, 0.049 → 0.090 at
  Logo, 0.111 → 0.157 on the `-ds` items. Flat-bottomed glyphs already agree to 0.00–0.04 px
  (max 0.09). The misses beyond 0.6 px (9 of 131 bottoms) are lost feet and ascenders, which
  `LINE_SNAP` cannot reach.
- **Tops.** Snapped against the truth, 0.108 → 0.096 px at Balanced and 0.143 → 0.126 at Logo: a gain
  of about 0.015 px. The one clear case is the 2 px x-height tops of the `-ds` items (0.32–0.36 →
  0.15–0.17), on lines the rule does not detect at Balanced.
- **Stems.** Filled stem pairs, the only thing the stem rule reaches, are few (0–6 per line on the
  synthetic items, 0–9 on the logos) and already agree to std ≤ 0.17 px (max 0.34; logos ≤ 0.13,
  max 0.21). The real stem scatter is in glyphs drawn as centreline strokes: 1.77–2.60 px against a
  true 3.0 at 3 px, 1.00–1.97 against 1.84–2.00 at 2 px. The rule works on line runs of the arcs and
  cannot touch those. On sans-3 the U's outline arcs measure 3.00 and 3.01 px while it is drawn as a
  2.60 px stroke.
- **Repeats.** On "letters settle better" (33 same-letter pairs) 1–2 pairs fall within 0.25 px on
  `reuse`'s own test (2 at Balanced, 1 at Logo); with a best-fit shift 11–19 do, but at Balanced
  only 1 glyph in 33 is one filled region and so eligible. The copies differ by a median 0.24–0.35 px
  after the best shift, about what each is off from the truth (a median 0.36 / 0.64 px), so making
  them identical makes neither more accurate. The gain is a few `<use>` elements and no visible
  change, at up to 0.25 px of outline shift per copy.
- **Tim's logos.** The trace sits 0.08–0.19 px from the source per glyph on intact glyphs, and the
  source itself scatters 0.06–0.25 px (the images wander by themselves); 0–4 glyphs per line are
  more than 1 px off, all lost pieces. The line test finds 0 of 6 tagline lines at Balanced (each
  glyph is 2.2–3.4 regions with gradient fills) and 6 of 6 at Logo, where 30–70 % of the glyphs are
  strokes. The 128 items, traced at 2×, are worse: 34 of 95 bottoms within 0.15 px of the truth, a
  snap helps their baselines (0.30 → 0.25) and hurts their tops (0.33 → 0.37), and 7 of 14 lines are
  detected.

The stage would not change anything visible on the taglines: its moves are 0.1–0.3 px on 25–35 px
capitals that are already as regular as the source. The simulation behind these figures ignores the
line test and the rule that a shared node is left alone, so it is an upper bound on what the stage
could move.

**What is visibly wrong with traced text instead.** Gross faults, not regularity:

- Letters lost: the R, O and D of "BETTER OUTSIDE" (image 2 at Logo, ground-coloured blobs that
  vanish), the D of "IDEAS" at both presets, the d's ascender on the synthetic "days" at 3–4 px, the
  foot of the 2 px "t".
- Counters filled: the A and the B of image 1, the A of "IDEAS" (a filled gradient triangle).
- Glyphs drawn as strokes up to 2× too thin (0.94–1.96 px against about 2.0 px in the source) and
  leaning up to 9° (3–7 near-vertical runs per line lean more than 1°; the I and L of "LIFE").

These point back to the seed and rescue stages (the 2 px case that `2026-10-09-thin-stroke-seeding-design.md`
records as its limit, where a seed for soft strokes is the candidate) and to the stroke stage's width
and tilt. Each would be its own spec; none is part of this one.

**Deviations recorded in the plan, for the record.** Neither was built, so neither is tested.

1. The line test would have rested on bottoms, not on tops and bottoms alike: the spec's "median top
   fits two thirds" fails on its own lowercase example (7 x-height tops of 11 letters in "brighter
   days").
2. Parity would have been held by `diffcheck arcs`, which runs `topology.build` in both engines on one
   label map, rather than by a separate `lettering` stage in `diffcheck`.
