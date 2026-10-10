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
   maximum. A pixel further off that line than 0.1 of its length (`OFF_LINE`), in linear light
   and in the encoded values both (images blend their edges on the encoded values: a half-covered
   blue-on-yellow pixel is a quarter off the linear line), is another colour, not a blend of the
   two (a gold swash reaching into a dark wordmark's box): it has no coverage,
   so it is in no segment, the fit does not chase it and the paint-out leaves it alone. The ink
   mask is coverage ≥ 0.5. Per-character segments are the mask's connected components merged
   where their x ranges overlap (dots of i and j, the two parts of a colon). When the segments
   number the string's non-space characters, a gap over 1.8 × the median gap and over the median
   plus 0.35 of the ink's height, with no space in the string there and not between two digits (a
   tabular 1 has a quarter em of air either side), gets one (`repair_spaces`): Vision
   drops the space in widely tracked text ("ABRIGHTER", "LIFEIS"), and the line keeps the
   repaired string. A line is **declined** — left to the trace — when the reader classed it
   `script` or `custom`, when the ink's spread is over 8 (gradient or multicolour text), or when
   the segment count differs from the string's non-space character count by more than a third
   (the string and the ink disagree). The spread is the range of the flat ink pixels' colours:
   the 90th minus the 10th percentile of their positions along their principal axis in Lab, so a
   ramp of span D reads 0.8 D (not their distance from the ink colour, which on large display
   letters with grain is their darkest specks, 7–11 ΔE from a body of one colour), and only the
   characters' pixels count: the components overlapping the quad as Vision gave it whose height
   is 0.4–1.3 of the line's, so a rule or divider in the grown box is not a second ink.
3. **Match.** Candidates, in order: the AI hints; then a keyless shortlist from the bundled
   Google Fonts index and the installed fonts, ranked by how the line's measured per-character
   bbox widths match each family's. The widths are over the line's cap height, estimated from
   the median vertical extent of its capital letters (else its digits, else its tall lowercase
   letters over 1.07; the ink's own height carries ascenders and descenders and made every
   lowercase width read narrow). Each family is compared at its own best width scale (least
   squares), because weight moves every width together, and is charged `0.07 × |ln scale|` for
   the scale it needed; the penalty was swept on the fixture set (0.05 to 0.4) and the shortlist
   gate is sensitive to it. The distance is the mean absolute width difference plus that
   charge. The stem is not in the ranking: the line's stem is a run of ink pixels and a
   family's is its I's bbox width, which for a serif is the serifs' width (Playfair Display's
   I is 0.36 of the cap, its stems about 0.15), and a family's regular face says nothing about
   its other weights. The stem picks the weight only (`weight_for_stem`, clamped to the
   family's variants). A line whose letters touch has fewer ink segments than characters, so
   it has no per-character widths and is not ranked: its shortlist is empty and the reader's
   hints carry it. The top 12 families are taken; a family present both in the index and
   installed is listed once, as installed (no download). A line with no reader hints then also
   tries sixteen plain sans families after its ranked ones (`catalog::SANS_FLOOR`: Montserrat,
   Poppins, Jost, Josefin Sans, Raleway, Quicksand, Nunito Sans, Lato, Inter, Work Sans, DM Sans,
   Manrope, Urbanist, Outfit, Red Hat Display, Kumbh Sans, at the stem's weight): the width
   ranking missed them for a widely tracked wordmark. A family that won another line of the same
   image is moved to the front of every other line's list (a logo uses one or two fonts).
   Every font on the list is **screened** with a quick fit (`fit_line_quick`: one start of each
   layout, no polish, no nudges, a tenth of the cost); after the first four (plus the hints) the
   screening stops once a fit of the ink's weight reaches `FIT_SURE` (0.84, what a true font
   scores on a crisp line; a real source's right face scores less, so its whole list is
   screened). The three best families by IoU then have their neighbouring weights ±100 screened,
   and their 200 and 300 when the ink's stroke is under 0.08 of its height (a face at the wrong
   weight scores under a worse face at the right one); in `plan` the Google ones are downloaded
   first, in `set_line` only the cached ones are used. The four most promising screens (those of
   the ink's weight first, by IoU less the stroke's charge) are fitted in full, and only a full
   fit can be chosen.
4. **Fit.** For each candidate: shape the string with `rustybuzz` (kerning and ligatures off),
   outline the glyphs with `ttf-parser`, and solve size (px per em), baseline y, start x and
   tracking (px added per gap) by coordinate descent on the soft IoU between the rendered coverage
   (tiny-skia, anti-aliased, one pixmap reused per thread) and the ink coverage, from an initial
   guess read off the string's own outlines (size from the ink height over the outlines' height,
   baseline from how far they reach below it, start from the ink's left, tracking from its
   width), started from five sizes either side; the coarse stage descends on both images blurred,
   the fine one on the sharp. When the ink's segments match the outlined glyphs one to one, a
   second layout is fitted from the ink (`ink_layout`): each glyph's outline centred on its own
   segment, only size, baseline and a shift solved, written back as the least-squares tracking
   (the mean gap) plus per-glyph nudges, since AI-made logos space letters unevenly and their
   word gaps are wider than their letter gaps. Both are polished (descent at 0.25 and 0.1 px,
   then per-glyph nudges of ±2 px around where each glyph is, scored on that glyph's window,
   kept where the line's IoU rises by 0.002, and descent again, twice) and the better kept; on
   evenly spaced text the tracking model wins. A line taller than 64 px is searched on a copy
   reduced 2× (4× above 128 px) and polished at its own size. The fit carries `stroke`
   (`fit::stroke_ratio`): the median horizontal stroke at mid-height of the drawn string over the
   ink's, each stroke its summed coverage across the row. A candidate is **acceptable** when its
   IoU is at least `FIT_ACCEPT` (0.6, a floor set on real sources: a right face there scores
   0.67–0.83 against the crisp fixtures' 0.84 up) and its stroke is within 35 % of the ink's
   (`STROKE_AGREE`). Among the acceptable full fits, those within 0.05 IoU of the best are a tie
   the IoU cannot break (on a hairline it is edge-dominated, and a 500 outscores the 300 the
   source is); it goes to the best IoU less 0.25 per unit of |ln stroke|. No acceptable fit
   declines the line (`no_fit`).
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
   gets `<text x y font-family font-size fill="none" pointer-events="none" data-line>string</text>`, its `x`
   one value per character, each glyph's origin with its nudge (spaces at their computed place),
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
uploaded unless you choose AI redraw or turn on Read text with AI, which send the image you are
tracing to OpenAI with your own API key." The README's next sentence, "It is the only thing in the
app that touches the network, and it is off until you turn it on", becomes: "Those two, the fonts
and the update check are the only things in the app that touch the network: a matched Google Font
is downloaded from Google and the update check asks GitHub for the newest version, neither with an
image attached, and the AI features are off until you turn them on." The consent
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
  one pixel wide, score 0.84–0.89; every other item is above 0.90) and the stroke within `STROKE_AGREE` of the
  ink's (0.91–1.14); the true font's IoU beats each wrong font's on every item; `FIT_ACCEPT` sits under every
  true font with a 0.04 margin; Montserrat Regular's stroke is refused on every all-caps Josefin Sans Light
  line of 18 px and up. `FIT_ACCEPT` is no longer above every wrong font here (24 wrong fits pass the
  acceptance, each beaten by the true font): it is a floor set on real sources, and the stroke is what keeps a
  wrong weight off. The fixture glyphs are drawn at 8× and averaged down, at the font's unhinted advances:
  FreeType's hinted advances are whole pixels and drift a line by up to 15 px from where the app's shaping
  puts it. Unit tests in `text/fit.rs`: a line drawn with uneven letter gaps and a wide word gap is laid out
  from its ink (every glyph within 0.75 px); a heavy candidate on a hairline reads heavier than the ink and the
  true one reads even; a nudge scored on its window scores what the whole line does; a 140 px line found on a
  reduced copy lands on its ink at full size.
- **Shortlist** (`tests/text_match.rs`): with the real index (every family, nothing installed),
  the true family is in the top `SHORTLIST` (12) on every fixture item that has per-character
  widths (cap height ≥ 18 px; 64 of 72 items), and the items whose letters touch (no widths, so
  no ranking) are at most a fifth of the ranked ones. Top 3 is met on 40 of the 64: Josefin
  Sans Light ranks worst, since the index holds regular weights only. Unit tests in
  `text/catalog.rs`: a line with no widths gets an empty shortlist, and a family both in the
  index and installed appears once, as installed.
- **Ink** (unit tests in `text/ink.rs`): colour, coverage, segments and the decline reasons on
  synthetic lines; the inpaint leaves a flat backdrop flat and a gradient backdrop within 2
  levels of the gradient; a dropped word space is put back and nothing else changes, and a
  tabular 1 among tracked digits is not set apart; a rule or a divider of another colour in the
  grown box is not a gradient, large letters with grain and darker specks are not one either, a
  14 ΔE ramp across wide letters is one and a 4 ΔE ramp is not; a neighbour's swash in the box is
  neither ink nor painted out, while blue ink on yellow keeps its encoded-blend edges half covered
  (`text/fit.rs`). Unit tests in `text/plan.rs`: a fit of the wrong weight is not acceptable, the light weight
  beats a heavier one that outscores it on a hairline, only a full fit is chosen, and the three
  best families' neighbours (and light weights on a hairline) are screened. `text/catalog.rs`:
  the sans floor is the families the catalogue holds at the stem's weight.
- **Real-image gate** (`tests/text_examples_gate.rs`, ignored, `STUDI0TRACE_TEXT_EXAMPLES`): Tim's
  three images planned with Vision, the real catalogue and real downloads; every tracked
  all-caps tagline Matched (image 1 `A BRIGHTER TOMORROW`; image 2 `EST. 2026`, `DENVER, CO`,
  `LIFE IS BETTER OUTSIDE`, `FIND WHAT MATTERS`, `MORE EVERY DAY`, `REACH HIGHER` and 6 of the 7
  serif small words; image 3 `IDEAS FOR A BRIGHTER TOMORROW`, `EST. 2026`) under the repaired
  string, every Matched line's stroke the ink's, the scripts and `Creative` declined, `Wander`
  declined or Matched in a serif, `LUMORA`/`HORIZON`/`ADVENTURE`/`SUMMIT`/the long line Matched
  only in a sans, image 3 under 25 s. It writes `N-plan.json` and `N-check.png`, and the crops of
  every Matched line are to be looked at against the source: the eye is part of it.
- **Readers**: `tests/text_vision.rs` renders a fixture line and asserts Vision reads its string
  (skipped where Vision is unavailable); `tests/text_openai.rs` uses `redraw_openai.rs`'s mock
  server: the request is `POST /v1/responses` with the key only in the header, the schema reply
  is parsed, each error status maps to its code, an AI failure leaves the Vision reading.
- **Compose** (`tests/text_compose.rs`): the group's shape, one path per line, the hidden (with one x per
  character, a nudged glyph's included)
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

## Status

2026-10-09, after real-image tuning (Task 16).

**The three example images** (`tests/text_examples_gate.rs`, Vision and no key, real Google downloads, fonts
cached): the gate passes. 19 lines matched of 25, every tracked tagline among them, each in the ink's weight
(stroke 0.89–1.09 of the ink's), the crops looked at against the source.

- 1 (2 lines, 2 matched, 6.9 s): `LUMORA` Kumbh Sans 400 (IoU 0.826, stroke 0.99); `A BRIGHTER TOMORROW` (space
  put back) Clarity City 200 (0.770, 1.01).
- 2 (19 lines, 14 matched, 22.4 s): `DENVER, CO`, `FIND WHAT MATTERS`, `MORE EVERY DAY`, `REACH HIGHER` Red Hat
  Display 400 (0.72–0.80); `EST. 2026` Red Hat Display 600 (0.810); `LIFE IS BETTER OUTSIDE` Red Hat Display 500
  (0.794); `MOUNTAINS / FORESTS / LAKES / NEW PERSPECTIVES` Clarity City 300 (0.780); `GOOD` Balthazar 400;
  `EXPLORE`, `PEOPLE`, `GREAT`, `BELONG` Castoro 400; `CREATE`, `PLACES` Brygada 1918 500 (serifs 0.72–0.79).
  Declined: `ADVENTURE` no_fit (0.40), `SUMMTT` (Vision's SUMMIT) no_fit, `Awaits` gradient (its box holds the
  dark ADVENTURE too), `Wander` and `Explore` mismatch.
- 3 (4 lines, 3 matched, 6.6 s): `HORIZON` Red Hat Display 700 (0.755, 1.07); `IDEAS FOR A BRIGHTER TOMORROW`
  Montserrat 400 (0.732, 1.00); `EST. 2026` Red Hat Display 500 (0.750). `reative` (Creative) gradient.

What changed, in order: the dropped word spaces put back from the ink; each glyph laid on its own segment; the
acceptance an IoU floor (0.6) plus the stroke within 35 % of the ink's, ties broken by the stroke, the best
families' other weights fitted; the gradient judged on the range of the characters' flat pixels; a
pixel off the ink's colour line not ink; the sans floor; every font screened and four fitted in full, display
lines searched reduced. Image 3 went from 64 s to 7 s; image 2 stayed at 22 s (21.9 s before) while trying
twice the fonts.

**Suites.** `cargo test --release` in `apps/desktop/src-tauri` green (`text_fit` 65 s, was 76 s); `cargo clippy
--release --all-targets` reports nothing from `apps/desktop`.

**Open.** The stroke measure reads mid-height rows of the ink's height, which on a lowercase line cross the
bowls: there it tells a 300 from a 400 less well (Montserrat Regular on Josefin Light's lowercase line reads
1.1–1.2). The weight the stem asks for still comes from whole-pixel runs (`measured_stem`), the neighbours make
up for it. `FIT_ACCEPT` is a floor: a wrong face of the right weight can pass it where the right face is not in
the list (24 wrong fits on the fixtures pass it, each beaten by the true font). Run times are with the fonts
cached; a first run adds their download. The ink colour is still the darkest pixels' (on large display letters,
a little darker than their body).

### 2026-10-09, after the first full build (Tasks 1-15)

**Suites.** `frontend`: 294 tests in 38 files pass, `npm run build` clean. `apps/desktop/src-tauri`:
`cargo test --release` green (154 unit tests and every integration binary; `text_examples` is `#[ignore]`d), and
`cargo test --workspace --release` 495 passed, 0 failed. `cargo clippy --release --all-targets` reports nothing
from `apps/desktop` (every warning is in `backend/vexel-rs`, as on `main`).

**The three example images** (`tests/text_examples.rs`, Vision and no key, Google Fonts fetched for real; the
images are not in git). Result: **no line was matched.** The feature declined everything it was given and
re-set nothing wrongly, but the expectation above (every tagline re-set in a plausible font) is not met on
these images. Per image, lines found / matched, run time:

- 1 (1254², 2 lines, 0 matched, 22.8 s). `LUMORA` no_fit, best Kumbh Sans 500 at IoU 0.773. `ABRIGHTER TOMORROW`
  (Vision dropped the space after the A) no_fit, best Clarity City 400 at 0.547.
- 2 (1536x1024, 19 lines, 0 matched, 27.8 s). no_fit, best candidate and IoU: DENVER, CO Orlean 0.732; EST. 2026
  Preahvihear 0.724; GOOD Balthazar 0.705; EXPLORE Castoro 0.775; PEOPLE Castoro 0.757; CREATE Brygada 1918 600
  0.734; GREAT Sree Krushnadevaraya 0.734; BELONG Lustria 0.728; PLACES Rosarivo 0.738; `LIFEIS BETTER OUTSIDE`
  Varela 0.457; FIND WHAT MATTERS Varela 0.673; MORE EVERY DAY Red Hat Display 600 0.652; REACH HIGHER NTR 0.719;
  MOUNTAINS / FORESTS / LAKES / NEW PERSPECTIVES Orlean 0.330. Declined for another reason: ADVENTURE `fonts` (no
  candidate fitted), Awaits / Wander / Explore / `SUMMTT` (SUMMIT) `gradient` (the ink is not one colour).
- 3 (1536x1024, 4 lines, 0 matched, 64.0 s). HORIZON no_fit, Tamil MN 700 0.563 (the source is a plain geometric
  sans; the shortlist did not reach one); `reative` (Creative) `gradient`; `IDEAS FOR ABRIGHTER TOMORROW` Questrial
  0.350 (0.666 Brooklyn with the space put back); EST. 2026 Attort 0.588.

What was found (probes kept out of git: an oracle reader that returns the corrected strings, and `set_line` with
a forced font, to look at what the fit draws):

- *`FIT_ACCEPT` (0.79) is calibrated on crisp synthetic lines and these sources are not.* They are AI-generated:
  a hairline stroke one pixel off scores far below what the eye sees. Forced into the group, EXPLORE in Castoro
  (0.775) and LIFE IS BETTER OUTSIDE in Varela (0.57, space fixed) are visually close to the source, while CREATE in
  Brygada 600 (0.734) is visibly too heavy and HORIZON in Tamil MN is the wrong face. The score alone does not
  separate them; a stroke-weight ratio would.
- *The weight is rarely tried.* `with_weight_neighbours` runs only when the top two candidates are within 0.03.
  Trying the neighbours always flips exactly one line: LUMORA to Kumbh Sans 400 at 0.811 (a plausible match). A
  weight sweep of Montserrat, Raleway, Kumbh Sans, Josefin Sans, Quicksand, Jost and Poppins over the sans lines found
  EST. 2026 (image 2) at Montserrat 500 0.807, but that family is not on that line's shortlist.
- *A string with a missing space fits worse and shortlists worse* (`ABRIGHTER`, `LIFEIS`, `IDEAS FOR ABRIGHTER`): the
  AI reader is the intended fix and was not run (the harness wires no reader). With the spaces put back the IoUs rose by
  0.03-0.12 and still stayed under the threshold.
- *Wide, uneven tracking* (the taglines) is the fit's weak spot: one tracking value for the whole line, the word gaps
  of these sources are wider than a letter gap, and the per-glyph nudges stop at 2 px.
- *Run time is dominated by large lines:* 64 s for 4 lines on image 3 (HORIZON at 185 px), 22 s for 2 lines on image
  1; image 2's 19 small lines take 28 s.

**Painted-out and composed.** Nothing matched, so the real run wrote no painted PNG. With every line that had a
candidate forced into the group (`set_line` with the first candidate's font), the painted images traced through
the Rust engine (balanced preset: 3.6 s, 15.8 s, 8.6 s) show no trace of the lines (the painted crops of image 1's
wordmark and tagline and of image 3's tagline trace to blank) and the composed result is the engine's SVG plus
the group, so the pipeline works end to end. A faint remnant sits at the truncated last E of LIFE IS BETTER
OUTSIDE (image 2).

**Settings window.** The sheet was rendered in a browser at the window's 520x850 in light and dark: the Text group
is whole and correct in both, the consent sheet opens from *Read text with AI* with no key. The sheet is **968 px
tall** (955 without the new Model hint), so the fixed, non-resizable window scrolls 118 px; the Text group's heading
sits at y≈711 of the window's 850, and the rest of the group, *Hidden text in exports* last, is reached by scrolling.
The window stays at 850 (a 13″ display cannot show 980) and the sheet scrolls.

**Not verified.** The real app was not launched: Tim's installed Studi0Trace was running, `npm run smoke` refuses to
run beside it, and a second `tauri dev` instance would share its settings file and Keychain. So the main window's
Text section, the hover mark, a string edit and the switch were checked by the frontend suites only, not on screen;
the Keychain state was not read.

**Deferred.** The acceptance rule (stroke weight beside the IoU, and a threshold set on real sources), always
trying the weight neighbours and widening the shortlist for wide-tracked lines, a word-gap parameter in the fit,
and a speed budget for large lines each want their own task and a bench of real sources; none was changed here
because the threshold and the shortlist are what keep a wrong face off the canvas.

## Final fix wave

2026-10-09, after the whole-branch review (commits 44e88fc..120a319).

**What changed.**

- *Re-trace on the answer.* `set_line`'s answer carries `repainted` (`text::repainted`: the painted PNG before and
  after differ), and the store traces again exactly when it is true, else composes again. A font picked for a
  declined or Off line used to recompose over traces that still held the old pixels, drawing the text twice.
- *The AI reader is never handed an image Vision found no text in* (it cannot add a line).
- *Stale text.* A finished trace reads its group again (`Plans::group_for`: the group now, while the painted bytes
  are still the ones it traced), so an edit made while it ran lands; `text_set_line` hashes off the async runtime
  and stores with `Plans::put_if` (only while the entry still has the bytes' hash; otherwise `expired`), and the
  store drops an answer whose image object changed while it was awaited.
- *Load.* At most two plans run at once (`TextState::plan_permits`, held by the plan's task); a font is written to
  a `.part` of its own and renamed into place, stale parts are swept when the cache is made, and one `ensure`
  fetches six fonts at a time (offline on any transport failure, missing in the order asked).
- *Progress, cancel and retry.* While the plan runs the image holds a job of phase `text` (the trace button busy
  with Cancel, the canvas "Reading text…"); a cancel lets the next trace ask again; a failed plan shows its error
  and Try again (`retryText`, which also traces again when the new plan re-sets a line, since the traces on screen
  were made without it); an untraced image's Text header says "Trace to read the text".
- *Settings reach cached work.* The group always carries the hidden `<text>`; `composeSvg(base, group,
  { hiddenText })` leaves it out when the setting is off, and a change of the setting recomposes every trace.
  Turning Recognise text off drops every trace that carries a group.
- *Validation and hygiene.* Control characters XML cannot carry are dropped from the readers' strings, from an
  edit and in `escape_text`. `set_line` refuses (`bad_request`) a Google font the index does not hold, an installed
  one this Mac does not have and a string over 3 × the line's length + 16. A string edit of a line with no
  candidates downloads its shortlist before the fit. Vision runs in an autorelease pool, and a failed request is
  `text_failed` ("Text recognition failed."), not "needs macOS 10.15". `TextFit.stroke` is in the frontend's type.
- *Docs and copy.* The privacy sentence names "the image you are tracing", the README names the update check,
  CLAUDE.md describes the text module and the 520×850 window, the fixture fonts' OFL.txt carries all three
  copyright lines.

**Deviations from the design, recorded.** There is no weight select in the font picker: the search answers each
family at its weight nearest 400, and a candidate's weight is whatever the fit chose. A trace's stats are not
recomputed with `svgStats` after the group is composed (the counts are the engine's, without the text). There is
no `App.text.test.tsx`: the wiring is covered by `library.test.ts`, `TextSection.test.tsx` and
`TraceButton.test.tsx`. The Settings window is 520×850, not the 520×900 above, and its sheet scrolls.

**Suites.** `frontend`: 304 tests in 38 files, `tsc --noEmit` and `npm run build` clean. `apps/desktop/src-tauri`:
`cargo test --release` green (173 unit tests, `text_compose` 20, `text_fonts` 7, `text_fit` 65 s). `cargo clippy
--release --all-targets -p studi0trace-desktop` reports nothing from `apps/desktop`; it stops on `backend/vexel-rs`'s
`eq_op` (`rects.rs:109`, deny-by-default in clippy 1.98, not on this branch) unless that lint is allowed.

**The gate** (`text_examples_gate`, fonts cached): passes, 19 of 25 lines matched as before — image 1 2/2 (7.5 s),
image 2 14/19 (24.7 s), image 3 3/4 (6.7 s), the same faces, weights and IoUs as after Task 16.
