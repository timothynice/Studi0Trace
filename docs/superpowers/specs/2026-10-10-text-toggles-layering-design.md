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
current params and the current enabled set). On Auto the full mask is Auto's own run (`auto|mask`); once its
pick is known (an Auto run leaves the pick's params in `item.params`), every other mask is traced with
`auto: false` and kept under the pick's params key, exactly as Auto's candidates are, so a toggle under Auto
traces one mask (or shows a prefetched one), never a whole Auto run; before the pick is known another mask can
only be an Auto run of its own. Rust's `Plans` keeps the painted PNG of the masks asked for per image (an LRU of
at most `PAINTED_MAX` = 16 that never drops the full mask; built on demand from the kept inks — painting out is
milliseconds) and `vectorize(image_id, parameters, auto, job, mask)` hands the worker that one; an empty
mask traces the original bytes, and a mask that names lines with no plan kept for the bytes (or Recognise
text off) answers `image_expired`. `for_trace` and `group_for` go: `vectorize` no longer returns `text_group`
(section 2). The line switches change nothing in Rust: `set_line` with `enabled` is no longer called —
`enabled` is frontend state (`TextLine.enabled` in the plan is read at plan time only and then owned by the
store), and `repainted` is gone with it.

Scheduling (`state/library.ts`): when a plan lands, the first trace uses the full mask (every matched line).
When it returns, the store queues the empty mask (the untouched image) so the all-type ↔ all-shapes
compare is instant. Then, while no user-initiated job is running for any image, it prefetches "all but one"
masks in line order (`fullMask without line i`), one at a time, under the current params (on Auto, the
pick's) — only when the plan has at most `PREFETCH_MAX_LINES` = 8 matched lines; above that only the empty
mask is prefetched. Only the selected image prefetches: a selection change stops the previous image's running
prefetch and puts it back at the head of the queue, where it waits until that image is selected again. A
prefetch is aborted when the user traces, changes a parameter or a preset, edits a string, uses or reverts a
redraw, or when the image is closed; settings that land on a cached trace start it again only after they have
rested `PREFETCH_SETTLE_MS` = 1 s (a slider's ticks do not start and kill workers). A mask that is not cached
when a switch asks for it is traced on demand like any trace (the button shows it busy; the switch stays where
the user put it). `Job` gains `mask` and `prefetch: boolean`; a prefetch shows no busy state in the Viewer and
its failure is silent. Per image the store keeps at most `MASKED_TRACES_MAX` = 24 traces of a masked image per
settings key, dropping the least recently shown first and never the full mask's, the untouched image's, the
one on screen or the one last exported.

## 2. Composition on the frontend, every candidate precomputed

`TextLine.candidates[i]` gains `fit` (the candidate's own `Fit`), `path` (its outline as an SVG `d` in
source pixels) and `text` (its hidden `<text …>…</text>` element), written by `plan.rs`/`compose.rs` for
every fitted candidate; `TextLine.fit` stays as the chosen candidate's. `set_line` with a string change
refits every candidate and returns the new paths; `set_line` with a font that is already a candidate only
moves `chosen` when that candidate's fit is the full one (`Candidate.refined`), and fits it in full in its place
first when it was only screened; a font not among the candidates (a search pick) is fitted and appended. The
frontend sets `chosen` locally and recomposes at once (only the traces whose mask sets that line), then tells
Rust in the background (`textSetLine({font})`, answer discarded unless it carries a newly fitted candidate or a
new path for the chosen one, which is merged in).

`lib/textsvg.ts` becomes `composeSvg(base, plan, enabled: ReadonlySet<lineId>, settings)`: it parses the base
with `svgdoc.parseSvg` (DOM), inserts each enabled line's chosen `<path d fill data-line>`, whatever its status
(a line a string edit leaves with no font is switched off by the store, and the traces that painted it out keep
their vector until a trace without it lands) (plus its `text`
element when `settings.hiddenText`) at the position section 3 gives, and serialises. `TraceAnswer` keeps
`base` and `svg`; `textGroup` goes. Everything that reads a trace (viewer, Layers, export, Auto candidates,
stats) reads the composed `svg` as today; a text line's path is a shape in `svgdoc` and so in Layers.

## 3. Layering from the source's stacking

`ink.rs` reports per line `occluders: Vec<[u8; 3]>`: the pixels inside the characters' boxes that the
off-colour rule excluded from the ink, grouped into 8-connected blobs, every blob of at least
`OCCLUDER_MIN_PX` (12) px that lies over the letters giving its median colour; blobs within 6 ΔE of each other
merge. `TextLine.occluders` carries them (`[[r,g,b], …]`, usually empty). A blob lies over the letters
(`ink::lies_over`; ink "past" a pixel means a mask pixel within 3 px beyond the blob's edge, walking through
the blob first) when at least 30 % of its pixels have ink past them on both sides along one axis (it crosses a
stem: a bar, a script stroke across the letters), or, crossing nothing, when it touches the letters (10 % or
more of its pixels have ink past them on some side) without hugging them (under 80 %): it overlaps their edge
and reaches away, as the script "Awaits" does over the feet of ADVENTURE. A band hugging the letters on one side
— a drop shadow, an outline — is never an occluder. The paint-out (`ink::paint_out`) neither fills nor samples
the foreign pixels (`LineInk::foreign`): the walk to a pixel's nearest neighbours goes past them to the backdrop.

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

- Rust: `Candidate { font, iou, stroke, fit: Fit, path: String, text: String, refined: bool }`; `TextLine.occluders`;
  `vectorize(…, mask: Vec<String>)`; `Plans::painted_for(image, &hash, &mask) -> Option<Arc<Vec<u8>>>`
  (builds and caches); `LineChange.enabled` and `LineAnswer.repainted` removed; `LineAnswer { plan }`.
- TS: `TextCandidate { font, iou, stroke, fit, path, text, refined? }`, `TextLine.occluders: [number, number, number][]`,
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

## Status

2026-10-10, after Tasks 1-7.

**Real-image gate** (`tests/text_examples_gate.rs`, Vision, no key, fonts cached): passes, 19 matched of 25 as before
(1: 2, 2: 14, 3: 3). It now also asserts the layering evidence: ADVENTURE (Declined NoFit, as before) reports
`occluders [[186,153,99]]`, 0.56 delta-E from the script's colour sampled out of the image ([186,152,98], the
commonest off-page colour round the A of "Awaits"; limit 12), and LUMORA reports none. Plan times: image 1 7.0 s,
image 2 23.1 s, image 3 6.8 s (36.9 s in all).

**By eye** (`STUDI0TRACE_TEXT_FORCE=1` harness, which now also writes `N-forced-painted.png`; the compose script is
a scratchpad `tsx` run of the real `composeSvg` under jsdom). Image 2's plan with ADVENTURE forced matched (Manrope
700, IoU 0.40: a poor fit, which is why it is declined) and every matched line enabled, the forced plan's full mask
painted out, traced by the Rust engine (balanced) and composed: `insertionIndex` puts ADVENTURE before shape 1 of 66,
the gold `#bb9862` script, and the render shows "Awaits" lying over the caps, as in the source. With ADVENTURE's
occluders emptied (control) the same composition appends it and the caps cover the script's top. The forced font's
shapes are wrong (it is declined for that), and where the gold crossed the painted-out letters the painted image
leaves a pale smear in the base; neither occurs in the app, which does not paint out a declined line.

**Suites.** `frontend`: 329 tests in 38 files, `tsc --noEmit` and `npm run build` clean. `apps/desktop/src-tauri`:
`cargo test --release` green (174 unit tests and every integration binary). `cargo clippy --release --all-targets
--no-deps -p studi0trace-desktop` reports nothing from `apps/desktop` once `fit_candidates` (8 arguments since it
gained the line id) is allowed `too_many_arguments`; `vexel-rs` has its own pre-existing warnings and one error, as on `main`.

**Trace times** (Rust engine, balanced, the cost of the prefetch; a trace of the painted image is the cheaper one
since the text is no longer in it): image 1 original 9.3 s (first run), full mask 3.7 s; image 2 original 30.3 s, full
mask 15.6 s. Image 1 (2 lines) needs the full, empty and two all-but-one masks after its first result; image 2's 14
all-but-one masks at 15-30 s each take minutes, which is why the prefetch yields to any user job and stops on an edit.

**Deviations.** Hidden shapes (Layers) do not survive a line toggle, because the base trace changes with the mask and
shape indices with it. A line whose status is declined has its switch disabled (it has no candidate to set). With
Recognise text off the Text section shows its header with "Off in Settings". The spec's gate assertion (the path
inserted before the "Awaits" shapes) is the occluder-colour assertion above plus the by-eye render, since the gate
has no trace. The assertion is on a declined line; the only matched line of the three images with occluders is image 3's HORIZON (two
warm colours, the sun's), whose layering was not looked at.

**Not verified.** The running app. The dev app was built and launched (`npm run dev`, debug build, no panic in its
output) but this environment grants no accessibility or screen-recording access, so no window could be seen or
driven: the Inspect tab's Text / Layers / Clean up, the instant tagline toggle after the second `vectorize`, the
instant font pick, the lines in Layers and the light and dark screenshots are unchecked, and the prefetch timings above are
the engine's, not read from the app's console. Needs Tim's eyes on the real app (both themes).
