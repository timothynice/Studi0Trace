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

## Status (2026-10-11)

Measured, refuted as written, and not built. In: the bench items `gradient/soft-disc-{1,2,3}`
(`bench/synth.py`: a blurred pale disc inside a sky gradient, σ 1, 2 and 3 px, its contrast fading
to nothing at the lower edge; 512, `-ds` and 128 variants) and a degraded source, the references
frozen before any change, a diagnosis and a survey, and a spike on a revised hypothesis. Not built:
`closed_soft_boundaries` and the `rejoin_ramps` / `refine_merge` hooks. Tasks 4–8 of
`docs/superpowers/plans/2026-10-11-soft-disc-merge.md` were cancelled on the diagnosis. The survey
(`task-3-report.md`) and the spike (`spike-ridge-arc.md`) are out of git; what they found is here.

**Why the mechanism as written is refuted.** The disc is never split from the sky at the seed stage,
on all three discs and on Horizon's sun, at `VexelParams()` and at `min_region=16` alike.

- One marker covers the image (260–262 k px, origin 0: the component was seeded whole); on Horizon
  the sun stays inside the sky's group (purity 0.23). The ridge round the disc follows its outline
  only where contrast is high and leaves a gap of 100–160° along the fading lower edge (σ 1:
  41°–140°; σ 2: 28°–153°; σ 3: 7°–171° plus a dozen more; Horizon: 45°–78° and 127°–154°), far
  wider than any neck `NECK_ERODE` parts. After erosion the disc and the sky are one piece.
- So `rejoin_ramps`, `merge_regions` and `refine_merge` never see a disc/sky pair, and the two
  hooks cannot change these traces. The "second element" in today's traces is the rescue's blob
  (1.5–4.5 k px cut from the disc's centre; Horizon's 1 580 px cap), which `refine_merge` even joins
  back at σ 1.
- Given a true split (the circle carved into its own label), `merge_regions` and `refine_merge`
  already keep the disc apart with no hook (38 024 px on all three discs, 14 424 on Horizon;
  `changed = False` on all four).
- The spec's thresholds fail the targets themselves. The step at ±4 px exceeds 12 on only 16–40 %
  of the synthetic rings (the top third carries it, not "the upper two thirds"; the rim colour
  equals the sky's at the disc's lower edge), and Horizon's sun is 52–73 % enclosed, since the
  mountains cover its lower part.

**The survey** (315 items: corpus 131, heldout 120, degraded 64; the pairs `rejoin_ramps` and
`refine_merge` join today). The test as written (60 pairs, 75 % enclosed, step at ±4 over 12 on more
than half) would newly hold apart 3 pairs: two crisp real shapes (the boot's collar stripe, a
stem of the "T" of OUTSIDE) and one 106 px glow shard. Reach 8 would hold apart 10: nine glow and
shading joins (among them the glow-512-ds core/halo join `refine_merge` exists for) and the
boot stripe. Reach 4 with a far-ridge test at 8 would hold apart 2, both correct. No sun or disc
appears in any of them. As a guard the test is harmless and pointless, and the loss is at the seed
stage.

**The revised hypothesis, and what a spike showed.** Close a long circular ridge arc at the seed
stage: a ridge chain of at least 60 px fitting a circle (RMS under 1.5 px) over at least 200° gets a
synthetic ridge band along its circle across the gap, so the disc's interior seeds as its own
component. Run in Python only, on a copy of the package, rule off matching the Rust references:

- σ 1 and σ 2 (512 and `-ds`) each become one region of 38 k px with a radial fill; ΔE mean
  1.5–1.8 → 0.06–0.09. Horizon's sun becomes one whole disc at both presets where today it is a
  cap sliver. The later stages keep the disc, as the diagnosis predicted. σ 3 has no ridge chain
  (67 ridge px on its ring) and is out of reach of any ridge-based rule.
- Piece 1, the outline over the gap. Where there is no contrast the boundary wanders: wobble 0 → 8–24
  at 512, 167–190 at 128; artifact index 0 → 10–48 on the disc items. The closing circle has to be
  carried into the fit (an imprint of the circle on the ring for that stretch, as
  `topology._imprint` does for a primitive, or the watershed steered onto it); without that the
  discs fail the artifact gate.
- Piece 2, selectivity. The geometric rule alone closes 65 items, 11 of them the discs, and visibly
  damages six: the glow (a hard octagonal ring inside glow-512-ds), u1f4a0 (a cut band across the
  bottom petal), old-key-q75 (a notch at the collar), u1f61b-q75 (a bump on the left eye), u2640
  (jagged counter edges) and thin-mark (the inner ring breaks into dashes); it closes the rest
  through occluders and along the round part of non-round shapes. Requiring the covered arc's edge
  to be soft (median ‖F(±1)‖ / ‖F(±4)‖ < 0.8) and a step (the ±8 to ±4 ratio < 1.4) clears every
  clean crisp shape, the glow and the spurious hits, and still closes 24 items. But JPEG q75 and
  nn2x ringing make a crisp edge read as soft (ratios 0.50–0.80 against the discs' 0.36–0.76), and 13
  such crisp items are still closed: 4 regress, 3–4 improve (mx-claus-q75 gets its pom-pom back,
  u1f307-q75 its sun).
- Every subset gate fails (corpus, heldout, degraded) until both pieces are solved.

**Next, if it is taken up.** A new spec, not an edit of this one: the seed-stage rule with its
constants (`RING_ARC_MIN` 60, `RING_INLIER` 1.5, `RING_ARC_DEG` 200°, largest gap 180°, `RING_GAP_MIN`
10°, radius from 20 to a quarter of the long side, a 2 px link, a 1.0 px band), the carried circle
(piece 1) and a discriminator that separates a soft disc from a JPEG-ringed crisp edge (piece 2:
the 10–90 % width of the step, or the softness on a denoised image, or no ridge of another region
in the gap angles). Plus a deterministic RANSAC seed both engines share, and the full `qloop.sh full`
and a survey of every component with an arc ridge, since the rule moves partitions widely.

**Acceptance note.** `outline_px` is null on the soft-disc items: the truth carries a blur filter,
so the vector truth's outline is not a crisp edge to measure against. The spec's `outline_px` ≤ 0.5
gate cannot apply; a loop on them is judged on score, `delta_e_mean`, the disc being one element
with a radial fill, and the eye (`edge_f1` falls where the truth is blurred and a vector edge counts
as a false one, so ΔE is the honest figure).
