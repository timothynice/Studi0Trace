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
