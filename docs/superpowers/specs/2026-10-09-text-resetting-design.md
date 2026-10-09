# Text re-setting — design

Status: approved by Tim 2026-10-09 in chat ("create the plan for everything and autonomously build
this all"). Branch `claude/svg-text-handling-fbf2c8`. Three engine follow-ups have their own specs:
`2026-10-09-thin-stroke-seeding-design.md`, `2026-10-09-soft-disc-merge-design.md`,
`2026-10-09-text-as-shapes-design.md`.

## Why

Small text is the worst thing Vexel draws. On three of Tim's inputs (AI-made logos, 1254–1536 px,
taglines of 17–31 px caps with 2–3 px strokes, black on white) the trace loses whole letters — the
"0" of "EST. 2026", the "O" of "OUTSIDE", the "D" of "IDEAS", the bowls of R, B and G — and the ones
it keeps are wobbly and unlike each other. Large text ("LUMORA", 150 px caps) is fine. The loss is
in the partition: `seed_mask` drops pixels within 2 px of a ridge, so a 2–3 px stroke seeds only
through fragments, and `seed_markers` discards fragments under `min_region`; the wave-lockup loop
confirmed this (D1) and both engine fixes it tried regressed the bench. That fix is queued as its own
spec. This one takes the other road, which the goal actually asks for: *consistent letterforms*.
Letters are consistent when they come from a font, so the app recognises each line of text, finds
the font (Tim's inputs are set in fonts that exist on Google Fonts or his Mac, or something very
close), sets the string in it, fits it onto the pixels and draws the glyph outlines. The trace never
sees the text. Editable and searchable text are side effects, not goals.

## Where it lives

In the Mac app, `apps/desktop/src-tauri/src/text/`, beside `redraw/`. The engine, the core and the
worker are untouched: `tests/redraw_boundary.rs` keeps `worker.rs` free of `text`, `reqwest`,
`keychain` and `openai`, and the core stays wasm-clean. The web platform (the Python dev harness)
has no text path and rejects the commands with `unavailable`, as it does redraw.

## Flow

Text runs on every trace while *Recognise text* is on (Settings, default on). Nothing runs without
it. The plan for an image is computed once and cached; traces reuse it.

1. **Find and read.** The parent decodes the image (`studi0trace_core::intake::load`, as
   `redraw::Source::decode` does) and runs macOS Vision (`objc2-vision`:
   `VNRecognizeTextRequest`, accurate level, language correction off, `minimumTextHeight` 0) on
   it. Each observation is a line: a quad in pixels (Vision's normalised, bottom-left coordinates
   converted) and a string. If *Read text with AI* is on, the AI reader runs after Vision with the
   whole image and the numbered line crops, and returns per line the corrected string, up to three
   font guesses (family, weight 100–900, italic) and a class: `type`, `script` or `custom`. Both
   readers implement one trait:

   ```rust
   pub trait Reader {
       fn read(&self, image: &Source, lines: &[LineBox]) -> Result<Vec<Reading>, CommandError>;
   }
   pub struct Reading { pub string: String, pub hints: Vec<FontHint>, pub class: Class }
   ```

   Vision is the finder for both; the AI reader never supplies boxes (its coordinates are rough).
   A line Vision reads with confidence under 0.3, or whose string is empty after trimming, is
   dropped.
2. **Ink.** Per line, inside the quad grown by 20 % of its height: the backdrop is the median
   colour of the box's 2 px border ring; coverage per pixel is the pixel's position between the
   backdrop and the ink colour along the line joining them in linear RGB, clamped to 0..1; the ink
   colour is the median of pixels whose distance from the backdrop is at least 90 % of the box's
   maximum. The ink mask is coverage ≥ 0.5. Per-character segments are the mask's connected
   components merged where their x ranges overlap (dots of i and j, the two parts of a colon).
   A line is **declined** — left to the trace — when the reader classed it `script` or `custom`,
   when the ink's spread (90th percentile ΔE of the covered pixels from the ink colour) is over
   8 (gradient or multicolour text), or when the segment count differs from the string's
   non-space character count by more than a third (the string and the ink disagree).
3. **Match.** Candidates, in order: the AI hints; then a keyless shortlist from the bundled
   Google Fonts index and the installed fonts, ranked by distance between the line's measured
   per-character bbox widths over cap height (for the characters the string names) and the
   family's, plus the stroke-thickness-over-cap-height distance; the top 12 families. For each
   family the weight is the variant whose stem thickness is nearest the line's, and its
   neighbours ±100 are tried when the best fit is within 0.03 IoU of the next. A family that
   won another line of the same image is moved to the front of every other line's list (a logo
   uses one or two fonts).
4. **Fit.** For each candidate: shape the string with `rustybuzz` (kerning on), outline the glyphs
   with `ttf-parser`, and solve size (px per em), baseline y, start x and tracking (px added per
   gap) by coordinate descent on the IoU between the rendered coverage (tiny-skia, anti-aliased)
   and the ink coverage, from an initial guess (size from the ink height over the font's cap
   height for all-caps strings, else over the shaped string's bbox height; start from the ink's
   left; tracking from the ink width). Steps 1, 0.5, 0.25 and 0.1 px, stop when no parameter
   improves. Then per-glyph x nudges of up to ±1.5 px, kept only where the line's IoU rises
   (AI-made logos space letters unevenly; the letterforms stay the font's). The best candidate
   wins. A best IoU under `FIT_ACCEPT` declines the line. `FIT_ACCEPT` is set on the fixture set
   (§Tests): the lowest true-font IoU there minus 0.05, expected near 0.82.
5. **Paint out.** The ink mask of every matched, enabled line, dilated by 1 px, is inpainted:
   each masked pixel takes the linear interpolation between the nearest unmasked pixels to its
   left and right on its row, averaged with the same along its column. The result is encoded as
   PNG and cached beside the plan; `vectorize` ships it to the worker instead of the original
   bytes while the plan has at least one matched, enabled line. The cache is keyed by the image's
   bytes (a hash), so an accepted or reverted redraw invalidates it.
6. **Compose.** `vectorize` returns the base SVG as it does today, plus `text_group`: a string
   `<g id="text">…</g>` in the SVG's own coordinates (the source pixel grid; the upsample path's
   `scale(0.5)` group is a sibling, never a parent). Per matched, enabled line, one `<path>` of
   every glyph outline with `fill` set to the ink colour, carrying `data-line="<id>"` — one path
   per line so a line is one shape in Layers. When *Hidden text in exports* is on, each line also
   gets `<text x y font-family font-size fill="none" pointer-events="none" data-line>string</text>`
   after its path: searchable and read by screen readers, invisible, and nothing for `svgdoc.ts`
   (which ignores `<text>`), the core's renderer (built without text) or `svg2pdf` (likewise) to
   draw. The frontend composes (`lib/textsvg.ts`: insert the group before `</svg>`) for the
   viewer, every Auto candidate and export; stats are recomputed with `svgStats`.

Edits in the panel: a string or font change calls `text_set_line`, which refits that line and
returns the plan and a new `text_group`; the frontend recomposes at once, no trace. Enabling or
disabling a line changes what is painted out, so the frontend drops the image's traces and traces
again.

## Fonts

- **Installed fonts** through `fontdb::Database::load_system_fonts()`: family, weight, style and
  the file. Their features are measured on first use and cached in app data.
- **Google Fonts.** `apps/desktop/tools/gfonts_index.py` builds `src-tauri/assets/gfonts.json`
  from `https://fonts.google.com/metadata/fonts` (keyless): per family, the category and the
  variants (weight, italic, Google's thickness/slant/width), and for the family's regular variant
  (400 or nearest), downloaded during the build of the index, the measured features: cap height
  and x-height over em, stem thickness over cap height (from the rendered I and H), and the bbox
  width over cap height of each of A–Z, a–z and 0–9 as u8 hundredths. About 200 KB, embedded with
  `include_str!`. The tool is run by hand to refresh the index; it is not part of the app build.
- **Download.** `GET https://fonts.googleapis.com/css2?family=<Family>:ital,wght@<i>,<w>` with a
  non-browser user agent returns `@font-face` rules whose `src` is a TTF on `fonts.gstatic.com`;
  the file is fetched into `<app data>/fonts/<family>-<weight><i>.ttf` and reused. Every Google
  Font is OFL, Apache or UFL licensed: outlining glyphs into a document is permitted. A failed
  download (offline) is not an error: the line is matched against installed fonts only and the
  panel shows a "Google Fonts unreachable" chip.
- **Network boundary.** `text/fonts.rs` is the only module that talks to Google, and
  `text/reader_openai.rs` the only one that talks to OpenAI (through `redraw::openai::Api`'s
  client builder and base URL rules, so `STUDI0TRACE_OPENAI_BASE` points both at the tests' mock
  server). `tests/redraw_boundary.rs` is extended to hold that.

## The AI reader

One `POST {base}/responses` call with the image and the line crops as `input_image` data URLs,
a fixed prompt, and a JSON schema reply (`lines: [{index, string, class, fonts: [{family, weight,
italic}]}]`). The model is `textModel` in Settings, default `gpt-5-mini`; an unknown model is
`refused`. The key comes from the keychain as redraw's does; without one the reader is skipped and
the panel says "Read with AI is on but no key is stored". Errors map through
`redraw::openai::parse_reply` and `transport` to the same codes (`no_key`, `invalid_key`,
`not_allowed`, `quota`, `refused`, `timeout`, `offline`, `bad_reply`, `too_large`). An AI failure
never fails the plan: the Vision reading stands and the panel shows the AI error.

## Data

```rust
pub struct TextPlan { pub image: String, pub reader: ReaderKind, pub lines: Vec<TextLine>, pub ai_error: Option<String>, pub fonts_offline: bool }
pub struct TextLine {
    pub id: String,            // "{image}-t{n}"
    pub quad: [[f32; 2]; 4],   // source pixels
    pub string: String,
    pub class: Class,          // type | script | custom
    pub colour: [u8; 3],
    pub status: Status,        // matched | declined(Reason) | off
    pub candidates: Vec<Candidate>, // ranked; each {font: FontRef, iou: f32}
    pub chosen: Option<usize>,
    pub enabled: bool,
    pub fit: Option<Fit>,      // size, baseline, x, tracking, nudges: Vec<f32>
}
pub struct FontRef { pub family: String, pub weight: u16, pub italic: bool, pub source: Source /* google | installed */ }
```

Commands (all in `commands.rs`, registered in `lib.rs`): `text_plan(image_id) -> TextPlan`
(emits `text-phase` `{id, phase}` with `finding`, `reading`, `matching`, `done`, `failed`),
`text_cancel(image_id)`, `text_set_line(image_id, line_id, change: {string?, font?, enabled?})
-> {plan, text_group}`, `text_search_fonts(query) -> Vec<FontRef>` (prefix match over the index
and installed families, 20 results). `vectorize` gains `text_group: Option<String>` in its reply.
State: `TextPlans` in the app state, one entry per image `{plan, painted: Option<Arc<Vec<u8>>>,
bytes_hash}`, forgotten with the image. Heavy work (Vision, fitting, inpainting) runs under
`redraw::blocking`; the AI call runs on the async runtime like redraw's.

Platform (`frontend/src/platform/types.ts`): `textPlan(id, {signal, onPhase})`,
`textSetLine(id, lineId, change)`, `textSearchFonts(query)`; `web.ts` rejects with
`unavailable`. `library.ts` holds `text?: TextState {plan, phase, error}` per image and
`TraceAnswer.textGroup`; `trace()` awaits the plan first when Settings say so. A plan is requested
once per image bytes; redraw accept/revert clears it.

## UI

A **Text** section in `VectorizePanel`, in a new `text` slot under the redraw slot, native only
(`CAN_TEXT`). Header row: "Text", a count ("4 lines"), the reader ("On-device" or "AI") and, while
running, the `mac-dot animate-pulse` phase row redraw uses. Per line, a row: the string in a text
input, a font combobox (the ranked candidates with their fit, then a search box over Google Fonts
and installed families), a weight select, a fit chip ("Fit 94 %") and a `Switch`. A declined line
shows "Traced as shapes — script lettering / gradient fill / no font fits / string disagrees" in
muted text with its switch off; turning it on with a chosen font forces the re-set. Hovering a row
draws its quad on the canvas through the Viewer's `marks` (an `<svg viewBox>` overlay as
`InspectorOverlay` does), with the `ring-1 ring-primary/20` tint, never a one-sided border. The
section is empty-state "No text found" when the plan has no lines, and hidden when *Recognise
text* is off.

Settings gains a **Text** group under AI redraw: *Recognise text* (toggle, default on), *Read text
with AI* (toggle, default off; switching it on with no stored key opens the redraw consent sheet
with its text-reading variant, and it stays off if the sheet is dismissed), *Model* (text, default
`gpt-5-mini`), *Hidden text in exports* (toggle, default on). The Settings window grows to
520×900. `settings.rs` gains `recognise_text`, `read_text_with_ai`, `text_model`, `hidden_text`
with those defaults; its `defaults_and_camel_case` test follows.

Copy. `PRIVACY_SENTENCE` (`frontend/src/lib/redraw.ts`, held by `redraw.test.ts` and repeated in
the README at line 76 and the consent sheet) becomes: "Your images stay on your Mac: nothing is
uploaded unless you choose AI redraw or turn on Read text with AI, which send that one image to
OpenAI with your own API key." The README's next sentence, "It is the only thing in the app that
touches the network, and it is off until you turn it on", becomes: "Those two and the fonts are
the only things in the app that touch the network: a matched Google Font is downloaded from
Google with no image attached, and the AI features are off until you turn them on." The consent
sheet gets a text-reading variant of its first paragraph ("Each image with text is uploaded to
OpenAI with your API key and billed to your OpenAI account; the reply is the text and a font name,
and the image is never changed") and the same key field.

## Errors

Stable codes, as the app's others: `text_unavailable` (Vision missing; the plan is empty and the
panel says text needs macOS 10.15), `font_download` (never surfaced as a failure — see Fonts),
and redraw's codes for the AI reader. A cancelled plan is silent. A plan failure never fails a
trace: `vectorize` traces the original bytes when there is no plan.

## Tests

- **Fixture set** `apps/desktop/src-tauri/tests/fixtures/text/`, generated by
  `tests/fixtures/text/generate.py` from three checked-in OFL fonts (Josefin Sans Light 300,
  Montserrat Regular 400, Playfair Display Regular 400; the TTFs are in the fixture directory with
  their licences): the strings "EST. 2026", "LIFE IS BETTER OUTSIDE" and "Ideas for a brighter
  tomorrow" at cap heights 12, 18 and 30 px, tracking 0 and 0.2 em, on white and on a pale
  linear gradient, each with its truth (font, size, baseline, x, tracking) in `truth.json`. The
  fonts also stand in for the download: `FontSource` is a trait, and the tests' implementation
  serves these files for their families while the real one fetches.
- **Fit** (`tests/text_fit.rs`): with the true font, size within 2 %, baseline within 0.3 px,
  tracking within 0.2 px, IoU ≥ 0.83 on every item (the 12 px Josefin Sans Light lines, whose strokes are
  one pixel wide, score 0.84–0.89; every other item is above 0.90); the true font's IoU beats each wrong
  font's on every item; `FIT_ACCEPT` sits between the two populations. The fixture glyphs are drawn at
  8× and averaged down, at the font's unhinted advances: FreeType's hinted advances are whole pixels and
  drift a line by up to 15 px from where the app's shaping puts it.
- **Shortlist** (`tests/text_match.rs`): with an index holding the three families among a
  hundred of the real index's entries, the true family ranks in the top 3 on every item.
- **Ink** (unit tests in `text/ink.rs`): colour, coverage, segments and the decline reasons on
  synthetic lines; the inpaint leaves a flat backdrop flat and a gradient backdrop within 2
  levels of the gradient.
- **Readers**: `tests/text_vision.rs` renders a fixture line and asserts Vision reads its string
  (skipped where Vision is unavailable); `tests/text_openai.rs` uses `redraw_openai.rs`'s mock
  server: the request is `POST /v1/responses` with the key only in the header, the schema reply
  is parsed, each error status maps to its code, an AI failure leaves the Vision reading.
- **Compose** (`tests/text_compose.rs`): the group's shape, one path per line, the hidden
  `<text>` present or absent with the setting, coordinates in the source grid on the upsample
  path too, and the core's renderer draws the group's paths and nothing for the `<text>`.
- **Boundary** (`tests/redraw_boundary.rs`): `worker.rs` never mentions `text`; only
  `text/fonts.rs` and `text/reader_openai.rs` mention `reqwest`.
- **Frontend**: `TextSection.test.tsx` (rows, edits call `textSetLine`, the switch re-traces, the
  declined copy, hover marks), `textsvg.test.ts` (composition and stats), settings and consent
  flow in `SettingsView.test.tsx` / `App.text.test.tsx`, `library.test.ts` for plan-before-trace
  and invalidation on redraw.
- **Manual check** on Tim's three images (never in git): every tagline re-set in a plausible
  font, scripts declined, nothing else changed. Recorded in the plan's final task.

## Out of scope

Live `<text>` with embedded fonts in the export; vertical or rotated text (a quad rotated more
than 5° is declined, reason `rotated`); gradient-filled text; a local OCR for Linux or Windows;
regularising declined lines (that is `text-as-shapes`).
