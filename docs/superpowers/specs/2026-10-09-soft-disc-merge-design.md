# A soft disc inside a gradient — design

Status: approved by Tim 2026-10-09 in chat, to be built after `2026-10-09-thin-stroke-seeding-design.md`.
An engine quality loop with the same ceremony: Python and Rust together, `diffcheck`, fixtures,
`qloop.sh full`.

## Why

In Tim's "Horizon Creative" logo a pale sun (a soft-edged radial disc, 220 px across) sits inside
the sunset's linear gradient. Vexel keeps a 60 px sliver of it and merges the rest into the sky:
to a person it is obviously a circle with a gradient that fades toward the backdrop at its lower
edge. The same shape with a crisp edge (the gold sun of the "Adventure" logo) is fine.

## Mechanism

The disc's edge is soft (σ about 2 px) and its contrast against the sky falls to nothing along its
lower third, where the two gradients meet. Two stages lose it:

- `seed_markers` (partition.py:206-304) groups a component's eroded pieces by mean colour within
  `detail`; the disc and the sky differ by more than `detail` over most of the disc, but a soft
  edge has no neck for `NECK_ERODE` to part, so the disc's seed is split from the sky's only where
  its contrast is highest, and `rejoin_ramps` (py:318-406) rejoins what it parts: the boundary is
  not `NECK_RIDGE` steeper than the pixels 3 px either side, since a soft edge is as steep beside
  itself as on itself.
- `refine_merge` (refine.py:41-152): a pair with a smooth region is a candidate when the mean
  boundary gradient is at most `edge_limit` (0.6·detail) or the boundary is a ridge at no more
  than half its pairs; the disc's soft boundary passes both, and the union of a radial over a
  linear fits within the bar (1.15 × the worse of the two) because the disc is a small share of
  the union's core.

## Hypothesis

A soft boundary that is *closed, consistent and long* is evidence of a shape, however shallow.
Add one test, `closed_soft_boundary`, used by both `rejoin_ramps` and `refine_merge`: a boundary
between two regions whose length is at least `SOFT_RING_MIN` (60 px) and which encloses one of
them (the smaller region has no other neighbour along more than `SOFT_RING_OPEN` (25 %) of its
outline) is kept as a step when the colour difference across it, measured `SOFT_REACH` (4 px)
either side, exceeds `SOFT_STEP` (2·detail) over more than half the boundary's length, even
though no pair is a ridge. The union is then refused in `refine_merge` and not rejoined in
`rejoin_ramps`. The disc's lower third, where the step fades, is the minority; the upper two
thirds carry it. A glow (a halo with no inside) fails the enclosure test; a highlight ring's necks
(the case `NECK_RIDGE` was written for) are short and fail the length test.

Then the outline: `_place` already softens a chain by its measured blur (`_soften`); a disc kept
as its own region draws with a soft, smooth edge, as the gold sun does with a crisp one. A radial
fill on the disc is already permitted beside the sky's linear (fills.py:291; the fit is per
region).

## Protocol

1. **Bench items.** A `synth.py` template `soft_disc`: a radial disc with edge σ of 1, 2 and 3 px
   inside a linear gradient, with contrast falling to zero along one side, at 512 and 128 px, with
   the vector truth (the disc as a circle with its radial fill, inside the gradient rect); the
   degraded set gets it too. Freeze references first.
2. **Diagnose** with `VEXEL_DUMP` on the items and on the Horizon logo: which stage loses the
   disc (seed split, rejoin, merge, refine), the measured boundary gradient and step profile.
3. **Implement in Python** (`merge.py` gains `closed_soft_boundary`; `partition.rejoin_ramps` and
   `refine.refine_merge` consult it), then Rust (`merge.rs`, `partition.rs`, `refine.rs`), with
   `diffcheck` stages `labels0`, `merge` and `refine` holding the two to the bit.
4. **Gate**: `qloop.sh full` passes; the soft-disc items' `outline_px` ≤ 0.5 at 512 and the disc
   present as one region; the Horizon logo's sun drawn as one shape with a radial fill (manual
   check, out of git). Fixtures re-exported, `cargo test --workspace --release`.

## Constants

`SOFT_RING_MIN` 60, `SOFT_RING_OPEN` 0.25, `SOFT_REACH` 4, `SOFT_STEP` 2·detail; in both engines,
not parameters.

## Out of scope

Soft shapes that are not enclosed (a glow fading on all sides into a gradient stays with the
gradient), and discs whose contrast is under `SOFT_STEP` over more than half the ring.
