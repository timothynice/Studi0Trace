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
