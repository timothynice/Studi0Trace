# Text in the inspector, instant toggles, source layering — design

Status: approved by Tim 2026-10-10 in chat ("yes, and build this autonomously from here until done").
Follows `2026-10-09-text-resetting-design.md`, which this changes in three places: where the Text controls
live, how a trace relates to the painted-out lines, and where a line's glyphs sit in the output.

## Why

After testing the first build on his logos Tim asked for three things. The Text controls sit in the
Vectorize pane, away from the Layers they belong with. Switching a line between "set in a font" and
"traced as shapes" re-traces the image (10–40 s), so comparing the two is slow; picking another font
round-trips to Rust. And a line's glyphs always land on top of the trace, so where the source has a shape
over the text (the script "Awaits" lying across ADVENTURE) the glyphs come out over the script instead of
under it.

## 1. Traces keyed by the painted set

A trace names the lines painted out of the image it traces. The frontend sends `mask: string[]` — the
sorted ids of the matched lines to paint out — with every `vectorize` call, and caches the answer under
`paramsKey(params) + "|" + mask.join(",")` (`traces` in `state/library.ts`, with `shown` chosen by the
current params and the current enabled set). Rust's `Plans` keeps one painted PNG per mask per image
(`painted: HashMap<MaskKey, Arc<Vec<u8>>>`, built on demand from the kept inks — painting out is
milliseconds) and `vectorize(image_id, parameters, auto, job, mask)` hands the worker that one; an empty
mask traces the original bytes. `for_trace` and `group_for` go: `vectorize` no longer returns `text_group`
(section 2). The line switches change nothing in Rust: `set_line` with `enabled` is no longer called —
`enabled` is frontend state (`TextLine.enabled` in the plan is read at plan time only and then owned by the
store), and `repainted` is gone with it.

Scheduling (`state/library.ts`): when a plan lands, the first trace uses the full mask (every matched line).
When it returns, the store queues the empty mask (the untouched image) so the all-type ↔ all-shapes
compare is instant. Then, while no user-initiated job is running for any image, it prefetches "all but one"
masks in line order (`fullMask without line i`), one at a time, under the current params; a prefetch is
aborted when the user traces, changes a parameter or a preset, edits a string, or when the image is closed,
and never runs with Auto (Auto's candidates are cached by the same key, but only the full mask is traced for
them). A mask that is not cached when a switch asks for it is traced on demand like any trace (the button
shows it busy; the switch stays where the user put it). `Job` gains `mask` and `prefetch: boolean`; a
prefetch shows no busy state in the Viewer and its failure is silent.

## 2. Composition on the frontend, every candidate precomputed

`TextLine.candidates[i]` gains `fit` (the candidate's own `Fit`), `path` (its outline as an SVG `d` in
source pixels) and `text` (its hidden `<text …>…</text>` element), written by `plan.rs`/`compose.rs` for
every fitted candidate; `TextLine.fit` stays as the chosen candidate's. `set_line` with a string change
refits every candidate and returns the new paths; `set_line` with a font that is already a candidate only
moves `chosen` (no fit); a font not among the candidates (a search pick) is fitted and appended. The frontend
sets `chosen` locally and recomposes at once, then tells Rust in the background (`textSetLine({font})`,
answer discarded unless it carries a newly fitted candidate, which is merged in).

`lib/textsvg.ts` becomes `composeSvg(base, plan, enabled: ReadonlySet<lineId>, settings)`: it parses the base
with `svgdoc.parseSvg` (DOM), inserts each enabled matched line's `<path d fill data-line>` (plus its `text`
element when `settings.hiddenText`) at the position section 3 gives, and serialises. `TraceAnswer` keeps
`base` and `svg`; `textGroup` goes. Everything that reads a trace (viewer, Layers, export, Auto candidates,
stats) reads the composed `svg` as today; a text line's path is a shape in `svgdoc` and so in Layers.

## 3. Layering from the source's stacking

`ink.rs` reports per line `occluders: Vec<[u8; 3]>`: the pixels inside the characters' boxes that the
off-colour rule excluded from the ink, grouped into 8-connected blobs, every blob of at least
`OCCLUDER_MIN_PX` (12) px giving its median colour; blobs within 6 ΔE of each other merge. `TextLine.occluders`
carries them (`[[r,g,b], …]`, usually empty).

Composition inserts a line's path before the first shape in document order (among the base's top-level
shapes and those directly inside a top-level `<g>` other than the upsample root group) whose bounds intersect
the line's quad bbox and whose fill is within `OCCLUDER_DE` (12) ΔE of any of the line's occluders; a line
with no such shape is appended at the end as before. Lines are processed in plan order (top of the image
first); a line inserted earlier is a shape later lines may be inserted before, so two re-set lines keep the
source's order. On an upsampled trace (the shapes inside `<g transform="scale(0.5)">`) every line is
appended at the end: a path inside that group would be scaled, and such inputs (≤192 px) rarely hold text.
Hidden shapes: the inspector's `render(hidden)` still works, since the text paths are shapes with indices.

## 4. Inspector

The sidebar's *Inspect* tab (`components/Inspector.tsx`) becomes three collapsible sections, each with a
disclosure header in the pattern of the Vectorize pane's "Advanced" toggle (`ChevronRight` rotating,
`aria-expanded`): **Text** (the `TextSection` content, moved here whole: header count and reader, phase row
with Cancel, errors, per-line rows, Try again, the hover marks on the canvas), **Layers** (the existing list)
and **Clean up** (the existing controls). Open/closed state is held in `useLayerInspector`'s state
(`open: { text, layers, cleanup }`, all open by default) for the session. The stats line at the top of the
tab stays. The Vectorize pane keeps AI redraw only; its `text` slot is removed. `TextSection`'s props are
unchanged apart from losing `enabled`-driven hiding: when Recognise text is off the section renders its
header with "Off in Settings".

## Data and contract changes

- Rust: `Candidate { font, iou, stroke, fit: Fit, path: String, text: String }`; `TextLine.occluders`;
  `vectorize(…, mask: Vec<String>)`; `Plans::painted_for(image, &hash, &mask) -> Option<Arc<Vec<u8>>>`
  (builds and caches); `LineChange.enabled` and `LineAnswer.repainted` removed; `LineAnswer { plan }`.
- TS: `TextCandidate { font, iou, stroke, fit, path, text }`, `TextLine.occluders: [number, number, number][]`,
  `TraceRequest.mask: string[]`, `VectorizeResponse.text_group` removed, `TraceAnswer.textGroup` removed,
  `Job.mask`, `Job.prefetch`; `Library.setTextLine(id, lineId, { string? | font? })`,
  `Library.toggleTextLine(id, lineId, on)` (frontend-only), `composeSvg(base, plan, enabled, settings)`.
- The spec of 2026-10-09: flow step 6 ("Compose") and the UI section are superseded by sections 2–4 here.

## Tests

- Rust: `ink.rs` occluders on a synthetic line with a coloured bar across two letters (one occluder, its
  colour) and none on a plain line; `Plans::painted_for` builds per mask and caches; the plan's candidates
  carry `path`/`text`; `set_line` with a known font moves `chosen` without refitting (same paths), with a new
  font appends a fitted candidate; `text_compose.rs`'s end-to-end test adapted; the fixture suites and the
  real-image gate (`text_examples_gate`) stay green, the gate now also asserting that on image 2 the ADVENTURE
  line's path, when matched or forced, is inserted before the "Awaits" shapes (by occluder colour).
- Frontend: `textsvg.test.ts` — insertion before an occluding shape, append without one, plan order between
  two lines, hidden text on/off, upsample group appended; `library.test.ts` — the mask key, the full-then-empty
  schedule, prefetch order and abort on a user action, a switch showing a cached trace instantly and tracing
  on demand otherwise, font switch local then background `textSetLine`; `Inspector.test.tsx` — three
  collapsible sections, state kept; `TextSection.test.tsx` adapted; `App` tests for the moved section.
- By eye on Tim's images through the harness: Adventure's "Awaits" over ADVENTURE (force ADVENTURE on),
  Lumora's tagline unchanged.

## Out of scope

Precomputing every mask combination; layering by anything other than colour evidence (no depth model);
text inside the upsample group; a weight select in the row.
