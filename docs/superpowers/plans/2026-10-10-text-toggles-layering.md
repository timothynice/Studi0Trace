# Text Toggles & Layering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Instant switching between a line as type and as shapes (traces cached per painted set, every candidate font precomputed), each text line layered where the source stacks it, and the Text controls in the inspector beside Layers.

**Architecture:** Rust stops composing: the plan carries, per candidate font, its fit, outline path and hidden `<text>`, plus per line the colours of anything that lay over the text (`occluders`); `vectorize` takes the mask of lines to paint out and `Plans` caches a painted PNG per mask. The frontend owns the enabled set, keys traces by `params|mask`, schedules the full and empty masks eagerly and "all but one" masks in the background, and composes the SVG itself with `svgdoc`'s DOM parser, inserting each line's path before the first traced shape whose fill matches an occluder and overlaps the line. The Inspect tab gets three collapsible sections: Text, Layers, Clean up.

**Tech Stack:** Rust (Tauri 2, the existing `text` module, serde), React 18 + TypeScript + vitest (`@testing-library/react`), jsdom's DOMParser/XMLSerializer.

**Spec:** `docs/superpowers/specs/2026-10-10-text-toggles-layering-design.md` (read it first; it supersedes flow step 6 and the UI section of `2026-10-09-text-resetting-design.md`).

## Global Constraints

- The engine (`backend/`), the core (`crates/studi0trace-core`) and `apps/desktop/src-tauri/src/worker.rs` are not changed; `tests/redraw_boundary.rs` keeps passing (no `text` in the worker, `reqwest` only in `fonts.rs`, `openai` only in `reader_openai.rs`/`fonts.rs`).
- Components never call `invoke`/`fetch`; the platform layer is the only host boundary. No React in `state/library.ts`.
- Never a one-sided coloured border as a highlight; collapsible headers follow the Vectorize pane's "Advanced" toggle (`ChevronRight` rotating, `aria-expanded`, `mac-ghost h-8 w-full px-1.5`).
- Constants: `OCCLUDER_MIN_PX = 12` (px per blob), `OCCLUDER_DE = 12` (ΔE, frontend), blobs within 6 ΔE merge.
- Trace cache key: `` `${paramsKey(params)}|${mask.join(",")}` `` with `mask` the sorted line ids painted out; `"auto"` keys become `` `auto|${mask}` ``. The full mask = every line whose status is `matched`; the enabled set starts as the plan's `enabled` flags and is frontend state afterwards.
- Prefetch order: full mask (the first trace) → empty mask → for each matched line in plan order, full mask minus that line; one at a time; only while no user job runs on any image; aborted by a user trace, a param/preset change, a string edit, a close; never under Auto; a prefetch shows no busy state and fails silently.
- Composition inserts in plan order; a line with no matching occluder shape is appended at the end; on an upsampled trace (a top-level `<g transform="scale(0.5)">` holding the shapes) every line is appended at the end.
- `svgdoc` indices: base shapes keep their document-order indices; text-line shapes (`data-line`) take indices after every base shape whatever their DOM position, so Layers' hidden set survives a toggle.
- Commit after every task, message a sentence on what and why, blank line, `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`. Never stage `.superpowers/` or the scratchpad.
- Commands: Rust `cd apps/desktop/src-tauri && cargo test --release --test text_compose --test text_fit --test text_match --test text_ink_fixtures --test text_fonts --test text_openai --lib` (text_fit ≈65 s); the gate `STUDI0TRACE_TEXT_EXAMPLES=/private/tmp/claude-502/-Users-TimNice-Development-tracer--claude-worktrees-svg-text-handling-fbf2c8/33ac6b5f-d9f7-489b-ab30-1b0e7edbf73b/scratchpad/text cargo test --release --test text_examples_gate -- --ignored --nocapture`; frontend `cd frontend && npm run test:run -- <files>`, `npx tsc --noEmit -p .`, `npm run build`.

## Review Focus

1. A plan with no matched line (mask empty from the start): no prefetch, no composition, the trace key is `params|` — pinned in Task 5's tests.
2. A line toggled while the trace for its new mask is in flight (and toggled back): one request per mask, the switch stays where the user left it, the answer lands under its own key — Task 5.
3. Layers' hidden set after a toggle: hiding a base shape, then toggling a text line, keeps the same base shape hidden — Task 4 (index rule) and Task 6.
4. Two re-set lines overlapping, the lower one occluding the upper in the source: plan order plus occluder insertion keeps the source's order — Task 4.
5. An upsampled trace (`scale(0.5)` root group): text appended at the end, never scaled — Task 4.

## File map

Rust (`apps/desktop/src-tauri/`): `src/text/mod.rs` (`Candidate`, `TextLine.occluders`, `LineChange`), `src/text/ink.rs` (`occluders`), `src/text/compose.rs` (`candidate_path`, `candidate_text`; the group writer removed), `src/text/plan.rs` (candidates carry fit/path/text; `set_line` without `enabled`; `Painted` becomes per-mask), `src/text/plans.rs` (`painted_for`; `for_trace`/`group_for` removed), `src/commands.rs` (`vectorize(mask)`, `LineAnswer { plan }`), `tests/text_compose.rs`, `tests/text_examples_gate.rs`.
Frontend (`frontend/src/`): `platform/types.ts`, `platform/native.ts`, `platform/web.ts`, `lib/api.ts`, `lib/svgdoc.ts` (index rule), `lib/textsvg.ts` (+test), `state/library.ts` (+test), `components/Inspector.tsx` (+test), `components/TextSection.tsx` (+test), `components/VectorizePanel.tsx`, `hooks/useLayerInspector.ts`, `App.tsx`.

---

### Task 1: Per-candidate paths and the line's occluders (Rust)

**Files:**
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`Candidate`, `TextLine`), `src/text/ink.rs`, `src/text/compose.rs`, `src/text/plan.rs`
- Test: `src/text/ink.rs` (unit), `tests/text_compose.rs`

**Interfaces:**
- Produces:

```rust
// mod.rs
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate { pub font: FontRef, pub iou: f32, #[serde(default)] pub stroke: f32, pub fit: Fit, pub path: String, pub text: String }
// TextLine gains: #[serde(default)] pub occluders: Vec<[u8; 3]>,
// ink.rs
pub struct LineInk { …, pub occluders: Vec<[u8; 3]> }   // filled by line_ink; see the rule below
// compose.rs
pub fn candidate_path(shaped: &glyphs::Shaped, fit: &Fit) -> String;                       // fit::path_data
pub fn candidate_text(line_id: &str, string: &str, family: &str, shaped: &glyphs::Shaped, fit: &Fit) -> String;  // the hidden <text> element exactly as compose_group writes it today
```

The occluder rule (`ink.rs`): the `foreign` pixels (already computed: off the backdrop→ink line in both linear and encoded space) that lie inside the characters' boxes (`segments` x ranges × the ink's y bounds, each grown 1 px) are grouped into 8-connected blobs; a blob of at least `OCCLUDER_MIN_PX` (12) px gives its median colour; colours within 6 ΔE of one already listed merge (keep the first). `occluders` is that list, in blob order.

- [ ] **Step 1: Write the failing tests**

In `src/text/ink.rs`'s `mod tests`:

```rust
#[test]
fn a_bar_of_another_colour_across_the_letters_is_an_occluder_and_a_plain_line_has_none() {
    let (src, quad) = three_letters();
    assert!(line_ink(&src, quad).occluders.is_empty());
    // a gold bar 4 px tall across the middle of the three bars
    let mut g = src.clone();
    paint(&mut g, 18, 30, 52, 4, [212, 160, 23]);
    let ink = line_ink(&g, quad);
    assert_eq!(ink.occluders.len(), 1, "{:?}", ink.occluders);
    let [r, gg, b] = ink.occluders[0];
    assert!(delta_e([r, gg, b], [212, 160, 23]) < 6.0);
    // the bar's pixels are not ink: the segments are still three
    assert_eq!(ink.segments.len(), 3);
}
```

In `tests/text_compose.rs`, extend the end-to-end test (`plans_a_fixture_line_end_to_end_and_an_edit_recomposes`): after the plan, assert every candidate carries a non-empty `path` starting with `M`, a `text` containing `>EST. 2026</text>` and `data-line="img-t0"`, a `fit` whose `iou` equals the candidate's `iou`; and that `planned.plan.lines[0].occluders.is_empty()`.

- [ ] **Step 2: Run to see them fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --lib text::ink && cargo test --release --test text_compose 2>&1 | tail -5`
Expected: compile errors (`occluders`, `path`, `text`, `fit` missing).

- [ ] **Step 3: Implement**

`ink.rs`: after `foreign` is built, compute `occluders` with a flood fill over `foreign && in_character_box` (reuse the stack-based labelling in `segments_of`, generalised to a mask slice), median colour via the existing `median_colour`, the merge rule with `delta_e`. `LineInk::empty()` gets `occluders: Vec::new()`.

`compose.rs`: split the per-line body of `compose_group` into `candidate_path` and `candidate_text` (the `<text x="…" y="…" font-family font-size fill="none" pointer-events="none" data-line>` with the per-glyph x list exactly as today); `compose_group`/`text_group`/`Composed` are deleted (Task 2 removes their callers; if Task 2 is not yet in, keep them compiling by calling the two helpers).

`plan.rs`: wherever a `Candidate` is built from a `(Candidate, Fit)` pair (`finish_line`, `fit_candidates`, `refine`, the forced-font path, `set_line`'s append), shape the string once per candidate's font (`Font::load` via `catalog.locate`) and fill `fit`, `path`, `text`; `TextLine.occluders = ink.occluders.clone()` in `run`. Make `Candidate` construction one function `fn candidate(font: FontRef, fit: Fit, string: &str, line_id: &str, shaped: &Shaped) -> Candidate` so no site forgets a field.

- [ ] **Step 4: Run the tests** — `cargo test --release --lib text::ink`, `--test text_compose`, then the whole fixture list from Global Constraints. Expected: green (text_fit unchanged).

- [ ] **Step 5: Commit** — `git add apps/desktop/src-tauri/src apps/desktop/src-tauri/tests` / message: "text: every candidate carries its fit, outline and hidden text, and a line reports the colours that lay over it".

---

### Task 2: Traces by mask (Rust): `Plans::painted_for`, `vectorize(mask)`, no group on the answer

**Files:**
- Modify: `src/text/plans.rs`, `src/text/plan.rs` (`Painted`, `paint_and_group` → `paint_mask`, `set_line`, `LineChange`, `repainted` removed), `src/text/mod.rs` (`LineChange`), `src/commands.rs` (`vectorize`, `LineAnswer`, `text_set_line`), `tests/text_compose.rs`, `tests/redraw_boundary.rs` (unchanged, must pass)

**Interfaces:**
- Produces:

```rust
// plan.rs
pub struct Planned { pub plan: TextPlan, pub inks: Vec<Arc<ink::LineInk>> }     // no painted, no group
/// The source with the lines of `mask` (ids) painted out, as PNG; None for an empty mask or no known line.
pub fn paint_mask(source: &Image, planned: &Planned, mask: &[String]) -> Result<Option<Vec<u8>>, CommandError>;
#[derive(Debug, Clone, Default, Deserialize)] pub struct LineChange { pub string: Option<String>, pub font: Option<FontRef> }
// plans.rs
pub type MaskKey = String;   // sorted ids joined by ","
impl Plans {
    /// The painted PNG for `mask`, built from the kept plan (and the source decoded by the caller) and cached per mask; None when no plan matches the hash.
    pub fn painted_for(&self, image: &str, hash: &[u8; 32], mask: &[String], source: &Image) -> Result<Option<Arc<Vec<u8>>>, CommandError>;
}
// commands.rs
pub async fn vectorize(app, state, image_id: String, parameters: Value, auto: bool, job: String, mask: Vec<String>) -> Result<Value, CommandError>;  // reply has no text_group
pub struct LineAnswer { pub plan: TextPlan }
```

`set_line` rules: `string` → refit every candidate (paths refreshed), the chosen font kept as `forced` as today; `font` already a candidate → set `chosen` only (no fit, status `Matched`, `enabled` untouched); `font` new → fit it, append as a candidate (fetching it through `fonts.ensure` first), choose it; no `enabled` field any more (`check_change` validates the font as today).

- [ ] **Step 1: Write the failing tests**

In `tests/text_compose.rs`: a `painted_for` test — plan the fixture line; `painted_for(.., &["img-t0"], &img)` returns Some PNG whose decode has no dark pixel; `painted_for(.., &[], &img)` returns None; calling the first again returns the same `Arc` (`Arc::ptr_eq`); an unknown id in the mask is ignored. A `set_line` test — `LineChange { font: Some(candidates[1].font) }` returns a plan whose `chosen == Some(1)` and whose candidates' paths are the same strings as before (no refit); `LineChange { string: Some("EST. 2027") }` returns candidates whose `text` contains `EST. 2027`. Remove the `repainted` and `enabled` tests.

- [ ] **Step 2: Run to see them fail** — compile errors.

- [ ] **Step 3: Implement** — `Entry` gains `painted: Mutex<HashMap<MaskKey, Arc<Vec<u8>>>>` (or inside the single state lock); `painted_for` looks up the entry by hash, builds through `paint_mask` on a miss (the caller runs it under `redraw::blocking`), caches, returns. `vectorize`: `let mask_key = …`; under `blocking`: hash, then if `recognise_text && !mask.is_empty()` decode the source (`Source::decode`) and `painted_for`, else the original bytes; submit; after `recv` return the answer untouched. Delete `for_trace`, `group_for`, `repainted`, `Painted.group`, `compose_group`'s callers. `text_set_line` answers `LineAnswer { plan }` and uses `put_if` as today.

- [ ] **Step 4: Run** — the fixture list, `--test redraw_boundary`, and `cargo test --release` for the crate. Expected: green.

- [ ] **Step 5: Commit** — "text: the trace names the lines it paints out; Plans caches a painted image per mask and the answer carries no group".

---

### Task 3: Frontend contract (types, platform, api)

**Files:**
- Modify: `frontend/src/platform/types.ts`, `platform/native.ts`, `platform/web.ts`, `lib/api.ts`, `platform/native.test.ts`

**Interfaces:**
- Produces (`types.ts`):

```ts
export interface TextCandidate { font: FontRef; iou: number; stroke: number; fit: TextFit; path: string; text: string }
export interface TextLine { …; occluders: [number, number, number][]; }
export interface TextLineChange { string?: string; font?: FontRef }
export interface TextLineAnswer { plan: TextPlan }
export interface TraceRequest { imageId: string; parameters: ParamValues; auto: boolean; job: string; /** Sorted ids of the text lines painted out of the image before it is traced. */ mask: string[] }
```

`lib/api.ts`: remove `text_group` from `VectorizeResponse`. `native.ts`: `vectorize` passes `mask` to `invoke("vectorize", { imageId, parameters, auto, job, mask })`. `web.ts`: ignores `mask`.

- [ ] **Step 1: Write the failing test** — in `native.test.ts`, the vectorize mock asserts the payload carries `mask: ["a-t0"]`.
- [ ] **Step 2: Run to see it fail.**
- [ ] **Step 3: Implement**; fix every `tsc` error this causes in callers minimally (the store is reworked in Task 5 — until then it passes `mask: []`).
- [ ] **Step 4: Run** `npm run test:run -- src/platform` and `npx tsc --noEmit -p .`.
- [ ] **Step 5: Commit** — "frontend: the trace request carries its mask; candidates carry fit, path and text; the answer carries no group".

---

### Task 4: Composition on the frontend with source layering; stable Layers indices

**Files:**
- Modify: `frontend/src/lib/svgdoc.ts`, `frontend/src/lib/textsvg.ts`
- Test: `frontend/src/lib/textsvg.test.ts`, `frontend/src/lib/svgdoc.test.ts`

**Interfaces:**
- Produces:

```ts
// textsvg.ts
export const OCCLUDER_DE = 12;
export interface ComposeInput { plan: TextPlan | null; enabled: ReadonlySet<string>; hiddenText: boolean }
/** The base trace with every enabled, matched line's chosen candidate inserted where the source stacks it. Returns `base` unchanged when nothing composes. */
export function composeSvg(base: string, input: ComposeInput): string;
/** The index in `doc.shapes` (document order over base shapes) before which `line` goes, or null to append. Exported for tests. */
export function insertionIndex(doc: SvgDoc, line: TextLine): number | null;
// svgdoc.ts
// SvgShape gains `line?: string` (the data-line of a text path); parseSvg assigns indices: base shapes first in document order, then text-line shapes in document order.
```

Insertion rule: parse the base with `parseSvg`; for each enabled matched line in plan order with a chosen candidate: `lineBox` = bbox of `quad`; among `doc.shapes` without `line`, the first (by index) whose `bounds` intersects `lineBox` and whose `fill` (hex) is within `OCCLUDER_DE` of any `line.occluders` colour (ΔE as `lib/text` or a small Lab helper in `textsvg.ts`) → insert the new `<path d fill data-line>` (+ the `text` element when `hiddenText`) as a previous sibling of that shape's element; none → append before `</svg>`. If the document's shapes live inside a `<g transform="scale(…)">` root group, always append. Inserted elements are created with `document.createElementNS`; the result is `XMLSerializer`. The fill colour is `#rrggbb` from `line.colour`.

- [ ] **Step 1: Write the failing tests** (`textsvg.test.ts`):

```ts
const base = (shapes: string) => `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 60">${shapes}</svg>`;
const line = (id: string, quad: [number, number][], occluders: [number, number, number][] = [], colour: [number, number, number] = [0, 0, 0]): TextLine =>
  ({ id, quad, string: "AB", class: "type", colour, status: { kind: "matched" }, chosen: 0, enabled: true, fit: null, occluders,
     candidates: [{ font: { family: "F", weight: 400, italic: false, source: "google" }, iou: 0.9, stroke: 1, fit: { size: 10, baseline: 30, x: 10, tracking: 0, nudges: [], iou: 0.9, stroke: 1 }, path: "M10 20h20v10h-20z", text: `<text x="10" y="30" fill="none" data-line="${id}">AB</text>` }] });
it("appends a line with no occluder, before </svg>", …)                       // order: backdrop rect, then path[data-line]
it("inserts before the first overlapping shape whose fill matches an occluder", …) // shapes: white backdrop, a gold rect over the line's box, a blue rect elsewhere; occluders [[212,160,23]] → path sits before the gold rect, after the backdrop
it("ignores a matching colour that does not overlap the line", …)
it("keeps plan order between two overlapping lines", …)                      // line A inserted before an occluder, line B (lower) appended → A precedes B
it("appends on an upsampled trace", …)                                       // `<g transform="scale(0.5)">…</g>` → path is the last child of <svg>
it("leaves the hidden text out when the setting is off, in when on", …)
it("returns the base unchanged with no plan, no enabled line, or a declined line", …)
```

(`svgdoc.test.ts`): a document with `rect, path[data-line], rect` parses with indices rect→0, rect→1, text path→2; `render(new Set([1]))` drops the second rect and keeps the text path.

- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement** `textsvg.ts` (DOM-based) and the `svgdoc.ts` index rule (`parseSvg` collects elements, then orders: non-line first; `render` uses the same mapping; `shapeLabel` shows "Text: <string>" for a line shape — pass the string via `data-label` on the path, written by `composeSvg`).
- [ ] **Step 4: Run** `npm run test:run -- src/lib/textsvg.test.ts src/lib/svgdoc.test.ts`.
- [ ] **Step 5: Commit** — "frontend: the text is composed here, each line inserted where the source stacks it; text shapes take the last indices so Layers' hidden set survives a toggle".

---

### Task 5: The store: traces by mask, the enabled set, scheduling, local font switches

**Files:**
- Modify: `frontend/src/state/library.ts`, `frontend/src/state/library.test.ts`

**Interfaces:**
- Produces:

```ts
export interface TraceAnswer { svg: string; base: string; elapsedMs: number; stats: Record<string, number> }   // svg = composeSvg(base, …) for the trace's own mask
export interface Job { id: string; key: string; startedAt: number; phase: Phase; mask: string[]; prefetch: boolean }
export interface TextState { plan: TextPlan | null; phase: TextPhase | null; error: ApiError | null; asked: boolean; /** Lines currently set in a font. */ enabled: ReadonlySet<string> }
export const maskOf = (text: TextState): string[];                 // sorted ids of enabled matched lines
export const fullMask = (plan: TextPlan): string[];
export const traceKeyFor = (item: ImageItem, mask: string[]): string;   // `${auto ? "auto" : paramsKey(params)}|${mask.join(",")}`
// Library gains:
toggleTextLine(id: string, lineId: string, on: boolean): void;      // flips the set; shows the cached trace for the new mask or traces it (a user job, not a prefetch)
// setTextLine(id, lineId, { string? | font? }): a font among the candidates → local chosen + recompose every trace, then platform.textSetLine in the background (its plan merged only if it adds a candidate); a string, or a font not among the candidates → the round trip as today, then recompose every trace from its base.
```

Scheduling: `finish(id, job, res)` stores `{ base, svg: composeSvg(base, { plan, enabled: new Set(job.mask) … }) }` under `job.key` — note the composed svg of a cached trace is recomputed whenever `plan`/`chosen`/`hiddenText` change (`recompose(id)` maps every trace from its `base` with its own key's mask, parsed from the key). `shown` = `traceKeyFor(item, maskOf(text))` when cached. After a user trace finishes (not a prefetch) and `text.plan` has matched lines: `schedulePrefetch(id)` — a queue of masks `[[] , ...fullMask.map(l => fullMask.without(l))]` filtered to uncached keys; `runPrefetch()` starts the next one when no non-prefetch job exists on any item and no prefetch is running; `abortPrefetch()` (called by `generate`, `setParam`, `pickPreset`, `setTextLine` string edits, `remove`, `clear`, `acceptRedraw`, `revertRedraw`, `setSettings` with params) aborts the running prefetch job and clears the queue. `item.job` for a prefetch has `prefetch: true`; `currentJob`/busy helpers ignore it (export a `userJob(item)`).

- [ ] **Step 1: Write the failing tests** (`library.test.ts`, with the file's helpers; the plan fixture has two matched lines `a-t0`, `a-t1`):

```ts
it("keys the first trace by params and the full mask, then prefetches the empty mask and all-but-one masks in order", …)
   // vectorize calls, in order, carry masks: ["a-t0","a-t1"], [], ["a-t1"], ["a-t0"]; the item's job during a prefetch has prefetch: true
it("a prefetch is aborted by a parameter change and never runs under Auto", …)
it("toggling a line shows the cached trace for its mask instantly, and traces on demand when it is not cached", …)
it("toggling while that mask's trace is in flight does not request it twice and lands under its key", …)
it("a font among the candidates swaps locally and tells the app in the background; a search pick round-trips", …)
it("a string edit recomposes every cached trace from its base", …)
it("a plan with no matched line traces with an empty mask and prefetches nothing", …)
it("hidden text off recomposes every trace without <text>", …)
```

- [ ] **Step 2: Run to see them fail.**
- [ ] **Step 3: Implement** (keep `ensureText`/`retryText`/`cancelText` as they are; remove the `repainted` branch; `KeptTraces` carries the new shapes).
- [ ] **Step 4: Run** `npm run test:run -- src/state/library.test.ts` then the whole suite and `tsc`.
- [ ] **Step 5: Commit** — "frontend: traces keyed by the painted set, the empty and all-but-one masks prefetched, font switches local".

---

### Task 6: Inspector sections, the Text section moved, App wiring

**Files:**
- Modify: `frontend/src/components/Inspector.tsx`, `components/TextSection.tsx`, `components/VectorizePanel.tsx`, `hooks/useLayerInspector.ts`, `App.tsx`
- Test: `components/Inspector.test.tsx`, `components/TextSection.test.tsx`, `App.*.test.tsx` as affected

**Interfaces:**
- `InspectorState` gains `sections: { text: boolean; layers: boolean; cleanup: boolean }` (all `true` in `EMPTY_INSPECTOR`), held in `useLayerInspector`'s shared state; `Inspector` gains `text?: ReactNode` (the section's body) and renders three `<Section title open onToggle>` blocks: Text (always, even before a trace — the plan can exist without one), Layers and Clean up (when `doc`). The disclosure button: `mac-ghost h-8 w-full px-1.5 text-foreground` with `ChevronRight` rotated when open, `aria-expanded`, the title in the existing `h4` style.
- `TextSection` loses `enabled`; its switch calls `onToggle(lineId, on)` (new prop) instead of `onChange({enabled})`; its font select/search call `onChange(lineId, {font})`; when Recognise text is off it renders the header plus "Off in Settings".
- `VectorizePanel` loses the `text` slot. `App.tsx` passes `<TextSection …>` into `Inspector`'s `text` prop and wires `onToggle={(l, on) => lib.toggleTextLine(id, l, on)}`; the hover overlay unchanged.

- [ ] **Step 1: Write the failing tests** — `Inspector.test.tsx`: renders three headers, each toggles its body and calls `onChange({ sections })`; the Text body renders before a trace; `TextSection.test.tsx`: the switch calls `onToggle("a-t0", false)`; "Off in Settings" when `recogniseText` is false (prop `enabled` replaced by `settingOn`).
- [ ] **Step 2: Run to see them fail.** - [ ] **Step 3: Implement.** - [ ] **Step 4: Run** the component tests, the App tests, the whole suite, `tsc`, `npm run build`.
- [ ] **Step 5: Commit** — "inspector: Text, Layers and Clean up as collapsible sections; the Text controls live beside the layers".

---

### Task 7: Verification — gate with the layering assertion, by eye, the app

**Files:**
- Modify: `apps/desktop/src-tauri/tests/text_examples_gate.rs`, `docs/superpowers/specs/2026-10-10-text-toggles-layering-design.md` (Status), `docs/superpowers/specs/2026-10-09-text-resetting-design.md` (a line pointing at the new spec), `CLAUDE.md` (the text paragraph: composition on the frontend, masks, occluders)

- [ ] **Step 1:** In the gate, after planning image 2, assert the `ADVENTURE` line (matched or not) reports at least one occluder within 12 ΔE of the gold `[212,160,23]`-ish script colour (print the colours) and the `LUMORA` line of image 1 reports none; run the gate (fonts cached) — 19 matched as before.
- [ ] **Step 2:** By eye through the harness plus a small script that composes image 2's plan with `composeSvg`'s rule (port the insertion rule to Python for the check or write the composed SVG from a tiny Rust `#[ignore]` test that reuses the frontend rule? — simplest: `cd frontend && npx tsx scripts/compose-check.ts <plan.json> <base.svg>` with a 30-line script calling `composeSvg`, kept in the scratchpad, not the repo): render and crop "Awaits"/ADVENTURE with ADVENTURE forced on — the script must lie over the caps.
- [ ] **Step 3:** All suites: `cd frontend && npm run test:run && npx tsc --noEmit -p . && npm run build`; `cd apps/desktop/src-tauri && cargo test --release`; clippy adds nothing in `apps/desktop`.
- [ ] **Step 4:** Launch the dev app (`cd apps/desktop && npm run dev` in a terminal tab), open the Lumora image: the Inspect tab shows Text / Layers / Clean up; toggling the tagline is instant after the second trace lands; picking a font is instant; the text lines appear in Layers. Screenshot both themes to the scratchpad and look.
- [ ] **Step 5:** Write the Status (what was measured, the prefetch timings on image 2, deviations), update the docs, commit.

## Self-review

- Spec §1 → Tasks 2, 3, 5; §2 → Tasks 1, 2, 4, 5; §3 → Tasks 1, 4, 7; §4 → Task 6; Data/contract → 1–3; Tests → each task; by-eye → 7.
- Placeholders: none — Task 7 step 2 chooses the tsx script explicitly.
- Types: `Candidate{fit,path,text}` ↔ `TextCandidate{fit,path,text}`; `LineChange{string,font}` ↔ `TextLineChange`; `vectorize(mask)` ↔ `TraceRequest.mask`; `LineAnswer{plan}` ↔ `TextLineAnswer{plan}`; `maskOf`/`fullMask`/`traceKeyFor` used in Tasks 5–6 only.
- Review Focus 1–5 each name their task.
