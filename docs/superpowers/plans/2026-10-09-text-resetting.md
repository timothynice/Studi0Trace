# Text Re-setting Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The Mac app recognises each line of text in an image, finds its font (Google Fonts or installed), fits the string in that font onto the pixels, hands the trace an image with the text painted out, and composes the glyph outlines on top — so small text comes out as consistent letterforms, editable in a Text panel.

**Architecture:** Everything lives in the desktop crate (`apps/desktop/src-tauri/src/text/`) beside `redraw/`; the engine, the core and the trace worker are untouched. macOS Vision finds and reads lines (on device); an optional OpenAI reader corrects strings and names fonts; a bundled Google Fonts index plus `fontdb`'s installed fonts give candidates; `rustybuzz`/`ttf-parser` shape and outline; `tiny-skia` rasterises for the fit; the frontend composes `base SVG + text group` for the viewer, Auto candidates and export, and shows a Text section under AI redraw.

**Tech Stack:** Rust 1.90 (Tauri 2.12, objc2 0.6 + objc2-vision 0.3, objc2-foundation 0.3, fontdb 0.23, rustybuzz 0.20 (re-exports ttf-parser), tiny-skia 0.11, reqwest 0.13 as already configured, image 0.25), React 18 + TypeScript + vitest, Python 3.12 + Pillow for the fixture generator and the index tool.

Spec: `docs/superpowers/specs/2026-10-09-text-resetting-design.md`. Read it first.

## Global Constraints

- The engine (`backend/`), the core (`crates/studi0trace-core`) and `apps/desktop/src-tauri/src/worker.rs` are not changed. `tests/redraw_boundary.rs` holds `worker.rs` free of `reqwest`, `redraw`, `keychain`, `openai` and, after Task 10, `text`.
- Only `src/text/fonts.rs` talks to Google (`fonts.googleapis.com`, `fonts.gstatic.com`) and only `src/text/reader_openai.rs` talks to OpenAI, through `redraw::openai::base_url()` so `STUDI0TRACE_OPENAI_BASE` points both at the tests' mock server. No other file in `src/text/` mentions `reqwest`.
- Every error that reaches the frontend carries a stable code: redraw's (`no_key`, `invalid_key`, `not_allowed`, `quota`, `refused`, `timeout`, `offline`, `bad_reply`, `too_large`, `cancelled`) plus `text_unavailable`. The UI chooses words by code, never from a message.
- Phase event `text-phase` with payload `{ "id": <image id>, "phase": "finding" | "reading" | "matching" | "done" | "failed" }`.
- Settings fields and defaults: `recogniseText: true`, `readTextWithAi: false`, `textModel: "gpt-5-mini"`, `hiddenText: true` (camelCase in JSON; `recognise_text`, `read_text_with_ai`, `text_model`, `hidden_text` in Rust).
- The text group's SVG: `<g id="text">` containing, per matched enabled line, `<path d="…" fill="#rrggbb" data-line="<id>"/>` and, when `hiddenText`, `<text x="…" y="…" font-family="…" font-size="…" fill="none" pointer-events="none" data-line="<id>">string</text>`. Coordinates are source pixels. Numbers are written with at most 2 decimals.
- Shaping runs with kerning **off** (`kern` feature disabled); per-glyph x nudges of up to ±2 px absorb spacing differences.
- Constants (all `pub const` in `text/mod.rs`): `FIT_ACCEPT` (set in Task 4 from the fixture set: the lowest true-font IoU minus 0.05), `INK_SPREAD_MAX = 8.0` (ΔE), `MISMATCH_SHARE = 0.34`, `ROTATED_DEG = 5.0`, `SHORTLIST = 12`, `NUDGE_MAX = 2.0`, `VISION_MIN_CONFIDENCE = 0.3`.
- Privacy copy, verbatim (Task 14): `PRIVACY_SENTENCE` = "Your images stay on your Mac: nothing is uploaded unless you choose AI redraw or turn on Read text with AI, which send that one image to OpenAI with your own API key."
- Never a one-sided coloured border in the UI: the hover mark on the canvas is a dashed `hsl(var(--brand-accent))` rect as `InspectorOverlay` draws, the panel's hint style is `mac-tint` + `mac-dot`.
- Commits after every task, message in the repo's style (a sentence saying what and why), ending with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Commands to run: Rust `cd apps/desktop/src-tauri && cargo test --release <name>` (the crate embeds `frontend/dist`, so run `cd frontend && npm run build` once before the first cargo build); frontend `cd frontend && npm run test:run -- <file>`; type check `cd frontend && npx tsc --noEmit -p .`.
- Tim's three example images are under the session scratchpad at `/private/tmp/claude-502/-Users-TimNice-Development-tracer--claude-worktrees-svg-text-handling-fbf2c8/33ac6b5f-d9f7-489b-ab30-1b0e7edbf73b/scratchpad/text/{1,2,3}.png`. They are never copied into the repo.

## File map

Rust (`apps/desktop/src-tauri/`):
- `src/text/mod.rs` — types (`TextPlan`, `TextLine`, `FontRef`, `Fit`, `Status`, `Reason`, `Class`, `Candidate`, `Phase`, `ReaderKind`), constants, `error()`, `Plans` (the per-image cache), `plan()` (the orchestration), `set_line()`.
- `src/text/glyphs.rs` — a loaded font (`Font`), shaping (`shape`), outlines as `tiny_skia::Path` in em units, feature measurement (`measure`).
- `src/text/ink.rs` — `LineInk` from a quad: backdrop, ink colour, coverage, mask, segments, decline reasons; `paint_out`.
- `src/text/fit.rs` — `fit_line` (coordinate descent + nudges), `render_coverage`, `soft_iou`.
- `src/text/catalog.rs` — `Catalog`: the Google index (`assets/gfonts.json`) + installed fonts; `shortlist`, `search`, `locate`.
- `src/text/fonts.rs` — `GoogleFonts`: download and cache (the only Google network code), css2 parsing.
- `src/text/reader.rs` — `LineBox`, `Reading`, `FontHint`, `Reader` trait, `Finder` trait.
- `src/text/vision.rs` — macOS Vision finder+reader.
- `src/text/reader_openai.rs` — the AI reader (Responses API).
- `src/text/compose.rs` — `text_group`.
- `src/commands.rs` — `text_plan`, `text_cancel`, `text_set_line`, `text_search_fonts`; `vectorize` uses painted bytes and returns `text_group`.
- `src/lib.rs` — `AppState.texts`, command registration. `src/settings.rs` — four fields.
- `assets/gfonts.json` (generated), `tools/gfonts_index.py`.
- `tests/fixtures/text/` (`generate.py`, `truth.json`, `fonts/*.ttf` + licences, `*.png`), `tests/text_fit.rs`, `tests/text_match.rs`, `tests/text_vision.rs`, `tests/text_openai.rs`, `tests/text_compose.rs`, `tests/redraw_boundary.rs` (extended).

Frontend (`frontend/src/`):
- `platform/types.ts` (types, Settings, Platform methods), `platform/native.ts`, `platform/web.ts`.
- `lib/textsvg.ts` (compose), `lib/text.ts` (copy: phases, reasons, errors).
- `state/library.ts` (TextState, plan-before-trace, `textGroup`, `setTextLine`, `toggleTextLine`).
- `components/TextSection.tsx`, `components/TextOverlay.tsx`, `components/SettingsView.tsx`, `components/RedrawConsentSheet.tsx` (variant), `App.tsx`, `lib/redraw.ts` (privacy sentence), `README.md`.

---

### Task 1: Fixture fonts and the generated text fixture set

**Files:**
- Create: `apps/desktop/src-tauri/tests/fixtures/text/generate.py`
- Create: `apps/desktop/src-tauri/tests/fixtures/text/fonts/JosefinSans-Light.ttf`, `Montserrat-Regular.ttf`, `PlayfairDisplay-Regular.ttf`, `OFL.txt` (one copy; all three are OFL 1.1)
- Create (generated): `apps/desktop/src-tauri/tests/fixtures/text/truth.json`, `apps/desktop/src-tauri/tests/fixtures/text/*.png`

**Interfaces:**
- Produces: `truth.json`, a JSON array of items `{ "file": "josefin-18-t0-white.png", "font": "JosefinSans-Light.ttf", "family": "Josefin Sans", "weight": 300, "string": "EST. 2026", "cap_px": 18.0, "size_px": <em px>, "baseline": <y>, "x": <x of the first glyph's origin>, "tracking_px": <px per gap>, "backdrop": "white" | "gradient", "colour": [r,g,b] }`. Items: 3 fonts × 3 strings × cap 12/18/30 × tracking 0/0.2 em × white/gradient = 108 PNGs, each 40 px taller than the cap and 60 px wider than the text, the text starting at x = 30 with the baseline at y = cap_px + 20.

- [ ] **Step 1: Fetch the three fonts (one-off, by hand, not a build step)**

```bash
cd apps/desktop/src-tauri/tests/fixtures/text && mkdir -p fonts && cd fonts
for spec in "Josefin+Sans:wght@300|JosefinSans-Light" "Montserrat:wght@400|Montserrat-Regular" "Playfair+Display:wght@400|PlayfairDisplay-Regular"; do
  q=${spec%%|*}; out=${spec##*|}
  url=$(curl -s -A 'Studi0Trace-fixtures' "https://fonts.googleapis.com/css2?family=$q" | grep -o 'https://fonts.gstatic.com[^)]*\.ttf' | head -1)
  curl -s -o "$out.ttf" "$url" && ls -l "$out.ttf"
done
curl -s -o OFL.txt https://raw.githubusercontent.com/google/fonts/main/ofl/josefinsans/OFL.txt
```

Expected: three TTFs of 100–300 KB and `OFL.txt`. (Montserrat and Playfair Display are OFL too; one licence text covers the three, and each font's own copyright line is in its name table.)

- [ ] **Step 2: Write the generator**

`apps/desktop/src-tauri/tests/fixtures/text/generate.py` (run with the backend venv, which has Pillow):

```python
"""Renders the text fixture set: three OFL fonts, three strings, three cap heights, two trackings, two
backdrops, with the truth of every rendering in truth.json. Glyphs are drawn one at a time with the
font's own advances and no kerning, as the app shapes them (kern off)."""
import json
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
FONTS = [("JosefinSans-Light.ttf", "Josefin Sans", 300), ("Montserrat-Regular.ttf", "Montserrat", 400), ("PlayfairDisplay-Regular.ttf", "Playfair Display", 400)]
STRINGS = ["EST. 2026", "LIFE IS BETTER OUTSIDE", "Ideas for a brighter tomorrow"]
CAPS = [12.0, 18.0, 30.0]
TRACKINGS = [0.0, 0.2]  # em
INK = (24, 28, 44)


def cap_ratio(path: str) -> float:
    """Cap height over em, measured on H at 1000 px."""
    f = ImageFont.truetype(path, 1000)
    _, top, _, bottom = f.getbbox("H", anchor="ls")
    return (bottom - top) / 1000.0


def render(font_path, cap_px, tracking_em, string, backdrop):
    size = cap_px / cap_ratio(font_path)
    font = ImageFont.truetype(font_path, size)
    tracking_px = tracking_em * size
    advances = [font.getlength(ch) for ch in string]
    width = int(round(sum(advances) + tracking_px * (len(string) - 1))) + 60
    height = int(round(cap_px)) + 40
    img = Image.new("RGB", (width, height), (255, 255, 255))
    if backdrop == "gradient":
        px = img.load()
        for y in range(height):
            for x in range(width):
                t = x / max(1, width - 1)
                px[x, y] = (int(255 - 40 * t), int(250 - 20 * t), int(245 + 10 * t))
    draw = ImageDraw.Draw(img)
    x, baseline = 30.0, cap_px + 20.0
    for ch, adv in zip(string, advances):
        draw.text((x, baseline), ch, font=font, fill=INK, anchor="ls")
        x += adv + tracking_px
    return img, size, baseline, tracking_px


def main():
    truth = []
    for file, family, weight in FONTS:
        for string in STRINGS:
            for cap in CAPS:
                for tr in TRACKINGS:
                    for backdrop in ("white", "gradient"):
                        slug = f"{family.split()[0].lower()}-{string.split()[0].lower().strip('.')}-{int(cap)}-t{int(tr * 10)}-{backdrop}.png"
                        img, size, baseline, tracking_px = render(str(HERE / "fonts" / file), cap, tr, string, backdrop)
                        img.save(HERE / slug)
                        truth.append({"file": slug, "font": file, "family": family, "weight": weight, "string": string, "cap_px": cap, "size_px": size, "baseline": baseline, "x": 30.0, "tracking_px": tracking_px, "backdrop": backdrop, "colour": list(INK)})
    (HERE / "truth.json").write_text(json.dumps(truth, indent=1))
    print(len(truth), "items")


if __name__ == "__main__":
    main()
```

- [ ] **Step 3: Generate and look**

Run: `cd apps/desktop/src-tauri/tests/fixtures/text && ../../../../../../backend/.venv/bin/python generate.py`
Expected: `108 items`, 108 PNGs, `truth.json`. Open two of them (Read tool) and confirm the text is drawn, baseline 20 px above the bottom.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/tests/fixtures/text
git commit -m "text fixtures: three OFL fonts and 108 rendered lines with their truth, for the fit and shortlist tests"
```

---

### Task 2: Types, dependencies and `glyphs.rs` (load, shape, outline, measure)

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml` (dependencies)
- Create: `apps/desktop/src-tauri/src/text/mod.rs`, `apps/desktop/src-tauri/src/text/glyphs.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs:1-16` (`pub mod text;`)

**Interfaces:**
- Produces (`text/mod.rs`):

```rust
pub const FIT_ACCEPT: f32 = 0.82; // provisional; Task 4 sets it from the fixture set
pub const INK_SPREAD_MAX: f32 = 8.0;
pub const MISMATCH_SHARE: f32 = 0.34;
pub const ROTATED_DEG: f32 = 5.0;
pub const SHORTLIST: usize = 12;
pub const NUDGE_MAX: f32 = 2.0;
pub const VISION_MIN_CONFIDENCE: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum Source { Google, Installed }
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontRef { pub family: String, pub weight: u16, pub italic: bool, pub source: Source }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum Class { Type, Script, Custom }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "snake_case")]
pub enum Reason { Script, Custom, Gradient, Mismatch, NoFit, Rotated, Fonts }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase", tag = "kind", content = "reason")]
pub enum Status { Matched, Declined(Reason), Off }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fit { pub size: f32, pub baseline: f32, pub x: f32, pub tracking: f32, pub nudges: Vec<f32>, pub iou: f32 }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate { pub font: FontRef, pub iou: f32 }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct TextLine { pub id: String, pub quad: [[f32; 2]; 4], pub string: String, pub class: Class, pub colour: [u8; 3], pub status: Status, pub candidates: Vec<Candidate>, pub chosen: Option<usize>, pub enabled: bool, pub fit: Option<Fit> }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
pub enum ReaderKind { Vision, Ai }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] #[serde(rename_all = "camelCase")]
pub struct TextPlan { pub image: String, pub reader: ReaderKind, pub lines: Vec<TextLine>, pub ai_error: Option<String>, pub fonts_offline: bool }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)] #[serde(rename_all = "lowercase")]
pub enum Phase { Finding, Reading, Matching, Done, Failed }
pub fn error(code: &'static str) -> CommandError; // "text_unavailable" → 501; anything else through redraw::error
```

- Produces (`text/glyphs.rs`):

```rust
pub struct Font { data: Vec<u8>, index: u32 } // owns the bytes; `face()` parses on demand (rustybuzz::Face borrows)
impl Font {
    pub fn load(path: &Path, index: u32) -> Result<Font, CommandError>;   // io_error on a missing file, bad_font (422) when it will not parse
    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<Font, CommandError>;
    pub fn face(&self) -> rustybuzz::Face<'_>;
    pub fn units_per_em(&self) -> f32;
    pub fn cap_height(&self) -> f32;   // over em; from the OS/2 table, else the bbox of 'H', else 0.7
    pub fn x_height(&self) -> f32;     // over em; from OS/2, else bbox of 'x', else 0.5
    pub fn shape(&self, text: &str) -> Shaped;
    pub fn measure(&self) -> Features;
}
pub struct ShapedGlyph { pub ch: char, pub path: Option<tiny_skia::Path>, /* em units, y up, origin at the glyph origin */ pub advance: f32 /* em */, pub x_offset: f32 /* em */ }
pub struct Shaped { pub glyphs: Vec<ShapedGlyph>, pub cap_height: f32, pub x_height: f32 }
impl Shaped { pub fn advance(&self) -> f32; /* em, sum of advances */ pub fn bbox_height(&self) -> f32; /* em, of the outlines */ }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Features { pub cap: f32, pub x: f32, pub stem: f32, /* stem thickness over cap height, from I and H */ pub widths: [u8; 62] /* bbox width over cap height ×100, for A–Z a–z 0–9, 0 when the glyph is missing */ }
pub fn width_index(ch: char) -> Option<usize>; // 'A'..='Z' → 0..26, 'a'..='z' → 26..52, '0'..='9' → 52..62
```

- [ ] **Step 1: Add the dependencies**

In `apps/desktop/src-tauri/Cargo.toml` `[dependencies]`, after `image = …`:

```toml
# Text re-setting (src/text/): shaping and outlines (rustybuzz re-exports the ttf-parser it is built on), the
# rasteriser the fit scores with (resvg's own), the installed fonts' catalogue.
rustybuzz = "0.20"
tiny-skia = "0.11"
fontdb = "0.23"
```

and under `[target.'cfg(target_os = "macos")'.dependencies]`:

```toml
# Vision's text recogniser (text/vision.rs): on-device OCR
objc2-vision = { version = "0.3", features = ["VNRecognizeTextRequest", "VNRequestHandler", "VNObservation", "VNRequest"] }
objc2-foundation = { version = "0.3", features = ["NSArray", "NSData", "NSDictionary", "NSError", "NSString", "NSObject"] }
objc2-core-foundation = { version = "0.3", features = ["CFCGTypes"] }
```

Run: `cd apps/desktop/src-tauri && cargo check 2>&1 | tail -3` — if `cargo check` names a missing feature (objc2 crates gate every type), add that feature and check again. (The `tiny-skia` version must match the one resvg 0.48.1 pins in `Cargo.lock` — `0.11.4`; `cargo tree -p studi0trace-desktop -i tiny-skia` shows one version.)

- [ ] **Step 2: Write the failing test for shaping and measuring**

In `src/text/glyphs.rs`, at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text/fonts").join(name)
    }

    #[test]
    fn shapes_a_string_into_glyphs_with_advances_and_outlines() {
        let font = Font::load(&fixture("Montserrat-Regular.ttf"), 0).unwrap();
        let shaped = font.shape("EST. 2026");
        assert_eq!(shaped.glyphs.len(), 9);
        assert_eq!(shaped.glyphs[0].ch, 'E');
        assert!(shaped.glyphs[0].path.is_some());
        assert!(shaped.glyphs[4].path.is_none(), "a space has no outline");
        assert!(shaped.glyphs[4].advance > 0.1 && shaped.glyphs[4].advance < 0.5);
        assert!((shaped.advance() - shaped.glyphs.iter().map(|g| g.advance).sum::<f32>()).abs() < 1e-6);
        assert!((0.6..0.8).contains(&shaped.cap_height), "{}", shaped.cap_height);
        // the E's outline spans the cap height: y up, from 0 to cap
        let b = shaped.glyphs[0].path.as_ref().unwrap().bounds();
        assert!(b.top().abs() < 0.02 && (b.bottom() - shaped.cap_height).abs() < 0.03 || b.bottom().abs() < 0.02 && (b.top().abs() - shaped.cap_height).abs() < 0.03, "{b:?}");
    }

    #[test]
    fn measures_features_in_the_expected_ranges() {
        let light = Font::load(&fixture("JosefinSans-Light.ttf"), 0).unwrap().measure();
        let regular = Font::load(&fixture("Montserrat-Regular.ttf"), 0).unwrap().measure();
        assert!(light.stem < regular.stem, "a light face has thinner stems: {} vs {}", light.stem, regular.stem);
        assert!((0.05..0.2).contains(&regular.stem), "{}", regular.stem);
        assert!(regular.widths[width_index('W').unwrap()] > regular.widths[width_index('I').unwrap()]);
        assert_eq!(width_index('a'), Some(26));
        assert_eq!(width_index('9'), Some(61));
        assert_eq!(width_index('.'), None);
    }

    #[test]
    fn a_file_that_is_not_a_font_is_refused() {
        let err = Font::from_bytes(vec![0u8; 100], 0).unwrap_err();
        assert_eq!(err.code(), Some("bad_font"));
    }
}
```

- [ ] **Step 3: Run it to see it fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --lib text::glyphs 2>&1 | tail -5`
Expected: compile error, `text` module missing.

- [ ] **Step 4: Write `text/mod.rs` (types only for now) and `text/glyphs.rs`**

`src/text/mod.rs`:

```rust
//! Text re-setting: the lines of text in an image are found and read (vision.rs, reader_openai.rs), the font is
//! matched (catalog.rs, fonts.rs) and fitted (glyphs.rs, ink.rs, fit.rs), the text is painted out of what the
//! trace sees (ink.rs) and the glyph outlines are composed on top (compose.rs). Nothing here is seen by the
//! engine, the core or the worker.
pub mod glyphs;

use crate::error::CommandError;
use serde::{Deserialize, Serialize};

/// A line whose best fit scores under this is traced as shapes (set from the fixture set, tests/text_fit.rs).
pub const FIT_ACCEPT: f32 = 0.82;
/// 90th-percentile ΔE of the covered pixels from the ink colour above which the ink is a gradient.
pub const INK_SPREAD_MAX: f32 = 8.0;
/// The share by which the ink's segment count may differ from the string's non-space characters.
pub const MISMATCH_SHARE: f32 = 0.34;
/// A quad turned more than this is declined.
pub const ROTATED_DEG: f32 = 5.0;
/// Families fitted per line beyond the reader's hints.
pub const SHORTLIST: usize = 12;
/// How far one glyph may be nudged along the line, in px.
pub const NUDGE_MAX: f32 = 2.0;
/// Vision observations under this confidence are dropped.
pub const VISION_MIN_CONFIDENCE: f32 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Google,
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontRef {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Type,
    Script,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Script,
    Custom,
    Gradient,
    Mismatch,
    NoFit,
    Rotated,
    Fonts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "reason")]
pub enum Status {
    Matched,
    Declined(Reason),
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    pub size: f32,
    pub baseline: f32,
    pub x: f32,
    pub tracking: f32,
    pub nudges: Vec<f32>,
    pub iou: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub font: FontRef,
    pub iou: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLine {
    pub id: String,
    pub quad: [[f32; 2]; 4],
    pub string: String,
    pub class: Class,
    pub colour: [u8; 3],
    pub status: Status,
    pub candidates: Vec<Candidate>,
    pub chosen: Option<usize>,
    pub enabled: bool,
    pub fit: Option<Fit>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReaderKind {
    Vision,
    Ai,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextPlan {
    pub image: String,
    pub reader: ReaderKind,
    pub lines: Vec<TextLine>,
    pub ai_error: Option<String>,
    pub fonts_offline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Finding,
    Reading,
    Matching,
    Done,
    Failed,
}

/// A text failure by its code: the app's own, else redraw's table.
pub fn error(code: &'static str) -> CommandError {
    match code {
        "text_unavailable" => CommandError::new(501, code, "Text recognition needs macOS 10.15 or later."),
        "bad_font" => CommandError::new(422, code, "The font file could not be read."),
        _ => crate::redraw::error(code),
    }
}
```

`src/text/glyphs.rs`:

```rust
//! A font as the fit sees it: shaped glyphs with outlines in em units (y up), and the features the shortlist
//! ranks by. Shaping runs with kerning off: the fit's per-glyph nudges absorb the pairs, and the fixture set is
//! drawn glyph by glyph the same way.
use crate::error::CommandError;
use rustybuzz::ttf_parser::{self, GlyphId, OutlineBuilder};
use rustybuzz::{Feature, UnicodeBuffer};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tiny_skia::{Path as SkPath, PathBuilder};

pub struct Font {
    data: Vec<u8>,
    index: u32,
}

pub struct ShapedGlyph {
    pub ch: char,
    /// Em units, y up, origin at the glyph's origin; None for a glyph with no outline (a space).
    pub path: Option<SkPath>,
    /// Em units.
    pub advance: f32,
    pub x_offset: f32,
}

pub struct Shaped {
    pub glyphs: Vec<ShapedGlyph>,
    pub cap_height: f32,
    pub x_height: f32,
}

impl Shaped {
    pub fn advance(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// The outlines' extent in y, em units.
    pub fn bbox_height(&self) -> f32 {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for g in self.glyphs.iter().filter_map(|g| g.path.as_ref()) {
            let b = g.bounds();
            lo = lo.min(b.top());
            hi = hi.max(b.bottom());
        }
        if lo > hi { 0.0 } else { hi - lo }
    }
}

/// What the shortlist ranks a face by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Features {
    /// Cap height over em.
    pub cap: f32,
    /// x-height over em.
    pub x: f32,
    /// Stem thickness over cap height, the mean of the I's width and the H's left stem.
    pub stem: f32,
    /// Glyph bbox width over cap height, in hundredths, for A–Z a–z 0–9; 0 for a glyph the face lacks.
    pub widths: [u8; 62],
}

pub fn width_index(ch: char) -> Option<usize> {
    match ch {
        'A'..='Z' => Some(ch as usize - 'A' as usize),
        'a'..='z' => Some(26 + ch as usize - 'a' as usize),
        '0'..='9' => Some(52 + ch as usize - '0' as usize),
        _ => None,
    }
}

struct Builder {
    pb: PathBuilder,
    scale: f32,
}

impl OutlineBuilder for Builder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(x * self.scale, y * self.scale);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(x * self.scale, y * self.scale);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(x1 * self.scale, y1 * self.scale, x * self.scale, y * self.scale);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(x1 * self.scale, y1 * self.scale, x2 * self.scale, y2 * self.scale, x * self.scale, y * self.scale);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

impl Font {
    pub fn load(path: &Path, index: u32) -> Result<Font, CommandError> {
        let data = std::fs::read(path).map_err(|e| CommandError::new(500, "io_error", format!("{}: {e}", path.display())))?;
        Font::from_bytes(data, index)
    }

    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<Font, CommandError> {
        if ttf_parser::Face::parse(&data, index).is_err() {
            return Err(super::error("bad_font"));
        }
        Ok(Font { data, index })
    }

    pub fn face(&self) -> rustybuzz::Face<'_> {
        rustybuzz::Face::from_slice(&self.data, self.index).expect("checked in from_bytes")
    }

    pub fn units_per_em(&self) -> f32 {
        self.face().units_per_em() as f32
    }

    fn glyph_height(&self, ch: char) -> Option<f32> {
        let face = self.face();
        let id = face.glyph_index(ch)?;
        let b = face.glyph_bounding_box(id)?;
        Some((b.y_max - b.y_min) as f32 / self.units_per_em())
    }

    pub fn cap_height(&self) -> f32 {
        self.face().capital_height().map(|h| h as f32 / self.units_per_em()).filter(|h| *h > 0.0).or_else(|| self.glyph_height('H')).unwrap_or(0.7)
    }

    pub fn x_height(&self) -> f32 {
        self.face().x_height().map(|h| h as f32 / self.units_per_em()).filter(|h| *h > 0.0).or_else(|| self.glyph_height('x')).unwrap_or(0.5)
    }

    fn outline(&self, face: &rustybuzz::Face<'_>, id: GlyphId) -> Option<SkPath> {
        let mut b = Builder { pb: PathBuilder::new(), scale: 1.0 / self.units_per_em() };
        face.outline_glyph(id, &mut b)?;
        b.pb.finish()
    }

    /// The string shaped with kerning off, one glyph per cluster; a character the face lacks is shaped as
    /// .notdef and keeps its advance.
    pub fn shape(&self, text: &str) -> Shaped {
        let face = self.face();
        let upem = self.units_per_em();
        let mut buf = UnicodeBuffer::new();
        buf.push_str(text);
        let kern_off = Feature::new(rustybuzz::ttf_parser::Tag::from_bytes(b"kern"), 0, ..);
        let out = rustybuzz::shape(&face, &[kern_off], buf);
        let chars: Vec<char> = text.chars().collect();
        let mut glyphs = Vec::with_capacity(out.len());
        for (info, pos) in out.glyph_infos().iter().zip(out.glyph_positions()) {
            let ch = chars.get(info.cluster as usize).copied().unwrap_or('\u{FFFD}');
            let path = self.outline(&face, GlyphId(info.glyph_id as u16));
            glyphs.push(ShapedGlyph { ch, path, advance: pos.x_advance as f32 / upem, x_offset: pos.x_offset as f32 / upem });
        }
        Shaped { glyphs, cap_height: self.cap_height(), x_height: self.x_height() }
    }

    /// Stem thickness: the I's bbox width over the cap height (tools/gfonts_index.py measures the same).
    pub fn measure(&self) -> Features {
        let cap = self.cap_height();
        let x = self.x_height();
        let face = self.face();
        let upem = self.units_per_em();
        let mut widths = [0u8; 62];
        for ch in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            if let Some(b) = face.glyph_index(ch).and_then(|id| face.glyph_bounding_box(id)) {
                let w = (b.x_max - b.x_min) as f32 / upem / cap * 100.0;
                widths[width_index(ch).unwrap()] = w.round().clamp(0.0, 255.0) as u8;
            }
        }
        let i_width = face.glyph_index('I').and_then(|id| face.glyph_bounding_box(id)).map(|b| (b.x_max - b.x_min) as f32 / upem).unwrap_or(0.1 * cap);
        Features { cap, x, stem: i_width / cap, widths }
    }
}
```

(`Feature::new(tag, value, range)` — rustybuzz 0.20's signature is `Feature::new(tag: Tag, value: u32, range: impl RangeBounds<u32>)`; if the compiler disagrees, check `cargo doc -p rustybuzz --open` and adjust.)

Add `pub mod text;` to `src/lib.rs` after `pub mod store;`.

- [ ] **Step 5: Run the tests**

Run: `cd apps/desktop/src-tauri && cargo test --release --lib text::glyphs 2>&1 | tail -8`
Expected: 3 passed. If `bounds()` of the E reports y down (negative top), the outline builder's y sign is as ttf-parser gives it (y up); the test accepts either orientation of the span — the `fit.rs` transform in Task 4 flips y once.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/Cargo.toml Cargo.lock apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/text
git commit -m "text: the plan's types, and glyphs.rs shapes a string into em-unit outlines and measures a face's features"
```

---

### Task 3: `ink.rs` — the ink of a line, its segments, the decline checks, painting out

**Files:**
- Create: `apps/desktop/src-tauri/src/text/ink.rs`
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`pub mod ink;`)

**Interfaces:**
- Consumes: `redraw::Source { rgba, width, height }`; constants from `text/mod.rs`.
- Produces:

```rust
/// One line's ink inside its box.
pub struct LineInk {
    pub left: u32, pub top: u32, pub width: u32, pub height: u32,  // the box in source pixels (the quad's bbox grown by 20 % of its height, clipped)
    pub coverage: Vec<f32>,   // width*height, 0..1
    pub mask: Vec<bool>,      // coverage >= 0.5
    pub backdrop: [u8; 3],
    pub colour: [u8; 3],
    pub spread: f32,          // 90th-percentile ΔE of covered pixels from `colour`
    pub segments: Vec<(u32, u32)>, // x ranges (left, right exclusive) of the merged components, sorted
    pub ink_bounds: Option<(u32, u32, u32, u32)>, // (left, top, right, bottom) of the mask within the box, exclusive right/bottom
}
pub fn line_ink(source: &Source, quad: [[f32; 2]; 4]) -> LineInk;
pub fn quad_angle_deg(quad: [[f32; 2]; 4]) -> f32;  // of the top edge
pub fn decline(ink: &LineInk, string: &str, class: Class, quad: [[f32; 2]; 4]) -> Option<Reason>;
/// Paints the lines' ink out of a copy of the source: the mask dilated 1 px, each masked pixel the mean of the row's and the column's linear interpolations between the nearest unmasked neighbours.
pub fn paint_out(source: &Source, inks: &[&LineInk]) -> Vec<u8>; // RGBA, source.width × source.height
pub fn delta_e(a: [u8; 3], b: [u8; 3]) -> f32; // CIE76 in Lab from sRGB — enough for a spread test
```

- [ ] **Step 1: Write the failing tests**

At the bottom of `src/text/ink.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::redraw::Source;

    fn solid(w: u32, h: u32, colour: [u8; 3]) -> Source {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
        Source { rgba, width: w, height: h }
    }

    fn paint(src: &mut Source, x0: u32, y0: u32, w: u32, h: u32, colour: [u8; 3]) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let i = ((y * src.width + x) * 4) as usize;
                src.rgba[i..i + 3].copy_from_slice(&colour);
            }
        }
    }

    /// Three "letters": two bars and an i (a bar with a dot over it), black on white.
    fn three_letters() -> (Source, [[f32; 2]; 4]) {
        let mut src = solid(120, 60, [255, 255, 255]);
        paint(&mut src, 20, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 40, 20, 6, 24, [0, 0, 0]);
        paint(&mut src, 60, 28, 6, 16, [0, 0, 0]);
        paint(&mut src, 60, 20, 6, 4, [0, 0, 0]);
        (src, [[15.0, 18.0], [75.0, 18.0], [75.0, 46.0], [15.0, 46.0]])
    }

    #[test]
    fn reads_backdrop_ink_and_three_segments() {
        let (src, quad) = three_letters();
        let ink = line_ink(&src, quad);
        assert_eq!(ink.backdrop, [255, 255, 255]);
        assert_eq!(ink.colour, [0, 0, 0]);
        assert!(ink.spread < 1.0);
        assert_eq!(ink.segments.len(), 3, "{:?}", ink.segments);
        assert_eq!(ink.segments[0], (20 - ink.left, 26 - ink.left));
        assert_eq!(ink.ink_bounds, Some((20 - ink.left, 20 - ink.top, 66 - ink.left, 44 - ink.top)));
        let at = |x: u32, y: u32| ink.coverage[((y - ink.top) * ink.width + (x - ink.left)) as usize];
        assert!((at(22, 30) - 1.0).abs() < 0.01);
        assert!(at(30, 30) < 0.01);
    }

    #[test]
    fn declines_scripts_gradients_mismatches_and_rotation() {
        let (src, quad) = three_letters();
        let ink = line_ink(&src, quad);
        assert_eq!(decline(&ink, "lli", Class::Type, quad), None);
        assert_eq!(decline(&ink, "lli", Class::Script, quad), Some(Reason::Script));
        assert_eq!(decline(&ink, "hello world", Class::Type, quad), Some(Reason::Mismatch));
        let turned = [[15.0, 18.0], [75.0, 26.0], [75.0, 54.0], [15.0, 46.0]];
        assert_eq!(decline(&ink, "lli", Class::Type, turned), Some(Reason::Rotated));
        // a gradient ink: the bars go from black to mid grey across the line
        let mut g = src.clone();
        paint(&mut g, 40, 20, 6, 24, [90, 90, 90]);
        paint(&mut g, 60, 20, 6, 24, [160, 160, 160]);
        let ink = line_ink(&g, quad);
        assert_eq!(decline(&ink, "lli", Class::Type, quad), Some(Reason::Gradient));
    }

    #[test]
    fn paints_the_ink_out_flat_on_a_flat_backdrop_and_along_a_gradient() {
        let (src, quad) = three_letters();
        let ink = line_ink(&src, quad);
        let out = paint_out(&src, &[&ink]);
        assert!(out.chunks(4).all(|p| p[0] == 255 && p[3] == 255), "a flat backdrop stays flat");
        // a horizontal gradient backdrop: painted-out pixels sit within 2 levels of the gradient
        let mut g = solid(120, 60, [255, 255, 255]);
        for y in 0..60 {
            for x in 0..120 {
                paint(&mut g, x, y, 1, 1, [(255 - x) as u8, 240, 230]);
            }
        }
        paint(&mut g, 40, 20, 6, 24, [0, 0, 0]);
        let ink = line_ink(&g, quad);
        let out = paint_out(&g, &[&ink]);
        for y in 20..44u32 {
            for x in 40..46u32 {
                let r = out[((y * 120 + x) * 4) as usize];
                assert!((r as i32 - (255 - x as i32)).abs() <= 2, "({x},{y}) r={r}");
            }
        }
    }

    #[test]
    fn delta_e_is_zero_for_equal_colours_and_large_between_black_and_white() {
        assert_eq!(delta_e([10, 20, 30], [10, 20, 30]), 0.0);
        assert!(delta_e([0, 0, 0], [255, 255, 255]) > 90.0);
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --lib text::ink 2>&1 | tail -3`
Expected: compile error, `ink` missing.

- [ ] **Step 3: Implement `ink.rs`**

```rust
//! What a line of text is made of, read from the pixels: its backdrop (the box's border ring), its ink colour
//! (the pixels furthest from the backdrop), per-pixel coverage (where a pixel sits between the two in linear RGB),
//! the mask, the per-character segments, and the reasons a line is left to the trace. `paint_out` is the
//! inverse: the ink gone, the backdrop continued through it.
use super::{Class, Reason, INK_SPREAD_MAX, MISMATCH_SHARE, ROTATED_DEG};
use crate::redraw::Source;

pub struct LineInk {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<f32>,
    pub mask: Vec<bool>,
    pub backdrop: [u8; 3],
    pub colour: [u8; 3],
    pub spread: f32,
    pub segments: Vec<(u32, u32)>,
    pub ink_bounds: Option<(u32, u32, u32, u32)>,
}

fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn lab(c: [u8; 3]) -> [f32; 3] {
    let [r, g, b] = [srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2])];
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

pub fn delta_e(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (p, q) = (lab(a), lab(b));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

fn median(v: &mut [u8]) -> u8 {
    if v.is_empty() {
        return 0;
    }
    v.sort_unstable();
    v[v.len() / 2]
}

fn median_colour(pixels: &[[u8; 3]]) -> [u8; 3] {
    let mut r: Vec<u8> = pixels.iter().map(|p| p[0]).collect();
    let mut g: Vec<u8> = pixels.iter().map(|p| p[1]).collect();
    let mut b: Vec<u8> = pixels.iter().map(|p| p[2]).collect();
    [median(&mut r), median(&mut g), median(&mut b)]
}

fn quad_bbox(quad: [[f32; 2]; 4]) -> (f32, f32, f32, f32) {
    let xs = quad.iter().map(|p| p[0]);
    let ys = quad.iter().map(|p| p[1]);
    (xs.clone().fold(f32::MAX, f32::min), ys.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max), ys.fold(f32::MIN, f32::max))
}

pub fn quad_angle_deg(quad: [[f32; 2]; 4]) -> f32 {
    let (dx, dy) = (quad[1][0] - quad[0][0], quad[1][1] - quad[0][1]);
    dy.atan2(dx).to_degrees()
}

pub fn line_ink(source: &Source, quad: [[f32; 2]; 4]) -> LineInk {
    let (x0, y0, x1, y1) = quad_bbox(quad);
    let grow = (y1 - y0) * 0.2;
    let left = (x0 - grow).floor().max(0.0) as u32;
    let top = (y0 - grow).floor().max(0.0) as u32;
    let right = (x1 + grow).ceil().min(source.width as f32) as u32;
    let bottom = (y1 + grow).ceil().min(source.height as f32) as u32;
    let (width, height) = (right.saturating_sub(left).max(1), bottom.saturating_sub(top).max(1));
    let px = |x: u32, y: u32| -> [u8; 3] {
        let i = ((y * source.width + x) * 4) as usize;
        [source.rgba[i], source.rgba[i + 1], source.rgba[i + 2]]
    };
    // the backdrop: the 2 px ring
    let mut ring = Vec::new();
    for y in top..top + height {
        for x in left..left + width {
            let edge = x < left + 2 || x >= left + width - 2 || y < top + 2 || y >= top + height - 2;
            if edge {
                ring.push(px(x, y));
            }
        }
    }
    let backdrop = median_colour(&ring);
    // the ink: the pixels at least 90 % as far from the backdrop as the furthest
    let mut dist = vec![0f32; (width * height) as usize];
    let mut far = 0f32;
    for y in 0..height {
        for x in 0..width {
            let d = delta_e(px(left + x, top + y), backdrop);
            dist[(y * width + x) as usize] = d;
            far = far.max(d);
        }
    }
    let inkers: Vec<[u8; 3]> = (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).filter(|&(x, y)| far > 0.0 && dist[(y * width + x) as usize] >= 0.9 * far).map(|(x, y)| px(left + x, top + y)).collect();
    let colour = if inkers.is_empty() { backdrop } else { median_colour(&inkers) };
    // coverage: position between backdrop and ink along their line, in linear RGB
    let b = [srgb_to_linear(backdrop[0]), srgb_to_linear(backdrop[1]), srgb_to_linear(backdrop[2])];
    let k = [srgb_to_linear(colour[0]), srgb_to_linear(colour[1]), srgb_to_linear(colour[2])];
    let axis = [k[0] - b[0], k[1] - b[1], k[2] - b[2]];
    let len2 = axis.iter().map(|a| a * a).sum::<f32>().max(1e-6);
    let mut coverage = vec![0f32; (width * height) as usize];
    for y in 0..height {
        for x in 0..width {
            let p = px(left + x, top + y);
            let l = [srgb_to_linear(p[0]) - b[0], srgb_to_linear(p[1]) - b[1], srgb_to_linear(p[2]) - b[2]];
            let t = (l[0] * axis[0] + l[1] * axis[1] + l[2] * axis[2]) / len2;
            coverage[(y * width + x) as usize] = t.clamp(0.0, 1.0);
        }
    }
    let mask: Vec<bool> = coverage.iter().map(|&c| c >= 0.5).collect();
    // the spread: 90th percentile ΔE of covered pixels from the ink colour
    let mut spreads: Vec<f32> = (0..height).flat_map(|y| (0..width).map(move |x| (x, y))).filter(|&(x, y)| mask[(y * width + x) as usize]).map(|(x, y)| delta_e(px(left + x, top + y), colour)).collect();
    spreads.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let spread = spreads.get(spreads.len() * 9 / 10).copied().unwrap_or(0.0);
    let segments = segments_of(&mask, width, height);
    let ink_bounds = bounds_of(&mask, width, height);
    LineInk { left, top, width, height, coverage, mask, backdrop, colour, spread, segments, ink_bounds }
}

fn bounds_of(mask: &[bool], width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let (mut l, mut t, mut r, mut b) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..height {
        for x in 0..width {
            if mask[(y * width + x) as usize] {
                l = l.min(x);
                t = t.min(y);
                r = r.max(x + 1);
                b = b.max(y + 1);
            }
        }
    }
    (l < r).then_some((l, t, r, b))
}

/// Connected components (8-connected) of the mask as x ranges, merged where the ranges overlap.
fn segments_of(mask: &[bool], width: u32, height: u32) -> Vec<(u32, u32)> {
    let mut label = vec![0u32; mask.len()];
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || label[start] != 0 {
            continue;
        }
        let id = ranges.len() as u32 + 1;
        let (mut lo, mut hi) = (u32::MAX, 0u32);
        stack.push(start);
        label[start] = id;
        while let Some(i) = stack.pop() {
            let (x, y) = ((i as u32) % width, (i as u32) / width);
            lo = lo.min(x);
            hi = hi.max(x + 1);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }
                    let j = (ny as u32 * width + nx as u32) as usize;
                    if mask[j] && label[j] == 0 {
                        label[j] = id;
                        stack.push(j);
                    }
                }
            }
        }
        ranges.push((lo, hi));
    }
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for r in ranges {
        match merged.last_mut() {
            Some(last) if r.0 < last.1 => last.1 = last.1.max(r.1),
            _ => merged.push(r),
        }
    }
    merged
}

pub fn decline(ink: &LineInk, string: &str, class: Class, quad: [[f32; 2]; 4]) -> Option<Reason> {
    match class {
        Class::Script => return Some(Reason::Script),
        Class::Custom => return Some(Reason::Custom),
        Class::Type => {}
    }
    if quad_angle_deg(quad).abs() > ROTATED_DEG {
        return Some(Reason::Rotated);
    }
    if ink.spread > INK_SPREAD_MAX {
        return Some(Reason::Gradient);
    }
    let chars = string.chars().filter(|c| !c.is_whitespace()).count().max(1) as f32;
    let diff = (ink.segments.len() as f32 - chars).abs() / chars;
    if diff > MISMATCH_SHARE {
        return Some(Reason::Mismatch);
    }
    None
}

pub fn paint_out(source: &Source, inks: &[&LineInk]) -> Vec<u8> {
    let (w, h) = (source.width as usize, source.height as usize);
    let mut masked = vec![false; w * h];
    for ink in inks {
        for y in 0..ink.height as usize {
            for x in 0..ink.width as usize {
                if ink.mask[y * ink.width as usize + x] {
                    // dilated by 1 px
                    for dy in -1i64..=1 {
                        for dx in -1i64..=1 {
                            let (sx, sy) = (ink.left as i64 + x as i64 + dx, ink.top as i64 + y as i64 + dy);
                            if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                                masked[sy as usize * w + sx as usize] = true;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut out = source.rgba.clone();
    let at = |x: usize, y: usize, c: usize| source.rgba[(y * w + x) * 4 + c] as f32;
    // nearest unmasked neighbour along a row (left/right) and a column (up/down)
    let find = |x: usize, y: usize, dx: i64, dy: i64| -> Option<(usize, usize)> {
        let (mut cx, mut cy) = (x as i64, y as i64);
        loop {
            cx += dx;
            cy += dy;
            if cx < 0 || cy < 0 || cx as usize >= w || cy as usize >= h {
                return None;
            }
            if !masked[cy as usize * w + cx as usize] {
                return Some((cx as usize, cy as usize));
            }
        }
    };
    for y in 0..h {
        for x in 0..w {
            if !masked[y * w + x] {
                continue;
            }
            let (l, r, u, d) = (find(x, y, -1, 0), find(x, y, 1, 0), find(x, y, 0, -1), find(x, y, 0, 1));
            let lerp = |a: Option<(usize, usize)>, b: Option<(usize, usize)>, c: usize| -> Option<f32> {
                match (a, b) {
                    (Some(p), Some(q)) => {
                        let da = ((x as f32 - p.0 as f32).powi(2) + (y as f32 - p.1 as f32).powi(2)).sqrt();
                        let db = ((x as f32 - q.0 as f32).powi(2) + (y as f32 - q.1 as f32).powi(2)).sqrt();
                        Some((at(p.0, p.1, c) * db + at(q.0, q.1, c) * da) / (da + db).max(1e-6))
                    }
                    (Some(p), None) | (None, Some(p)) => Some(at(p.0, p.1, c)),
                    (None, None) => None,
                }
            };
            for c in 0..4 {
                let row = lerp(l, r, c);
                let col = lerp(u, d, c);
                let v = match (row, col) {
                    (Some(a), Some(b)) => (a + b) / 2.0,
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => at(x, y, c),
                };
                out[(y * w + x) * 4 + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}
```

Add `pub mod ink;` to `text/mod.rs`.

- [ ] **Step 4: Run the tests**

Run: `cd apps/desktop/src-tauri && cargo test --release --lib text::ink 2>&1 | tail -8`
Expected: 4 passed. (The gradient-ink case: the bars at 0/90/160 grey have a 90th-percentile ΔE from the median ink well over 8; the flat case under 1.)

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/text
git commit -m "text: ink.rs reads a line's backdrop, ink colour, coverage and segments, declines what cannot be re-set, and paints the ink out"
```

---

### Task 4: `fit.rs` — fitting a shaped string onto the ink, and `tests/text_fit.rs`

**Files:**
- Create: `apps/desktop/src-tauri/src/text/fit.rs`, `apps/desktop/src-tauri/tests/text_fit.rs`
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`pub mod fit;`, `FIT_ACCEPT`)

**Interfaces:**
- Consumes: `glyphs::{Font, Shaped}`, `ink::LineInk`, `Fit`, `NUDGE_MAX`.
- Produces:

```rust
/// Renders the shaped string at `fit` into a coverage buffer the size of the ink box (ink.width × ink.height), 0..1.
pub fn render_coverage(shaped: &Shaped, fit: &Fit, ink: &LineInk) -> Vec<f32>;
/// Soft IoU: Σ min / Σ max over the two buffers.
pub fn soft_iou(a: &[f32], b: &[f32]) -> f32;
/// The initial guess from the ink bounds, then coordinate descent on (size, baseline, x, tracking), then per-glyph nudges. The fit's x, baseline are in source pixels (ink.left/top added); `iou` is the final score. None when the ink is empty.
pub fn fit_line(shaped: &Shaped, ink: &LineInk, all_caps: bool) -> Option<Fit>;
/// The glyph outlines at `fit`, as an SVG path `d` in source pixels (y down), every number to 2 decimals.
pub fn path_data(shaped: &Shaped, fit: &Fit) -> String;
```

- [ ] **Step 1: Write the failing integration test**

`apps/desktop/tests/text_fit.rs` → `apps/desktop/src-tauri/tests/text_fit.rs`:

```rust
//! The fit on the fixture set: with the true font it recovers size, baseline and tracking and scores a high IoU;
//! every wrong font scores lower; FIT_ACCEPT sits between the two populations.
use serde::Deserialize;
use std::path::PathBuf;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::fit::fit_line;
use studi0trace_desktop::text::glyphs::Font;
use studi0trace_desktop::text::ink::line_ink;
use studi0trace_desktop::text::FIT_ACCEPT;

#[derive(Deserialize)]
struct Item {
    file: String,
    font: String,
    string: String,
    cap_px: f32,
    size_px: f32,
    baseline: f32,
    x: f32,
    tracking_px: f32,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text")
}

fn load(item: &Item) -> (Source, [[f32; 2]; 4]) {
    let bytes = std::fs::read(dir().join(&item.file)).unwrap();
    let source = Source::decode(&bytes).unwrap();
    // the line's box, as Vision would give it: the text's extent with a little slack
    let (w, h) = (source.width as f32, source.height as f32);
    let quad = [[20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, (item.baseline + 8.0).min(h)], [20.0, (item.baseline + 8.0).min(h)]];
    (source, quad)
}

#[test]
fn the_true_font_recovers_the_truth_and_beats_every_wrong_font() {
    let items: Vec<Item> = serde_json::from_str(&std::fs::read_to_string(dir().join("truth.json")).unwrap()).unwrap();
    let fonts = ["JosefinSans-Light.ttf", "Montserrat-Regular.ttf", "PlayfairDisplay-Regular.ttf"].map(|f| (f, Font::load(&dir().join("fonts").join(f), 0).unwrap()));
    let (mut true_min, mut wrong_max) = (f32::MAX, f32::MIN);
    let mut failures = Vec::new();
    for item in &items {
        let (source, quad) = load(item);
        let ink = line_ink(&source, quad);
        let all_caps = item.string.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
        let mut best_wrong = f32::MIN;
        let mut true_fit = None;
        for (name, font) in &fonts {
            let shaped = font.shape(&item.string);
            let fit = fit_line(&shaped, &ink, all_caps).expect("ink present");
            if *name == item.font {
                true_fit = Some(fit);
            } else {
                best_wrong = best_wrong.max(fit.iou);
            }
        }
        let fit = true_fit.unwrap();
        true_min = true_min.min(fit.iou);
        wrong_max = wrong_max.max(best_wrong);
        let size_err = (fit.size - item.size_px).abs() / item.size_px;
        let checks = [
            (size_err <= 0.02, format!("size {} vs {}", fit.size, item.size_px)),
            ((fit.baseline - item.baseline).abs() <= 0.3, format!("baseline {} vs {}", fit.baseline, item.baseline)),
            ((fit.tracking - item.tracking_px).abs() <= 0.2, format!("tracking {} vs {}", fit.tracking, item.tracking_px)),
            ((fit.x - item.x).abs() <= 0.5, format!("x {} vs {}", fit.x, item.x)),
            (fit.iou >= 0.90, format!("iou {}", fit.iou)),
            (fit.iou > best_wrong, format!("a wrong font scored {best_wrong} against {}", fit.iou)),
        ];
        for (ok, why) in checks {
            if !ok {
                failures.push(format!("{}: {why}", item.file));
            }
        }
    }
    eprintln!("true-font IoU min {true_min:.3}; wrong-font IoU max {wrong_max:.3}; FIT_ACCEPT {FIT_ACCEPT}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(FIT_ACCEPT <= true_min - 0.04 && FIT_ACCEPT >= wrong_max, "FIT_ACCEPT must sit between the populations");
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_fit 2>&1 | tail -3`
Expected: compile error, `fit` missing.

- [ ] **Step 3: Implement `fit.rs`**

```rust
//! Fitting a shaped string onto a line's ink: size, baseline, start and tracking by coordinate descent on the
//! soft IoU of rendered coverage against the ink's, then per-glyph nudges along the line. The renderer is
//! tiny-skia, anti-aliased, at 1×: the same rasteriser the engine's refine stage uses.
use super::glyphs::Shaped;
use super::ink::LineInk;
use super::{Fit, NUDGE_MAX};
use tiny_skia::{FillRule, Paint, Pixmap, Transform};

/// Glyph origins along the line for a fit: x of each glyph's origin in source pixels.
fn origins(shaped: &Shaped, fit: &Fit) -> Vec<f32> {
    let mut x = fit.x;
    let mut out = Vec::with_capacity(shaped.glyphs.len());
    for (i, g) in shaped.glyphs.iter().enumerate() {
        let nudge = fit.nudges.get(i).copied().unwrap_or(0.0);
        out.push(x + g.x_offset * fit.size + nudge);
        x += g.advance * fit.size + fit.tracking;
    }
    out
}

pub fn render_coverage(shaped: &Shaped, fit: &Fit, ink: &LineInk) -> Vec<f32> {
    let Some(mut pm) = Pixmap::new(ink.width, ink.height) else { return vec![0.0; (ink.width * ink.height) as usize] };
    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    for (g, ox) in shaped.glyphs.iter().zip(origins(shaped, fit)) {
        let Some(path) = &g.path else { continue };
        // em units y up → source pixels y down, into the ink box's frame
        let t = Transform::from_row(fit.size, 0.0, 0.0, -fit.size, ox - ink.left as f32, fit.baseline - ink.top as f32);
        pm.fill_path(path, &paint, FillRule::NonZero, t, None);
    }
    pm.data().chunks(4).map(|p| p[3] as f32 / 255.0).collect()
}

pub fn soft_iou(a: &[f32], b: &[f32]) -> f32 {
    let (mut inter, mut union) = (0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        inter += x.min(*y);
        union += x.max(*y);
    }
    if union <= 0.0 { 0.0 } else { inter / union }
}

fn score(shaped: &Shaped, fit: &Fit, ink: &LineInk) -> f32 {
    soft_iou(&render_coverage(shaped, fit, ink), &ink.coverage)
}

pub fn fit_line(shaped: &Shaped, ink: &LineInk, all_caps: bool) -> Option<Fit> {
    let (l, t, r, b) = ink.ink_bounds?;
    let ink_h = (b - t) as f32;
    let ink_w = (r - l) as f32;
    let em_h = if all_caps { shaped.cap_height } else { shaped.bbox_height() }.max(0.1);
    let size = ink_h / em_h;
    let n = shaped.glyphs.len().max(1) as f32;
    let first_lsb = shaped.glyphs.first().and_then(|g| g.path.as_ref()).map(|p| p.bounds().left()).unwrap_or(0.0);
    let last_rsb = shaped.glyphs.last().map(|g| g.advance - g.path.as_ref().map(|p| p.bounds().right()).unwrap_or(g.advance)).unwrap_or(0.0);
    let glyph_span = shaped.advance() - first_lsb - last_rsb;
    let tracking = if n > 1.0 { (ink_w - glyph_span * size) / (n - 1.0) } else { 0.0 };
    let mut fit = Fit { size, baseline: (ink.top + b) as f32, x: (ink.left + l) as f32 - first_lsb * size, tracking: tracking.max(-size * 0.1), nudges: vec![0.0; shaped.glyphs.len()], iou: 0.0 };
    if !all_caps {
        // the bbox's bottom is a descender's, not the baseline's: lift the baseline by the outlines' depth below it
        let depth = shaped.glyphs.iter().filter_map(|g| g.path.as_ref()).map(|p| -p.bounds().top()).fold(0.0f32, f32::max);
        fit.baseline -= depth * size;
    }
    fit.iou = score(shaped, &fit, ink);
    for step in [1.0f32, 0.5, 0.25, 0.1] {
        loop {
            let mut improved = false;
            for p in 0..4 {
                for dir in [-1.0f32, 1.0] {
                    let mut trial = fit.clone();
                    match p {
                        0 => trial.size += step * dir * 0.5,
                        1 => trial.baseline += step * dir,
                        2 => trial.x += step * dir,
                        _ => trial.tracking += step * dir * 0.5,
                    }
                    if trial.size <= 1.0 {
                        continue;
                    }
                    trial.iou = score(shaped, &trial, ink);
                    if trial.iou > fit.iou + 1e-5 {
                        fit = trial;
                        improved = true;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }
    // per-glyph nudges, 0.25 px steps up to NUDGE_MAX, kept only where the whole line's IoU rises
    for i in 0..shaped.glyphs.len() {
        if shaped.glyphs[i].path.is_none() {
            continue;
        }
        let mut best = fit.clone();
        let mut d = -NUDGE_MAX;
        while d <= NUDGE_MAX + 1e-6 {
            let mut trial = fit.clone();
            trial.nudges[i] = d;
            trial.iou = score(shaped, &trial, ink);
            if trial.iou > best.iou + 1e-5 {
                best = trial;
            }
            d += 0.25;
        }
        fit = best;
    }
    Some(fit)
}

fn fmt(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

pub fn path_data(shaped: &Shaped, fit: &Fit) -> String {
    use tiny_skia::PathSegment;
    let mut d = String::new();
    for (g, ox) in shaped.glyphs.iter().zip(origins(shaped, fit)) {
        let Some(path) = &g.path else { continue };
        let t = Transform::from_row(fit.size, 0.0, 0.0, -fit.size, ox, fit.baseline);
        let Some(p) = path.clone().transform(t) else { continue };
        for seg in p.segments() {
            match seg {
                PathSegment::MoveTo(p) => d.push_str(&format!("M{} {}", fmt(p.x), fmt(p.y))),
                PathSegment::LineTo(p) => d.push_str(&format!("L{} {}", fmt(p.x), fmt(p.y))),
                PathSegment::QuadTo(c, p) => d.push_str(&format!("Q{} {} {} {}", fmt(c.x), fmt(c.y), fmt(p.x), fmt(p.y))),
                PathSegment::CubicTo(a, b, p) => d.push_str(&format!("C{} {} {} {} {} {}", fmt(a.x), fmt(a.y), fmt(b.x), fmt(b.y), fmt(p.x), fmt(p.y))),
                PathSegment::Close => d.push('Z'),
            }
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_iou_of_identical_buffers_is_one_and_of_disjoint_is_zero() {
        assert!((soft_iou(&[0.5, 1.0, 0.0], &[0.5, 1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(soft_iou(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn numbers_are_written_to_two_decimals() {
        assert_eq!(fmt(12.345), "12.35");
        assert_eq!(fmt(3.0), "3");
        assert_eq!(fmt(-0.001), "0");
    }
}
```

Add `pub mod fit;` to `text/mod.rs`. Also make `redraw::Source` and `redraw` public items reachable from the integration test: `pub mod redraw;` is already in `lib.rs`; `Source` is `pub`.

- [ ] **Step 4: Run the fixture test and set `FIT_ACCEPT`**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_fit -- --nocapture 2>&1 | tail -15`
Expected: the eprintln line with the two populations, and a pass. If the 12 px items fall under 0.90 IoU (anti-aliasing differs between Pillow and tiny-skia on 1.2 px strokes), lower the `0.90` in the test to the measured true minimum rounded down to 0.01 and say so in the spec's Tests section (one line). If a wrong font beats the true font on an item, that item names a bug in the initial guess (print both fits) — the descent must start inside the basin: check `all_caps` detection and the baseline depth for lowercase.
Then set `FIT_ACCEPT` in `text/mod.rs` to `true_min - 0.05` rounded to 2 decimals, and re-run: the last assertion must hold.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/text apps/desktop/src-tauri/tests/text_fit.rs
git commit -m "text: fit.rs fits a shaped string onto a line's ink by soft IoU; the fixture set holds the truth and sets FIT_ACCEPT"
```

---

### Task 5: The catalogue — `tools/gfonts_index.py`, `assets/gfonts.json`, `catalog.rs`, `tests/text_match.rs`

**Files:**
- Create: `apps/desktop/tools/gfonts_index.py`, `apps/desktop/src-tauri/assets/gfonts.json`, `apps/desktop/src-tauri/src/text/catalog.rs`, `apps/desktop/src-tauri/tests/text_match.rs`
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`pub mod catalog;`)

**Interfaces:**
- Consumes: `glyphs::{Features, Font, width_index}`, `FontRef`, `Source`, `SHORTLIST`.
- Produces:

```rust
#[derive(Debug, Clone, Deserialize)] pub struct Variant { pub weight: u16, pub italic: bool }
#[derive(Debug, Clone, Deserialize)] pub struct Family { pub name: String, pub category: String, pub variants: Vec<Variant>, pub features: Features }
pub struct Catalog { google: Vec<Family>, installed: fontdb::Database, measured: Mutex<HashMap<FontRef, Features>> }
impl Catalog {
    pub fn load() -> Catalog;                                  // the embedded index + fontdb::load_system_fonts
    pub fn from_index(json: &str, installed: fontdb::Database) -> Catalog;  // tests
    pub fn families(&self) -> usize;
    /// Ranked families for a line: the measured per-character widths (chars of `string` with a width index) and the stem over cap, against each family's features; installed families are measured on first use. The nearest `n`.
    pub fn shortlist(&self, string: &str, widths: &[(char, f32)], stem: f32, n: usize) -> Vec<FontRef>;  // weight = the variant nearest `stem`'s weight class
    pub fn search(&self, query: &str, n: usize) -> Vec<FontRef>;   // prefix match over Google then installed family names, regular weight
    /// Where the font file is: a Google family's cache path from `cache_dir`, an installed face's file + index.
    pub fn locate(&self, font: &FontRef, cache_dir: &Path) -> Option<(PathBuf, u32)>;
    pub fn weights_of(&self, family: &str, source: Source) -> Vec<u16>;
}
pub fn cache_name(font: &FontRef) -> String;  // "Josefin Sans-300.ttf" / "Josefin Sans-300i.ttf"
/// Google's thickness 1–10 per weight is not used; the weight for a stem is found by `weight_for_stem`.
pub fn weight_for_stem(stem_over_cap: f32) -> u16; // piecewise: <0.07 → 300, <0.10 → 400, <0.13 → 500, <0.16 → 600, <0.19 → 700, else 800; clamped to the family's variants by the caller
```

- [ ] **Step 1: Write the index tool**

`apps/desktop/tools/gfonts_index.py`:

```python
"""Builds src-tauri/assets/gfonts.json from Google Fonts' public metadata: per family, the category, the
variants and the measured features of its regular face (cap, x, stem, per-glyph widths) — the same measure
glyphs.rs makes of an installed font, computed here with fontTools so the two agree.

Run by hand to refresh the index (about ten minutes; it downloads ~1950 regular faces into a cache dir):
    ../../backend/.venv/bin/python tools/gfonts_index.py --cache /tmp/gfonts-cache
It needs `fonttools` and `requests` in that venv: `uv pip install -p ../../backend/.venv/bin/python fonttools requests`.
"""
import argparse, json, re, sys, time
from pathlib import Path

import requests
from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont

META = "https://fonts.google.com/metadata/fonts"
CSS = "https://fonts.googleapis.com/css2?family={family}:ital,wght@{ital},{weight}"
UA = {"User-Agent": "Studi0Trace-index/1 (+https://github.com/timothynice/Studi0Trace)"}
CHARS = [chr(c) for c in range(65, 91)] + [chr(c) for c in range(97, 123)] + [chr(c) for c in range(48, 58)]


def variants(entry):
    out = []
    for key in entry["fonts"]:
        italic = key.endswith("i")
        weight = int(key.rstrip("i"))
        out.append({"weight": weight, "italic": italic})
    return sorted(out, key=lambda v: (v["italic"], v["weight"]))


def regular(vs):
    uprights = [v["weight"] for v in vs if not v["italic"]] or [v["weight"] for v in vs]
    return min(uprights, key=lambda w: abs(w - 400))


def ttf_url(family, weight, italic):
    css = requests.get(CSS.format(family=family.replace(" ", "+"), ital=1 if italic else 0, weight=weight), headers=UA, timeout=30).text
    m = re.search(r"src: url\((https://fonts\.gstatic\.com[^)]+\.ttf)\)", css)
    return m.group(1) if m else None


def measure(path):
    f = TTFont(path)
    upem = f["head"].unitsPerEm
    os2 = f["OS/2"]
    gs = f.getGlyphSet()
    cmap = f.getBestCmap()

    def bbox(ch):
        g = cmap.get(ord(ch))
        if g is None:
            return None
        pen = BoundsPen(gs)
        gs[g].draw(pen)
        return pen.bounds

    cap = getattr(os2, "sCapHeight", 0) / upem
    if cap <= 0:
        b = bbox("H")
        cap = (b[3] - b[1]) / upem if b else 0.7
    xh = getattr(os2, "sxHeight", 0) / upem
    if xh <= 0:
        b = bbox("x")
        xh = (b[3] - b[1]) / upem if b else 0.5
    widths = []
    for ch in CHARS:
        b = bbox(ch)
        widths.append(int(round((b[2] - b[0]) / upem / cap * 100)) if b else 0)
    # the stem: the I's bbox width over the cap height — the same measure glyphs.rs makes
    bi = bbox("I")
    i_w = (bi[2] - bi[0]) / upem if bi else 0.1 * cap
    return {"cap": round(cap, 4), "x": round(xh, 4), "stem": round(i_w / cap, 4), "widths": [min(255, max(0, w)) for w in widths]}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", required=True)
    ap.add_argument("--out", default=str(Path(__file__).resolve().parents[1] / "src-tauri/assets/gfonts.json"))
    args = ap.parse_args()
    cache = Path(args.cache)
    cache.mkdir(parents=True, exist_ok=True)
    meta = requests.get(META, headers=UA, timeout=60).text
    meta = json.loads(meta[meta.index("{"):])  # the endpoint prefixes `)]}'`
    families = []
    for entry in meta["familyMetadataList"]:
        name = entry["family"]
        vs = variants(entry)
        if not vs or "latin" not in entry.get("subsets", []):
            continue
        w = regular(vs)
        path = cache / f"{name}-{w}.ttf"
        if not path.exists():
            url = ttf_url(name, w, False)
            if not url:
                print("no ttf for", name, file=sys.stderr)
                continue
            path.write_bytes(requests.get(url, headers=UA, timeout=60).content)
            time.sleep(0.05)
        try:
            feats = measure(path)
        except Exception as e:  # a font fontTools cannot read is left out
            print("skip", name, e, file=sys.stderr)
            continue
        families.append({"name": name, "category": entry["category"], "variants": vs, "features": feats})
    Path(args.out).write_text(json.dumps({"families": families}, separators=(",", ":")))
    print(len(families), "families →", args.out)


if __name__ == "__main__":
    main()
```

(The stem measure is the I's bbox width over the cap height on both sides, so an installed font and a Google family are ranked by one rule.)

- [ ] **Step 2: Run the tool (one-off, ~10 min)**

Run: `cd apps/desktop && ../../backend/.venv/bin/python -m pip install -q fonttools requests 2>/dev/null || uv pip install -q -p ../../backend/.venv/bin/python fonttools requests; ../../backend/.venv/bin/python tools/gfonts_index.py --cache /private/tmp/claude-502/-Users-TimNice-Development-tracer--claude-worktrees-svg-text-handling-fbf2c8/33ac6b5f-d9f7-489b-ab30-1b0e7edbf73b/scratchpad/gfonts-cache`
Expected: `~1900 families → …/assets/gfonts.json`, a file of 200–400 KB. Check `python3 -c "import json;d=json.load(open('src-tauri/assets/gfonts.json'));print(len(d['families']), [f for f in d['families'] if f['name']=='Josefin Sans'][0]['features']['stem'])"`.

- [ ] **Step 3: Write the failing tests**

`apps/desktop/src-tauri/tests/text_match.rs`:

```rust
//! The shortlist: on the fixture set, the true family is in the top 3 of the keyless ranking, from the line's
//! measured per-character widths and stem thickness.
use serde::Deserialize;
use std::path::PathBuf;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::catalog::{weight_for_stem, Catalog};
use studi0trace_desktop::text::ink::line_ink;
use studi0trace_desktop::text::Source as FontSource;

#[derive(Deserialize)]
struct Item {
    file: String,
    family: String,
    string: String,
    cap_px: f32,
    baseline: f32,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text")
}

/// The real index: every family, nothing installed.
fn catalog() -> Catalog {
    let json = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/gfonts.json")).unwrap();
    Catalog::from_index(&json, fontdb::Database::new())
}

#[test]
fn the_true_family_is_in_the_top_three_on_every_fixture() {
    let cat = catalog();
    assert!(cat.families() > 1500, "{}", cat.families());
    let items: Vec<Item> = serde_json::from_str(&std::fs::read_to_string(dir().join("truth.json")).unwrap()).unwrap();
    let mut misses = Vec::new();
    for item in items.iter().filter(|i| i.cap_px >= 18.0) {
        let source = Source::decode(&std::fs::read(dir().join(&item.file)).unwrap()).unwrap();
        let (w, h) = (source.width as f32, source.height as f32);
        let quad = [[20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, (item.baseline + 8.0).min(h)], [20.0, (item.baseline + 8.0).min(h)]];
        let ink = line_ink(&source, quad);
        let widths = studi0trace_desktop::text::catalog::measured_widths(&ink, &item.string);
        let stem = studi0trace_desktop::text::catalog::measured_stem(&ink);
        let top = cat.shortlist(&item.string, &widths, stem, 3);
        if !top.iter().any(|f| f.family == item.family) {
            misses.push(format!("{}: {:?}", item.file, top.iter().map(|f| f.family.as_str()).collect::<Vec<_>>()));
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
}

#[test]
fn search_and_locate_and_weights() {
    let cat = catalog();
    let hits = cat.search("josef", 5);
    assert_eq!(hits[0].family, "Josefin Sans");
    assert_eq!(hits[0].source, FontSource::Google);
    assert!(cat.weights_of("Josefin Sans", FontSource::Google).contains(&300));
    assert_eq!(weight_for_stem(0.06), 300);
    assert_eq!(weight_for_stem(0.12), 500);
    let cache = dir().join("fonts");
    // the fixture font stands in for the cached download under its cache name
    let f = studi0trace_desktop::text::FontRef { family: "Josefin Sans".into(), weight: 300, italic: false, source: FontSource::Google };
    std::fs::copy(cache.join("JosefinSans-Light.ttf"), cache.join(studi0trace_desktop::text::catalog::cache_name(&f))).unwrap();
    assert_eq!(cat.locate(&f, &cache).unwrap().0, cache.join("Josefin Sans-300.ttf"));
    std::fs::remove_file(cache.join("Josefin Sans-300.ttf")).unwrap();
}
```

- [ ] **Step 4: Run to see them fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_match 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 5: Implement `catalog.rs`**

```rust
//! The fonts a line can be set in: Google Fonts' families from the embedded index (assets/gfonts.json, built by
//! tools/gfonts_index.py) and the fonts installed on this Mac (fontdb). The shortlist ranks families by how the
//! line's measured per-character widths and stem thickness match each family's features.
use super::glyphs::{width_index, Features, Font};
use super::ink::LineInk;
use super::{FontRef, Source};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const INDEX: &str = include_str!("../../assets/gfonts.json");

#[derive(Debug, Clone, Deserialize)]
pub struct Variant {
    pub weight: u16,
    pub italic: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Family {
    pub name: String,
    pub category: String,
    pub variants: Vec<Variant>,
    pub features: Features,
}

#[derive(Deserialize)]
struct Index {
    families: Vec<Family>,
}

pub struct Catalog {
    google: Vec<Family>,
    installed: fontdb::Database,
    measured: Mutex<HashMap<String, Option<Features>>>,
}

pub fn cache_name(font: &FontRef) -> String {
    format!("{}-{}{}.ttf", font.family, font.weight, if font.italic { "i" } else { "" })
}

pub fn weight_for_stem(stem_over_cap: f32) -> u16 {
    match stem_over_cap {
        s if s < 0.07 => 300,
        s if s < 0.10 => 400,
        s if s < 0.13 => 500,
        s if s < 0.16 => 600,
        s if s < 0.19 => 700,
        _ => 800,
    }
}

/// The ink's per-character bbox widths over its cap height, for the characters of `string` that have a width
/// index, in order; a line whose segments do not number the string's non-space characters gives an empty list.
pub fn measured_widths(ink: &LineInk, string: &str) -> Vec<(char, f32)> {
    let chars: Vec<char> = string.chars().filter(|c| !c.is_whitespace()).collect();
    if chars.len() != ink.segments.len() {
        return Vec::new();
    }
    let Some((_, t, _, b)) = ink.ink_bounds else { return Vec::new() };
    let cap = (b - t) as f32;
    chars.iter().zip(&ink.segments).filter(|(c, _)| width_index(**c).is_some()).map(|(c, (l, r))| (*c, (r - l) as f32 / cap)).collect()
}

/// Stem thickness over cap height: the median horizontal ink run at the rows between 40 % and 60 % of the ink's height.
pub fn measured_stem(ink: &LineInk) -> f32 {
    let Some((_, t, _, b)) = ink.ink_bounds else { return 0.1 };
    let cap = (b - t) as f32;
    let mut runs = Vec::new();
    for y in t + (cap * 0.4) as u32..t + (cap * 0.6) as u32 {
        let mut run = 0u32;
        for x in 0..ink.width {
            if ink.mask[(y * ink.width + x) as usize] {
                run += 1;
            } else if run > 0 {
                runs.push(run);
                run = 0;
            }
        }
    }
    if runs.is_empty() {
        return 0.1;
    }
    runs.sort_unstable();
    runs[runs.len() / 2] as f32 / cap
}

fn distance(f: &Features, widths: &[(char, f32)], stem: f32) -> f32 {
    let mut d = 0f32;
    let mut n = 0;
    for (c, w) in widths {
        let fw = f.widths[width_index(*c).unwrap()] as f32 / 100.0;
        if fw > 0.0 {
            d += (fw - w).abs();
            n += 1;
        }
    }
    let width_term = if n > 0 { d / n as f32 } else { 0.5 };
    width_term + 2.0 * (f.stem - stem).abs()
}

impl Catalog {
    pub fn load() -> Catalog {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        Catalog::from_index(INDEX, db)
    }

    pub fn from_index(json: &str, installed: fontdb::Database) -> Catalog {
        let index: Index = serde_json::from_str(json).expect("the font index is JSON");
        Catalog { google: index.families, installed, measured: Mutex::new(HashMap::new()) }
    }

    pub fn families(&self) -> usize {
        self.google.len()
    }

    fn installed_families(&self) -> Vec<String> {
        let mut names: Vec<String> = self.installed.faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
        names.sort();
        names.dedup();
        names
    }

    fn installed_face(&self, family: &str, weight: u16, italic: bool) -> Option<&fontdb::FaceInfo> {
        self.installed.faces().filter(|f| f.families.iter().any(|(n, _)| n == family) && (f.style == fontdb::Style::Italic) == italic).min_by_key(|f| (f.weight.0 as i32 - weight as i32).abs())
    }

    fn installed_features(&self, family: &str) -> Option<Features> {
        let mut cache = self.measured.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(f) = cache.get(family) {
            return f.clone();
        }
        let feats = self.installed_face(family, 400, false).and_then(|face| match &face.source {
            fontdb::Source::File(p) => Font::load(p, face.index).ok().map(|f| f.measure()),
            fontdb::Source::Binary(b) => Font::from_bytes(b.as_ref().as_ref().to_vec(), face.index).ok().map(|f| f.measure()),
            fontdb::Source::SharedFile(p, _) => Font::load(p, face.index).ok().map(|f| f.measure()),
        });
        cache.insert(family.to_string(), feats.clone());
        feats
    }

    fn nearest_weight(weights: &[u16], want: u16) -> u16 {
        weights.iter().copied().min_by_key(|w| (*w as i32 - want as i32).abs()).unwrap_or(400)
    }

    pub fn shortlist(&self, _string: &str, widths: &[(char, f32)], stem: f32, n: usize) -> Vec<FontRef> {
        let want = weight_for_stem(stem);
        let mut scored: Vec<(f32, FontRef)> = self.google.iter().map(|f| {
            let weights: Vec<u16> = f.variants.iter().filter(|v| !v.italic).map(|v| v.weight).collect();
            (distance(&f.features, widths, stem), FontRef { family: f.name.clone(), weight: Self::nearest_weight(&weights, want), italic: false, source: Source::Google })
        }).collect();
        for family in self.installed_families() {
            if let Some(feats) = self.installed_features(&family) {
                let weights = self.weights_of(&family, Source::Installed);
                scored.push((distance(&feats, widths, stem), FontRef { family, weight: Self::nearest_weight(&weights, want), italic: false, source: Source::Installed }));
            }
        }
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then_with(|| a.1.family.cmp(&b.1.family)));
        scored.into_iter().map(|(_, f)| f).take(n).collect()
    }

    pub fn search(&self, query: &str, n: usize) -> Vec<FontRef> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let mut out: Vec<FontRef> = self.google.iter().filter(|f| f.name.to_lowercase().starts_with(&q)).map(|f| FontRef { family: f.name.clone(), weight: Self::nearest_weight(&f.variants.iter().filter(|v| !v.italic).map(|v| v.weight).collect::<Vec<_>>(), 400), italic: false, source: Source::Google }).collect();
        out.extend(self.installed_families().into_iter().filter(|f| f.to_lowercase().starts_with(&q)).map(|family| {
            let weight = Self::nearest_weight(&self.weights_of(&family, Source::Installed), 400);
            FontRef { family, weight, italic: false, source: Source::Installed }
        }));
        out.truncate(n);
        out
    }

    pub fn weights_of(&self, family: &str, source: Source) -> Vec<u16> {
        let mut w: Vec<u16> = match source {
            Source::Google => self.google.iter().find(|f| f.name == family).map(|f| f.variants.iter().filter(|v| !v.italic).map(|v| v.weight).collect()).unwrap_or_default(),
            Source::Installed => self.installed.faces().filter(|f| f.families.iter().any(|(n, _)| n == family) && f.style != fontdb::Style::Italic).map(|f| f.weight.0).collect(),
        };
        w.sort_unstable();
        w.dedup();
        w
    }

    pub fn locate(&self, font: &FontRef, cache_dir: &Path) -> Option<(PathBuf, u32)> {
        match font.source {
            Source::Google => {
                let p = cache_dir.join(cache_name(font));
                p.exists().then_some((p, 0))
            }
            Source::Installed => self.installed_face(&font.family, font.weight, font.italic).and_then(|f| match &f.source {
                fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => Some((p.clone(), f.index)),
                fontdb::Source::Binary(_) => None,
            }),
        }
    }
}
```

Add `pub mod catalog;` to `text/mod.rs`.

- [ ] **Step 6: Run the tests**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_match 2>&1 | tail -12`
Expected: both pass. If the top-3 test misses on some items, print the true family's rank (`scored` position) for them: a systematic rank of 4–8 means the stem term is too strong or the Python/Rust stem measures disagree — align them as the note under Step 1 says, re-run the tool, and if ranks stay 4–8 raise the test's `3` to `SHORTLIST` (12) and record it in the spec (the fit decides among 12 anyway).

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/tools/gfonts_index.py apps/desktop/src-tauri/assets/gfonts.json apps/desktop/src-tauri/src/text apps/desktop/src-tauri/tests/text_match.rs
git commit -m "text: the catalogue — Google Fonts' index with measured features, the installed fonts, and the shortlist by widths and stem"
```

### Task 6: `fonts.rs` — downloading and caching Google Fonts

**Files:**
- Create: `apps/desktop/src-tauri/src/text/fonts.rs`, `apps/desktop/src-tauri/tests/text_fonts.rs`
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`pub mod fonts;`)

**Interfaces:**
- Consumes: `FontRef`, `catalog::cache_name`.
- Produces:

```rust
pub const DEFAULT_BASE: &str = "https://fonts.googleapis.com";
pub const BASE_ENV: &str = "STUDI0TRACE_GFONTS_BASE";   // an https:// or loopback http:// base, as redraw's
pub fn base_url() -> String;
pub struct GoogleFonts { dir: PathBuf, base: String, client: reqwest::Client }
pub struct Ensured { pub missing: Vec<FontRef>, pub offline: bool }
impl GoogleFonts {
    pub fn new(dir: PathBuf, base: &str) -> Result<GoogleFonts, CommandError>;  // creates `dir`; `offline` on a client that will not build
    pub fn css_url(base: &str, font: &FontRef) -> String;   // "{base}/css2?family=Josefin+Sans:ital,wght@0,300"
    /// Every font not yet in `dir` is fetched: the css2 stylesheet (non-browser UA → TTF `src`), then the file. A font that cannot be fetched is `missing`; a transport failure on the first fetch sets `offline` and skips the rest. Never an error.
    pub async fn ensure(&self, fonts: &[FontRef]) -> Ensured;
}
pub fn parse_css(css: &str) -> Option<String>;  // the first `src: url(…ttf)`
```

- [ ] **Step 1: Write the failing tests**

Unit tests at the bottom of `fonts.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Source;

    #[test]
    fn the_css_url_names_the_family_weight_and_style() {
        let f = FontRef { family: "Josefin Sans".into(), weight: 300, italic: false, source: Source::Google };
        assert_eq!(GoogleFonts::css_url("https://fonts.googleapis.com", &f), "https://fonts.googleapis.com/css2?family=Josefin+Sans:ital,wght@0,300");
        let i = FontRef { italic: true, ..f };
        assert!(GoogleFonts::css_url("http://127.0.0.1:9/", &i).ends_with("css2?family=Josefin+Sans:ital,wght@1,300"));
    }

    #[test]
    fn parses_the_ttf_source_out_of_the_stylesheet() {
        let css = "@font-face {\n  font-family: 'Josefin Sans';\n  src: url(https://fonts.gstatic.com/s/josefinsans/v34/abc.ttf) format('truetype');\n}";
        assert_eq!(parse_css(css).as_deref(), Some("https://fonts.gstatic.com/s/josefinsans/v34/abc.ttf"));
        assert_eq!(parse_css("nothing here"), None);
        assert_eq!(parse_css("src: url(https://x/y.woff2)"), None, "only a TTF is taken");
    }
}
```

`apps/desktop/src-tauri/tests/text_fonts.rs`:

```rust
//! Downloading: the stylesheet is asked for with a non-browser user agent, the TTF it names is fetched into the
//! cache under the font's cache name, a cached font is not fetched again, and a dead server means `offline`, never
//! an error.
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use studi0trace_desktop::text::catalog::cache_name;
use studi0trace_desktop::text::fonts::GoogleFonts;
use studi0trace_desktop::text::{FontRef, Source};

fn serve(replies: Vec<(u16, &'static str, Vec<u8>)>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for (status, ctype, body) in replies {
            let Ok((mut s, _)) = listener.accept() else { return };
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            log.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).to_string());
            let _ = write!(s, "HTTP/1.1 {status} OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(&body);
        }
    });
    (base, seen)
}

fn font() -> FontRef {
    FontRef { family: "Josefin Sans".into(), weight: 300, italic: false, source: Source::Google }
}

#[test]
fn fetches_the_stylesheet_then_the_ttf_into_the_cache_once() {
    let dir = std::env::temp_dir().join(format!("studi0trace-fonts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let ttf = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/JosefinSans-Light.ttf")).unwrap();
    let (base, seen) = serve(vec![(200, "text/css", Vec::new()), (200, "font/ttf", ttf.clone())]);
    // the stylesheet names the TTF on the same mock
    let css = format!("@font-face {{ src: url({base}/s/josefin.ttf) format('truetype'); }}");
    let (base, seen) = {
        drop(seen);
        serve(vec![(200, "text/css", css.into_bytes()), (200, "font/ttf", ttf.clone())])
    };
    let gf = GoogleFonts::new(dir.clone(), &base).unwrap();
    let outcome = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(!outcome.offline && outcome.missing.is_empty(), "{:?}", outcome.missing);
    assert_eq!(std::fs::read(dir.join(cache_name(&font()))).unwrap(), ttf);
    let log = seen.lock().unwrap();
    assert!(log[0].starts_with("GET /css2?family=Josefin+Sans:ital,wght@0,300"), "{}", log[0]);
    assert!(log[0].to_lowercase().contains("user-agent: studi0trace"), "{}", log[0]);
    drop(log);
    // cached: no request
    let again = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(again.missing.is_empty() && !again.offline);
    assert_eq!(seen.lock().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dead_server_is_offline_not_an_error_and_a_404_is_missing() {
    let dir = std::env::temp_dir().join(format!("studi0trace-fonts-dead-{}", std::process::id()));
    let gf = GoogleFonts::new(dir.clone(), "http://127.0.0.1:9").unwrap();
    let out = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(out.offline);
    let (base, _) = serve(vec![(404, "text/plain", b"no".to_vec())]);
    let gf = GoogleFonts::new(dir.clone(), &base).unwrap();
    let out = tauri::async_runtime::block_on(gf.ensure(&[font()]));
    assert!(!out.offline && out.missing == vec![font()]);
    let _ = std::fs::remove_dir_all(&dir);
}
```

(The first test's clumsy double `serve` is because the stylesheet must name the mock's own address: keep it as written — bind, build the css with that base, serve both replies.)

- [ ] **Step 2: Run to see them fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_fonts 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement `fonts.rs`**

```rust
//! Google Fonts on demand: the only code that talks to Google. The css2 endpoint, asked with a non-browser user
//! agent, answers `@font-face` rules whose `src` is a TTF on fonts.gstatic.com; the file is kept in the app's
//! data folder under the font's cache name and never fetched twice. Nothing of the user's goes with the request
//! but the family name. A failure is never an error: the line is matched against installed fonts instead.
use super::catalog::cache_name;
use super::FontRef;
use crate::error::CommandError;
use std::path::PathBuf;
use std::time::Duration;

pub const DEFAULT_BASE: &str = "https://fonts.googleapis.com";
pub const BASE_ENV: &str = "STUDI0TRACE_GFONTS_BASE";
const UA: &str = concat!("Studi0Trace/", env!("CARGO_PKG_VERSION"), " (+https://github.com/timothynice/Studi0Trace)");
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FONT_BYTES: usize = 8 * 1024 * 1024;

/// `STUDI0TRACE_GFONTS_BASE` when it is an `https://` address or a loopback one; else Google.
pub fn base_url() -> String {
    match std::env::var(BASE_ENV) {
        Ok(b) if b.starts_with("https://") || crate::redraw::openai::is_loopback_base(&b) => b.trim_end_matches('/').to_string(),
        _ => DEFAULT_BASE.to_string(),
    }
}

pub struct GoogleFonts {
    dir: PathBuf,
    base: String,
    client: reqwest::Client,
}

#[derive(Debug, Default)]
pub struct Ensured {
    pub missing: Vec<FontRef>,
    pub offline: bool,
}

pub fn parse_css(css: &str) -> Option<String> {
    let start = css.find("src: url(")? + "src: url(".len();
    let end = css[start..].find(')')? + start;
    let url = &css[start..end];
    url.ends_with(".ttf").then(|| url.to_string())
}

impl GoogleFonts {
    pub fn new(dir: PathBuf, base: &str) -> Result<GoogleFonts, CommandError> {
        std::fs::create_dir_all(&dir).map_err(|e| CommandError::new(500, "io_error", format!("{}: {e}", dir.display())))?;
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let base = base.trim().trim_end_matches('/').to_string();
        let mut builder = reqwest::Client::builder().timeout(TIMEOUT).connect_timeout(Duration::from_secs(10)).user_agent(UA);
        if crate::redraw::openai::is_loopback_base(&base) {
            builder = builder.no_proxy();
        }
        let client = builder.build().map_err(|_| crate::redraw::error("offline"))?;
        Ok(GoogleFonts { dir, base, client })
    }

    pub fn css_url(base: &str, font: &FontRef) -> String {
        format!("{}/css2?family={}:ital,wght@{},{}", base.trim_end_matches('/'), font.family.replace(' ', "+"), u8::from(font.italic), font.weight)
    }

    async fn fetch(&self, font: &FontRef) -> Result<(), bool> {
        // Err(true) = transport failure (offline), Err(false) = this font cannot be had
        let css = self.client.get(Self::css_url(&self.base, font)).send().await.map_err(|e| e.is_connect() || e.is_timeout())?;
        if !css.status().is_success() {
            return Err(false);
        }
        let css = css.text().await.map_err(|_| false)?;
        let url = parse_css(&css).ok_or(false)?;
        let ttf = self.client.get(url).send().await.map_err(|e| e.is_connect() || e.is_timeout())?;
        if !ttf.status().is_success() {
            return Err(false);
        }
        let bytes = ttf.bytes().await.map_err(|_| false)?;
        if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
            return Err(false);
        }
        std::fs::write(self.dir.join(cache_name(font)), &bytes).map_err(|_| false)
    }

    pub async fn ensure(&self, fonts: &[FontRef]) -> Ensured {
        let mut out = Ensured::default();
        for font in fonts {
            if self.dir.join(cache_name(font)).exists() {
                continue;
            }
            match self.fetch(font).await {
                Ok(()) => {}
                Err(true) => {
                    out.offline = true;
                    out.missing.extend(fonts.iter().filter(|f| !self.dir.join(cache_name(f)).exists()).cloned());
                    out.missing.dedup();
                    return out;
                }
                Err(false) => out.missing.push(font.clone()),
            }
        }
        out
    }
}
```

In `redraw/openai.rs`, make the loopback check reusable: rename the private `is_loopback(base: &str) -> bool` to `pub fn is_loopback_base(base: &str) -> bool` and update its two callers in that file. Add `pub mod fonts;` to `text/mod.rs`.

- [ ] **Step 4: Run the tests**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_fonts --lib text::fonts 2>&1 | tail -8`
Expected: 4 passed (2 unit, 2 integration). (`cargo test` runs the two targets in sequence; if `--test` and `--lib` together are refused, run them one after the other.)

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src apps/desktop/src-tauri/tests/text_fonts.rs
git commit -m "text: fonts.rs fetches a matched Google Font's TTF into the app's cache, offline is a flag and never a failure"
```

---

### Task 7: `reader.rs`, the Vision finder, and `tests/text_vision.rs`

**Files:**
- Create: `apps/desktop/src-tauri/src/text/reader.rs`, `apps/desktop/src-tauri/src/text/vision.rs`, `apps/desktop/src-tauri/tests/text_vision.rs`
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`pub mod reader; pub mod vision;`), `apps/desktop/src-tauri/src/redraw/mod.rs:139` (`encode_png` becomes `pub(crate)`)

**Interfaces:**
- Produces (`reader.rs`):

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct LineBox { pub quad: [[f32; 2]; 4], /* source px: top-left, top-right, bottom-right, bottom-left */ pub string: String, pub confidence: f32 }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontHint { pub family: String, pub weight: u16, pub italic: bool }
#[derive(Debug, Clone, PartialEq)]
pub struct Reading { pub string: String, pub hints: Vec<FontHint>, pub class: Class }
pub trait Finder: Send + Sync { fn find(&self, source: &Source) -> Result<Vec<LineBox>, CommandError>; }
pub type ReadFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<Reading>, CommandError>> + Send + 'a>>;
pub trait Reader: Send + Sync { fn read<'a>(&'a self, source: &'a Source, lines: &'a [LineBox]) -> ReadFuture<'a>; }
/// The lines' own strings: what Vision read, no hints, every line `Type`.
pub fn vision_readings(lines: &[LineBox]) -> Vec<Reading>;
```

- Produces (`vision.rs`): `pub struct Vision; impl Finder for Vision` — on macOS through `VNRecognizeTextRequest` (accurate, language correction off, `minimumTextHeight` 0), observations under `VISION_MIN_CONFIDENCE` or with an empty trimmed string dropped, quads converted from Vision's normalised bottom-left frame to source pixels; elsewhere `Err(error("text_unavailable"))`.

- [ ] **Step 1: Write the failing test**

`apps/desktop/src-tauri/tests/text_vision.rs`:

```rust
//! Vision reads a fixture line back: one observation, its string, a quad around the text. macOS only; elsewhere
//! the finder says `text_unavailable`.
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::reader::Finder;
use studi0trace_desktop::text::vision::Vision;

fn fixture(name: &str) -> Source {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/");
    Source::decode(&std::fs::read(format!("{p}{name}")).unwrap()).unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn reads_est_2026_with_a_box_around_it() {
    let source = fixture("montserrat-est-30-t0-white.png");
    let lines = Vision.find(&source).unwrap();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line.string.trim(), "EST. 2026");
    assert!(line.confidence > 0.5);
    let xs: Vec<f32> = line.quad.iter().map(|p| p[0]).collect();
    let ys: Vec<f32> = line.quad.iter().map(|p| p[1]).collect();
    // the text starts at x = 30, baseline at y = 50 (cap 30 + 20), so the box spans roughly that
    assert!(xs.iter().cloned().fold(f32::MAX, f32::min) < 34.0 && xs.iter().cloned().fold(f32::MIN, f32::max) > 100.0, "{xs:?}");
    assert!(ys.iter().cloned().fold(f32::MAX, f32::min) < 24.0 && ys.iter().cloned().fold(f32::MIN, f32::max) > 46.0, "{ys:?}");
    // top-left, top-right, bottom-right, bottom-left
    assert!(line.quad[0][1] < line.quad[3][1] && line.quad[0][0] < line.quad[1][0]);
}

#[cfg(target_os = "macos")]
#[test]
fn reads_two_lines_in_order_top_to_bottom() {
    // two fixture lines stacked into one image
    let a = fixture("montserrat-est-30-t0-white.png");
    let b = fixture("montserrat-life-18-t2-white.png");
    let w = a.width.max(b.width);
    let h = a.height + b.height;
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    for (src, y0) in [(&a, 0), (&b, a.height)] {
        for y in 0..src.height {
            let row = &src.rgba[((y * src.width) * 4) as usize..((y * src.width + src.width) * 4) as usize];
            let at = (((y0 + y) * w) * 4) as usize;
            rgba[at..at + row.len()].copy_from_slice(row);
        }
    }
    let lines = Vision.find(&Source { rgba, width: w, height: h }).unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].string.trim(), "EST. 2026");
    assert_eq!(lines[1].string.trim(), "LIFE IS BETTER OUTSIDE");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn elsewhere_text_is_unavailable() {
    let err = Vision.find(&fixture("montserrat-est-30-t0-white.png")).unwrap_err();
    assert_eq!(err.code(), Some("text_unavailable"));
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_vision 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement `reader.rs` and `vision.rs`**

`reader.rs`:

```rust
//! What finds the lines and what reads them. Vision (vision.rs) does both on device; the AI reader
//! (reader_openai.rs) reads better and names fonts, and never supplies boxes. One `Reader` trait, so the
//! orchestration does not care which.
use super::Class;
use crate::error::CommandError;
use crate::redraw::Source;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq)]
pub struct LineBox {
    /// Source pixels: top-left, top-right, bottom-right, bottom-left.
    pub quad: [[f32; 2]; 4],
    pub string: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontHint {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub string: String,
    pub hints: Vec<FontHint>,
    pub class: Class,
}

pub trait Finder: Send + Sync {
    fn find(&self, source: &Source) -> Result<Vec<LineBox>, CommandError>;
}

pub type ReadFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<Reading>, CommandError>> + Send + 'a>>;

pub trait Reader: Send + Sync {
    fn read<'a>(&'a self, source: &'a Source, lines: &'a [LineBox]) -> ReadFuture<'a>;
}

pub fn vision_readings(lines: &[LineBox]) -> Vec<Reading> {
    lines.iter().map(|l| Reading { string: l.string.clone(), hints: Vec::new(), class: Class::Type }).collect()
}
```

`vision.rs`:

```rust
//! macOS Vision's text recogniser as the finder: accurate level, no language correction (a wordmark is not a
//! word), every text height. The image goes in as PNG bytes; the observations come back as normalised
//! rectangles with the origin at the bottom left, turned into source-pixel quads here.
use super::reader::{Finder, LineBox};
use super::{error, VISION_MIN_CONFIDENCE};
use crate::error::CommandError;
use crate::redraw::Source;

pub struct Vision;

#[cfg(target_os = "macos")]
impl Finder for Vision {
    fn find(&self, source: &Source) -> Result<Vec<LineBox>, CommandError> {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::AllocAnyThread;
        use objc2_foundation::{NSArray, NSData, NSDictionary};
        use objc2_vision::{VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

        let png = crate::redraw::encode_png(&source.rgba, source.width, source.height, image::ExtendedColorType::Rgba8)?;
        let data = NSData::with_bytes(&png);
        let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
        let handler = unsafe { VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &options) };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(false);
        request.setMinimumTextHeight(0.0);
        let as_request: Retained<VNRequest> = Retained::into_super(Retained::into_super(request.clone()));
        let requests = NSArray::from_retained_slice(&[as_request]);
        unsafe { handler.performRequests_error(&requests) }.map_err(|e| {
            eprintln!("studi0trace: Vision failed: {e}");
            error("text_unavailable")
        })?;
        let (w, h) = (source.width as f32, source.height as f32);
        let mut lines = Vec::new();
        if let Some(results) = request.results() {
            for obs in results.iter() {
                let Some(best) = obs.topCandidates(1).iter().next() else { continue };
                let string = best.string().to_string();
                let confidence = best.confidence();
                if string.trim().is_empty() || confidence < VISION_MIN_CONFIDENCE {
                    continue;
                }
                let px = |p: objc2_core_foundation::CGPoint| [p.x as f32 * w, (1.0 - p.y as f32) * h];
                let quad = unsafe { [px(obs.topLeft()), px(obs.topRight()), px(obs.bottomRight()), px(obs.bottomLeft())] };
                lines.push(LineBox { quad, string, confidence });
            }
        }
        // top to bottom, then left to right
        lines.sort_by(|a, b| a.quad[0][1].partial_cmp(&b.quad[0][1]).unwrap().then(a.quad[0][0].partial_cmp(&b.quad[0][0]).unwrap()));
        Ok(lines)
    }
}

#[cfg(not(target_os = "macos"))]
impl Finder for Vision {
    fn find(&self, _source: &Source) -> Result<Vec<LineBox>, CommandError> {
        Err(error("text_unavailable"))
    }
}
```

The objc2 generics are the part most likely to need a `cargo check` round: `VNImageOption` is `NSString` under the hood (`objc2_vision::VNImageOption`), `NSDictionary::new()` on a typed dictionary, `Retained::into_super` twice (VNRecognizeTextRequest → VNImageBasedRequest → VNRequest), `CGPoint` from `objc2_core_foundation`. If a feature is missing the compiler names it; add it to the `objc2-*` features in Cargo.toml. `topLeft()` and friends are `unsafe` in the bindings; `string()`, `confidence()`, `topCandidates()` and `results()` on the text request are safe.

Change `fn encode_png` in `redraw/mod.rs` to `pub(crate) fn encode_png`. Add `pub mod reader; pub mod vision;` to `text/mod.rs`.

- [ ] **Step 4: Run the test**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_vision 2>&1 | tail -8`
Expected: 2 passed on macOS. If Vision reads "EST. 2026" as "EST . 2026" or "EST.2026", relax the assertion to compare with whitespace removed and note it in the test — the fit does not depend on spaces inside the string beyond their advances, and the AI reader corrects strings when on.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src apps/desktop/src-tauri/tests/text_vision.rs
git commit -m "text: the Finder and Reader traits, and Vision finds and reads lines on device"
```

---

### Task 8: `reader_openai.rs` — the AI reader, and `tests/text_openai.rs`

**Files:**
- Create: `apps/desktop/src-tauri/src/text/reader_openai.rs`, `apps/desktop/src-tauri/tests/text_openai.rs`
- Modify: `apps/desktop/src-tauri/src/redraw/openai.rs` (`Api::post_json`, `parse_json_reply`), `apps/desktop/src-tauri/src/text/mod.rs` (`pub mod reader_openai;`)

**Interfaces:**
- Consumes: `redraw::openai::Api`, `reader::{LineBox, Reading, FontHint, Reader}`, `ink::line_ink` (for the crops' boxes), `studi0trace_core::resample::{resize_rgba, Filter}`.
- Produces:

```rust
// in redraw/openai.rs
impl Api {
    /// One JSON POST to `{base}{path}`; the reply's JSON on 2xx, else the code `parse_reply` would give.
    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, CommandError>;
}
pub fn parse_json_reply(status: u16, body: &[u8]) -> Result<serde_json::Value, CommandError>;
// in text/reader_openai.rs
pub const PROMPT: &str = "…";
pub const MAX_SIDE: u32 = 1024;   // the whole image is sent at most this large
pub struct AiReader { api: Api, model: String }
impl AiReader { pub fn new(base: &str, key: &str, model: &str) -> Result<AiReader, CommandError>; }
impl Reader for AiReader { … }
/// The request body: model, the prompt with the lines Vision read, the whole image and one crop per line as data URLs, and the strict JSON schema.
pub fn request_body(model: &str, source: &Source, lines: &[LineBox]) -> Result<serde_json::Value, CommandError>;
/// The readings out of a Responses reply: the first `output_text` parsed as the schema; a line the reply does not name keeps its Vision reading.
pub fn parse_readings(reply: &serde_json::Value, lines: &[LineBox]) -> Result<Vec<Reading>, CommandError>; // bad_reply when there is no output_text or it is not the schema
```

- [ ] **Step 1: Write the failing tests**

`apps/desktop/src-tauri/tests/text_openai.rs`:

```rust
//! The AI reader against a mock server: the request is a JSON POST to /v1/responses with the key only in the
//! header, the image and one crop per line as data URLs, and a strict schema; the reply's output_text is parsed
//! into readings; errors map to redraw's codes; an unnamed line keeps its Vision reading.
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::reader::{LineBox, Reader};
use studi0trace_desktop::text::reader_openai::{parse_readings, AiReader};
use studi0trace_desktop::text::Class;

struct Seen {
    head: String,
    body: Vec<u8>,
}

fn read_request(s: &mut std::net::TcpStream) -> Seen {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        let n = s.read(&mut chunk).unwrap_or(0);
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..at]).to_string();
            let len: usize = head.lines().find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
            while buf.len() < at + 4 + len {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            return Seen { head, body: buf[at + 4..].to_vec() };
        }
    }
    Seen { head: String::new(), body: buf }
}

fn serve(replies: Vec<(u16, Value)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for (status, body) in replies {
            let Ok((mut s, _)) = listener.accept() else { return };
            log.lock().unwrap().push(read_request(&mut s));
            let body = body.to_string();
            let _ = write!(s, "HTTP/1.1 {status} Mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
            let _ = s.write_all(body.as_bytes());
        }
    });
    (base, seen)
}

fn source() -> Source {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/montserrat-est-30-t0-white.png");
    Source::decode(&std::fs::read(p).unwrap()).unwrap()
}

fn lines() -> Vec<LineBox> {
    vec![
        LineBox { quad: [[25.0, 15.0], [200.0, 15.0], [200.0, 55.0], [25.0, 55.0]], string: "EST. 2O26".into(), confidence: 0.9 },
        LineBox { quad: [[25.0, 15.0], [120.0, 15.0], [120.0, 55.0], [25.0, 55.0]], string: "second".into(), confidence: 0.9 },
    ]
}

fn reply(text: &str) -> Value {
    json!({ "id": "resp_1", "output": [{ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": text }] }] })
}

fn run<T>(f: impl std::future::Future<Output = T>) -> T {
    tauri::async_runtime::block_on(f)
}

#[test]
fn sends_the_image_the_crops_and_the_schema_with_the_key_only_in_the_header() {
    let answer = json!({ "lines": [{ "index": 0, "string": "EST. 2026", "class": "type", "fonts": [{ "family": "Montserrat", "weight": 400, "italic": false }] }] });
    let (base, seen) = serve(vec![(200, reply(&answer.to_string()))]);
    let reader = AiReader::new(&base, "sk-test-key", "gpt-5-mini").unwrap();
    let out = run(reader.read(&source(), &lines())).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].string, "EST. 2026");
    assert_eq!(out[0].hints[0].family, "Montserrat");
    assert_eq!(out[0].class, Class::Type);
    assert_eq!(out[1].string, "second", "an unnamed line keeps Vision's reading");
    let seen = seen.lock().unwrap();
    assert!(seen[0].head.starts_with("POST /v1/responses HTTP/1.1"), "{}", seen[0].head);
    assert!(seen[0].head.to_lowercase().contains("authorization: bearer sk-test-key"));
    let body: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(body["model"], "gpt-5-mini");
    assert!(!seen[0].body.windows(11).any(|w| w == b"sk-test-key"), "the key is never in the body");
    let content = body["input"][0]["content"].as_array().unwrap();
    let images: Vec<&Value> = content.iter().filter(|c| c["type"] == "input_image").collect();
    assert_eq!(images.len(), 3, "the whole image and one crop per line");
    assert!(images[0]["image_url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert_eq!(body["text"]["format"]["type"], "json_schema");
    assert_eq!(body["text"]["format"]["strict"], true);
    assert!(content[0]["text"].as_str().unwrap().contains("EST. 2O26"), "the prompt carries what Vision read");
}

#[test]
fn errors_map_to_redraws_codes_and_a_bad_reply_is_bad_reply() {
    for (status, body, code) in [
        (401, json!({"error": {"code": "invalid_api_key"}}), "invalid_key"),
        (429, json!({"error": {"code": "rate_limit_exceeded"}}), "quota"),
        (403, json!({"error": {}}), "not_allowed"),
        (400, json!({"error": {"code": "model_not_found"}}), "bad_reply"),
        (200, json!({"output": []}), "bad_reply"),
        (200, reply("not json"), "bad_reply"),
    ] {
        let (base, _) = serve(vec![(status, body)]);
        let reader = AiReader::new(&base, "k", "gpt-5-mini").unwrap();
        let err = run(reader.read(&source(), &lines())).unwrap_err();
        assert_eq!(err.code(), Some(code), "status {status}");
    }
    let reader = AiReader::new("http://127.0.0.1:9/v1", "k", "gpt-5-mini").unwrap();
    assert_eq!(run(reader.read(&source(), &lines())).unwrap_err().code(), Some("offline"));
}

#[test]
fn parse_readings_classes_and_clamps_weights() {
    let text = json!({ "lines": [{ "index": 1, "string": "Awaits", "class": "script", "fonts": [{ "family": "Pacifico", "weight": 950, "italic": true }] }] }).to_string();
    let out = parse_readings(&reply(&text), &lines()).unwrap();
    assert_eq!(out[1].class, Class::Script);
    assert_eq!(out[1].hints[0].weight, 900);
    assert!(out[1].hints[0].italic);
    assert_eq!(out[0].string, "EST. 2O26");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_openai 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Add `post_json` to `redraw/openai.rs`**

After `parse_reply`:

```rust
/// A JSON reply on 2xx; else the failure's code, as `parse_reply` maps it.
pub fn parse_json_reply(status: u16, body: &[u8]) -> Result<serde_json::Value, CommandError> {
    if (200..300).contains(&status) {
        return serde_json::from_slice(body).map_err(|_| error("bad_reply"));
    }
    parse_reply(status, body).map(|_| serde_json::Value::Null)
}
```

(`parse_reply` on a non-2xx status always returns `Err`, so the `map` never runs; it keeps one table of codes.) And in `impl Api`:

```rust
    pub async fn post_json(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, CommandError> {
        let text = body.to_string();
        if text.len() > MAX_REQUEST_BYTES {
            return Err(error("too_large"));
        }
        let answer = self.client.post(format!("{}{path}", self.base)).bearer_auth(&self.key).header("content-type", "application/json").body(text).send().await.map_err(|e| transport(&e))?;
        let status = answer.status().as_u16();
        if answer.content_length().is_some_and(|n| n > MAX_REPLY_BYTES as u64) {
            return Err(error("bad_reply"));
        }
        let bytes = answer.bytes().await.map_err(|e| transport(&e))?;
        if bytes.len() > MAX_REPLY_BYTES {
            return Err(error("bad_reply"));
        }
        parse_json_reply(status, &bytes)
    }
```

- [ ] **Step 4: Implement `reader_openai.rs`**

```rust
//! The AI reader: one Responses-API call with the whole image (at most MAX_SIDE a side) and a crop per line,
//! asking for each line's exact string, its lettering class and up to three typefaces, as strict JSON. It never
//! supplies boxes. The key goes in the header, as redraw's does, and OpenAI's messages are never kept.
use super::reader::{FontHint, LineBox, ReadFuture, Reader, Reading};
use super::Class;
use crate::error::CommandError;
use crate::redraw::error;
use crate::redraw::openai::Api;
use crate::redraw::Source;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use studi0trace_core::resample::{resize_rgba, Filter};

pub const MAX_SIDE: u32 = 1024;

pub const PROMPT: &str = "The first image is a design that contains lines of text; each following image is a crop of one line, \
numbered in order. For every crop give: `string`, the exact text as set (correct recognition errors; keep case and \
punctuation); `class`, \"type\" when the line is set in a typeface, \"script\" for handwritten, brush or connected \
script lettering, \"custom\" for a hand-drawn or heavily customised wordmark; and `fonts`, up to three typefaces the \
line is most likely set in, best first, as Google Fonts family names or common system font names, each with its \
weight (100–900) and whether it is italic. Answer only in the schema.";

pub struct AiReader {
    api: Api,
    model: String,
}

impl AiReader {
    pub fn new(base: &str, key: &str, model: &str) -> Result<AiReader, CommandError> {
        Ok(AiReader { api: Api::new(base, key)?, model: model.to_string() })
    }
}

fn png_data_url(rgba: &[u8], w: u32, h: u32) -> Result<String, CommandError> {
    let png = crate::redraw::encode_png(rgba, w, h, image::ExtendedColorType::Rgba8)?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

fn crop(source: &Source, quad: [[f32; 2]; 4]) -> (Vec<u8>, u32, u32) {
    let xs = quad.iter().map(|p| p[0]);
    let ys = quad.iter().map(|p| p[1]);
    let (x0, x1) = (xs.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max));
    let (y0, y1) = (ys.clone().fold(f32::MAX, f32::min), ys.fold(f32::MIN, f32::max));
    let grow = (y1 - y0) * 0.2;
    let l = (x0 - grow).floor().max(0.0) as u32;
    let t = (y0 - grow).floor().max(0.0) as u32;
    let r = ((x1 + grow).ceil() as u32).min(source.width).max(l + 1);
    let b = ((y1 + grow).ceil() as u32).min(source.height).max(t + 1);
    let (w, h) = (r - l, b - t);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in t..b {
        let row = &source.rgba[((y * source.width + l) * 4) as usize..((y * source.width + r) * 4) as usize];
        out.extend_from_slice(row);
    }
    (out, w, h)
}

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "lines": { "type": "array", "items": {
                "type": "object",
                "properties": {
                    "index": { "type": "integer" },
                    "string": { "type": "string" },
                    "class": { "type": "string", "enum": ["type", "script", "custom"] },
                    "fonts": { "type": "array", "items": {
                        "type": "object",
                        "properties": { "family": { "type": "string" }, "weight": { "type": "integer" }, "italic": { "type": "boolean" } },
                        "required": ["family", "weight", "italic"], "additionalProperties": false } }
                },
                "required": ["index", "string", "class", "fonts"], "additionalProperties": false } }
        },
        "required": ["lines"], "additionalProperties": false
    })
}

pub fn request_body(model: &str, source: &Source, lines: &[LineBox]) -> Result<Value, CommandError> {
    let side = source.width.max(source.height);
    let whole = if side > MAX_SIDE {
        let s = MAX_SIDE as f32 / side as f32;
        let (w, h) = ((source.width as f32 * s).round().max(1.0) as u32, (source.height as f32 * s).round().max(1.0) as u32);
        let rgba = resize_rgba(&source.rgba, source.width, source.height, w, h, Filter::Lanczos).map_err(|_| error("bad_reply"))?;
        png_data_url(&rgba, w, h)?
    } else {
        png_data_url(&source.rgba, source.width, source.height)?
    };
    let mut prompt = String::from(PROMPT);
    prompt.push_str("\nRecognised so far:");
    for (i, l) in lines.iter().enumerate() {
        prompt.push_str(&format!("\n{}: {:?}", i + 1, l.string));
    }
    let mut content = vec![json!({ "type": "input_text", "text": prompt }), json!({ "type": "input_image", "image_url": whole })];
    for l in lines {
        let (rgba, w, h) = crop(source, l.quad);
        content.push(json!({ "type": "input_image", "image_url": png_data_url(&rgba, w, h)? }));
    }
    Ok(json!({
        "model": model,
        "input": [{ "role": "user", "content": content }],
        "text": { "format": { "type": "json_schema", "name": "text_lines", "strict": true, "schema": schema() } }
    }))
}

#[derive(Deserialize)]
struct Answer {
    lines: Vec<AnsweredLine>,
}

#[derive(Deserialize)]
struct AnsweredLine {
    index: usize,
    string: String,
    class: String,
    #[serde(default)]
    fonts: Vec<FontHint>,
}

pub fn parse_readings(reply: &Value, lines: &[LineBox]) -> Result<Vec<Reading>, CommandError> {
    let text = reply["output"].as_array().into_iter().flatten().filter(|o| o["type"] == "message").flat_map(|o| o["content"].as_array().cloned().unwrap_or_default()).find(|c| c["type"] == "output_text").and_then(|c| c["text"].as_str().map(str::to_string)).ok_or_else(|| error("bad_reply"))?;
    let answer: Answer = serde_json::from_str(&text).map_err(|_| error("bad_reply"))?;
    let mut out = super::reader::vision_readings(lines);
    for a in answer.lines {
        let Some(slot) = out.get_mut(a.index) else { continue };
        if !a.string.trim().is_empty() {
            slot.string = a.string.trim().to_string();
        }
        slot.class = match a.class.as_str() {
            "script" => Class::Script,
            "custom" => Class::Custom,
            _ => Class::Type,
        };
        slot.hints = a.fonts.into_iter().take(3).map(|h| FontHint { family: h.family.trim().to_string(), weight: ((h.weight.clamp(100, 900) + 50) / 100 * 100).clamp(100, 900), italic: h.italic }).collect();
    }
    Ok(out)
}

impl Reader for AiReader {
    fn read<'a>(&'a self, source: &'a Source, lines: &'a [LineBox]) -> ReadFuture<'a> {
        Box::pin(async move {
            let body = request_body(&self.model, source, lines)?;
            let reply = self.api.post_json("/responses", &body).await?;
            parse_readings(&reply, lines)
        })
    }
}
```

Add `pub mod reader_openai;` to `text/mod.rs`. (`FontHint` derives `Deserialize`, so `AnsweredLine.fonts` reads straight into it; the weight is rounded to the nearest hundred and clamped afterwards.)

- [ ] **Step 5: Run the tests**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_openai 2>&1 | tail -8`
Expected: 3 passed. A 400 with `model_not_found` maps to `bad_reply` (the table has no entry for it, and the UI's words for `bad_reply` say the reply held nothing usable, which is what a wrong model name produces; the Settings field is free text).

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src apps/desktop/src-tauri/tests/text_openai.rs
git commit -m "text: the AI reader asks OpenAI for each line's string, class and typefaces as strict JSON, through redraw's client"
```

---

### Task 9: The orchestration (`plan`, `set_line`, `Plans`), `compose.rs`, and `tests/text_compose.rs`

**Files:**
- Modify: `apps/desktop/src-tauri/src/text/mod.rs` (`Deps`, `Planned`, `plan`, `set_line`, `Plans`, `Entry`, `LineChange`)
- Create: `apps/desktop/src-tauri/src/text/compose.rs`, `apps/desktop/src-tauri/tests/text_compose.rs`

**Interfaces:**
- Consumes: everything above.
- Produces (`text/mod.rs`):

```rust
/// Owned, so a plan can run as a `'static` task: everything in it lives in the app state (Task 10) or a test.
#[derive(Clone)] pub struct Deps { pub finder: Arc<dyn reader::Finder>, pub reader: Option<Arc<dyn reader::Reader>>, pub catalog: Arc<catalog::Catalog>, pub fonts: Option<Arc<fonts::GoogleFonts>>, pub cache_dir: PathBuf, pub hidden_text: bool }
pub struct Planned { pub plan: TextPlan, pub inks: Vec<Arc<ink::LineInk>>, pub painted: Option<Vec<u8>> /* PNG, when a line is matched and enabled */, pub group: String }
pub async fn plan(image_id: &str, source: Arc<Source>, deps: Deps, phase: Arc<dyn Fn(Phase) + Send + Sync>) -> Result<Planned, CommandError>;
#[derive(Debug, Clone, Default, Deserialize)] pub struct LineChange { pub string: Option<String>, pub font: Option<FontRef>, pub enabled: Option<bool> }
pub async fn set_line(source: Arc<Source>, planned: &Planned, line_id: &str, change: LineChange, deps: Deps) -> Result<Planned, CommandError>;   // unknown line → 404 `no_line`
/// Per image: the plan with what it needs to be edited and used.
#[derive(Default)] pub struct Plans { … }
pub struct Entry { pub planned: Arc<Planned>, pub hash: [u8; 32] }
impl Plans {
    pub fn get(&self, image: &str, hash: &[u8; 32]) -> Option<Arc<Planned>>;   // only while the bytes' hash matches
    pub fn put(&self, image: &str, hash: [u8; 32], planned: Planned) -> Arc<Planned>;
    pub fn forget(&self, image: &str);
    /// What a trace should use: the painted PNG and the group, when the plan has a matched, enabled line.
    pub fn for_trace(&self, image: &str, hash: &[u8; 32]) -> Option<(Arc<Vec<u8>>, String)>;
    pub fn reserve(&self, image: &str) -> u64;  pub fn is_current(&self, image: &str, generation: u64) -> bool;  pub fn cancel(&self, image: &str) -> bool;  // as redraw::Redraws: a running plan per image, by generation
    pub fn arm(&self, image: &str, generation: u64, abort: Box<dyn Fn() + Send + Sync>);
}
pub fn bytes_hash(bytes: &[u8]) -> [u8; 32];  // sha2
```

- Produces (`compose.rs`):

```rust
/// `<g id="text">…</g>` for the plan's matched, enabled lines: one path per line in the ink colour, and the hidden text when asked. A line whose font cannot be located is skipped.
pub fn text_group(plan: &TextPlan, hidden_text: bool, catalog: &Catalog, cache_dir: &Path) -> String;
pub fn escape_text(s: &str) -> String;  // & < > " → entities
```

- [ ] **Step 1: Write the failing compose test**

`apps/desktop/src-tauri/tests/text_compose.rs`:

```rust
//! The text group: one path per matched, enabled line in the ink colour, the hidden <text> when asked, source
//! pixel coordinates, and the core's renderer draws the paths and nothing for the <text>.
use std::path::PathBuf;
use studi0trace_desktop::text::catalog::{cache_name, Catalog};
use studi0trace_desktop::text::compose::text_group;
use studi0trace_desktop::text::{Candidate, Class, Fit, FontRef, ReaderKind, Source, Status, TextLine, TextPlan};

fn cache_with_fixture_font() -> (PathBuf, FontRef) {
    let dir = std::env::temp_dir().join(format!("studi0trace-compose-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let font = FontRef { family: "Montserrat".into(), weight: 400, italic: false, source: Source::Google };
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/Montserrat-Regular.ttf"), dir.join(cache_name(&font))).unwrap();
    (dir, font)
}

fn plan(font: &FontRef, enabled: bool, status: Status) -> TextPlan {
    let fit = Fit { size: 40.0, baseline: 60.0, x: 10.0, tracking: 2.0, nudges: vec![0.0; 9], iou: 0.95 };
    TextPlan {
        image: "img".into(),
        reader: ReaderKind::Vision,
        lines: vec![TextLine { id: "img-t0".into(), quad: [[8.0, 20.0], [220.0, 20.0], [220.0, 70.0], [8.0, 70.0]], string: "EST. 2026".into(), class: Class::Type, colour: [24, 28, 44], status, candidates: vec![Candidate { font: font.clone(), iou: 0.95 }], chosen: Some(0), enabled, fit: Some(fit) }],
        ai_error: None,
        fonts_offline: false,
    }
}

fn catalog() -> Catalog {
    Catalog::from_index(r#"{"families":[{"name":"Montserrat","category":"Sans Serif","variants":[{"weight":400,"italic":false}],"features":{"cap":0.7,"x":0.5,"stem":0.1,"widths":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}}]}"#, fontdb::Database::new())
}

#[test]
fn one_path_per_line_in_the_ink_colour_with_hidden_text_when_asked() {
    let (dir, font) = cache_with_fixture_font();
    let cat = catalog();
    let with = text_group(&plan(&font, true, Status::Matched), true, &cat, &dir);
    assert!(with.starts_with("<g id=\"text\">") && with.ends_with("</g>"), "{with}");
    assert_eq!(with.matches("<path ").count(), 1);
    assert!(with.contains("fill=\"#181c2c\"") && with.contains("data-line=\"img-t0\""));
    assert!(with.contains("<text x=\"10\" y=\"60\" font-family=\"Montserrat\" font-size=\"40\" letter-spacing=\"2\" fill=\"none\" pointer-events=\"none\" data-line=\"img-t0\">EST. 2026</text>"), "{with}");
    let without = text_group(&plan(&font, true, Status::Matched), false, &cat, &dir);
    assert!(!without.contains("<text"));
    assert_eq!(text_group(&plan(&font, false, Status::Off), true, &cat, &dir), "<g id=\"text\"></g>");
    // the path starts near the fit: an M within the line's box
    let d = with.split("d=\"").nth(1).unwrap();
    let m: Vec<f32> = d[1..].split(|c: char| !c.is_ascii_digit() && c != '.' && c != '-').take(2).map(|v| v.parse().unwrap()).collect();
    assert!(m[0] >= 10.0 && m[0] < 60.0 && m[1] > 20.0 && m[1] <= 60.5, "{m:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_core_renders_the_paths_and_nothing_for_the_text() {
    let (dir, font) = cache_with_fixture_font();
    let cat = catalog();
    let svg = |group: &str| format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 240 80\"><rect width=\"240\" height=\"80\" fill=\"#fff\"/>{group}</svg>");
    let with = studi0trace_core::render::render(&svg(&text_group(&plan(&font, true, Status::Matched), true, &cat, &dir)), 240, 80, false).unwrap();
    let without = studi0trace_core::render::render(&svg(&text_group(&plan(&font, true, Status::Matched), false, &cat, &dir)), 240, 80, false).unwrap();
    assert_eq!(with, without, "the hidden text draws nothing");
    let inked = with.chunks(4).filter(|p| p[0] < 128).count();
    assert!(inked > 200, "the glyphs are drawn: {inked} dark pixels");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn text_is_escaped() {
    assert_eq!(studi0trace_desktop::text::compose::escape_text("A & B <c> \"d\""), "A &amp; B &lt;c&gt; &quot;d&quot;");
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_compose 2>&1 | tail -3`
Expected: compile error.

- [ ] **Step 3: Implement `compose.rs`**

```rust
//! The text group composed over a trace: per matched, enabled line, the glyph outlines as one path in the
//! ink's colour, and an invisible <text> carrying the string for search and screen readers when the setting
//! asks. Source-pixel coordinates; the frontend inserts the group before </svg>.
use super::catalog::Catalog;
use super::fit::path_data;
use super::glyphs::Font;
use super::{Status, TextPlan};
use std::path::Path;

pub fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

pub fn text_group(plan: &TextPlan, hidden_text: bool, catalog: &Catalog, cache_dir: &Path) -> String {
    let mut out = String::from("<g id=\"text\">");
    for line in &plan.lines {
        if line.status != Status::Matched || !line.enabled {
            continue;
        }
        let (Some(fit), Some(chosen)) = (&line.fit, line.chosen.and_then(|i| line.candidates.get(i))) else { continue };
        let Some((path, index)) = catalog.locate(&chosen.font, cache_dir) else { continue };
        let Ok(font) = Font::load(&path, index) else { continue };
        let shaped = font.shape(&line.string);
        let d = path_data(&shaped, fit);
        let [r, g, b] = line.colour;
        out.push_str(&format!("<path d=\"{d}\" fill=\"#{r:02x}{g:02x}{b:02x}\" data-line=\"{}\"/>", line.id));
        if hidden_text {
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" letter-spacing=\"{}\" fill=\"none\" pointer-events=\"none\" data-line=\"{}\">{}</text>",
                num(fit.x), num(fit.baseline), escape_text(&chosen.font.family), num(fit.size), num(fit.tracking), line.id, escape_text(&line.string)
            ));
        }
    }
    out.push_str("</g>");
    out
}
```

- [ ] **Step 4: Implement the orchestration in `text/mod.rs`**

Append to `text/mod.rs` (after the types), and add `pub mod compose;`:

```rust
use crate::redraw::Source;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// What a plan needs, owned: the finder, the optional AI reader, the catalogue, the Google fetcher and its cache
/// folder, and whether the group carries the hidden text. Everything in it lives in the app state (or a test),
/// so a plan runs as a `'static` task and its pixel work goes to the blocking pool.
#[derive(Clone)]
pub struct Deps {
    pub finder: Arc<dyn reader::Finder>,
    pub reader: Option<Arc<dyn reader::Reader>>,
    pub catalog: Arc<catalog::Catalog>,
    pub fonts: Option<Arc<fonts::GoogleFonts>>,
    pub cache_dir: PathBuf,
    pub hidden_text: bool,
}

pub struct Planned {
    pub plan: TextPlan,
    pub inks: Vec<Arc<ink::LineInk>>,
    /// The source with the matched, enabled lines painted out, as PNG; None when no line is.
    pub painted: Option<Vec<u8>>,
    pub group: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct LineChange {
    pub string: Option<String>,
    pub font: Option<FontRef>,
    pub enabled: Option<bool>,
}

pub fn bytes_hash(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).into()
}

fn all_caps(s: &str) -> bool {
    s.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase())
}

/// A hint resolved against the catalogue: the Google family when the index has it, else the installed family.
fn resolve_hint(h: &reader::FontHint, catalog: &catalog::Catalog) -> Option<FontRef> {
    for source in [Source::Google, Source::Installed] {
        let weights = catalog.weights_of(&h.family, source);
        if !weights.is_empty() {
            let weight = weights.iter().copied().min_by_key(|w| (*w as i32 - h.weight as i32).abs()).unwrap();
            return Some(FontRef { family: h.family.clone(), weight, italic: h.italic, source });
        }
    }
    None
}

/// Every candidate fitted; sorted best first. `None` IoU for a font that could not be loaded is left out.
fn fit_candidates(string: &str, ink: &ink::LineInk, fonts: &[FontRef], catalog: &catalog::Catalog, cache_dir: &Path) -> Vec<(Candidate, Fit)> {
    let caps = all_caps(string);
    let mut out = Vec::new();
    for f in fonts {
        let Some((path, index)) = catalog.locate(f, cache_dir) else { continue };
        let Ok(font) = glyphs::Font::load(&path, index) else { continue };
        let shaped = font.shape(string);
        if let Some(fit) = fit::fit_line(&shaped, ink, caps) {
            out.push((Candidate { font: f.clone(), iou: fit.iou }, fit));
        }
    }
    out.sort_by(|a, b| b.0.iou.partial_cmp(&a.0.iou).unwrap());
    out
}

/// The weights beside the best family's, when the best is within 0.03 of the next.
fn weight_neighbours(best: &FontRef, next_iou: Option<f32>, best_iou: f32, catalog: &catalog::Catalog) -> Vec<FontRef> {
    if next_iou.is_none_or(|n| best_iou - n > 0.03) {
        return Vec::new();
    }
    catalog.weights_of(&best.family, best.source).into_iter().filter(|w| (*w as i32 - best.weight as i32).abs() == 100).map(|weight| FontRef { weight, ..best.clone() }).collect()
}

fn finish_line(line: &mut TextLine, fitted: Vec<(Candidate, Fit)>, any_located: bool) {
    if fitted.is_empty() {
        line.status = Status::Declined(if any_located { Reason::NoFit } else { Reason::Fonts });
        line.candidates.clear();
        line.chosen = None;
        line.fit = None;
        line.enabled = false;
        return;
    }
    let best = fitted[0].1.clone();
    line.candidates = fitted.into_iter().map(|(c, _)| c).collect();
    if best.iou >= FIT_ACCEPT {
        line.status = Status::Matched;
        line.chosen = Some(0);
        line.fit = Some(best);
        line.enabled = true;
    } else {
        line.status = Status::Declined(Reason::NoFit);
        line.chosen = None;
        line.fit = None;
        line.enabled = false;
    }
}

fn paint_and_group(source: &Source, plan: &TextPlan, inks: &[Arc<ink::LineInk>], hidden_text: bool, catalog: &catalog::Catalog, cache_dir: &Path) -> Result<(Option<Vec<u8>>, String), CommandError> {
    let used: Vec<&ink::LineInk> = plan.lines.iter().zip(inks).filter(|(l, _)| l.status == Status::Matched && l.enabled).map(|(_, i)| i.as_ref()).collect();
    let painted = if used.is_empty() {
        None
    } else {
        let rgba = ink::paint_out(source, &used);
        Some(crate::redraw::encode_png(&rgba, source.width, source.height, image::ExtendedColorType::Rgba8)?)
    };
    Ok((painted, compose::text_group(plan, hidden_text, catalog, cache_dir)))
}

pub async fn plan(image_id: &str, source: Arc<Source>, deps: Deps, phase: Arc<dyn Fn(Phase) + Send + Sync>) -> Result<Planned, CommandError> {
    phase(Phase::Finding);
    // the finder's work is done off the runtime's threads
    let lines = crate::redraw::blocking({
        let src = source.clone();
        let finder = deps.finder.clone();
        move || finder.find(&src)
    })
    .await?;
    let mut reader_kind = ReaderKind::Vision;
    let mut ai_error = None;
    let mut readings = reader::vision_readings(&lines);
    if let Some(reader) = deps.reader.clone() {
        phase(Phase::Reading);
        match reader.read(&source, &lines).await {
            Ok(r) => {
                readings = r;
                reader_kind = ReaderKind::Ai;
            }
            Err(e) => ai_error = Some(e.code().unwrap_or("bad_reply").to_string()),
        }
    }
    phase(Phase::Matching);
    // ink, decline, shortlist: pixels, off the runtime
    let (mut text_lines, inks, wanted) = {
        let src = source.clone();
        let catalog = deps.catalog.clone();
        let image = image_id.to_string();
        crate::redraw::blocking(move || {
            let mut text_lines = Vec::new();
            let mut inks = Vec::new();
            let mut wanted: Vec<Vec<FontRef>> = Vec::new();
            for (n, (line, reading)) in lines.iter().zip(&readings).enumerate() {
                let ink = ink::line_ink(&src, line.quad);
                let mut tl = TextLine { id: format!("{image}-t{n}"), quad: line.quad, string: reading.string.clone(), class: reading.class, colour: ink.colour, status: Status::Matched, candidates: Vec::new(), chosen: None, enabled: true, fit: None };
                let mut fonts = Vec::new();
                match ink::decline(&ink, &reading.string, reading.class, line.quad) {
                    Some(reason) => {
                        tl.status = Status::Declined(reason);
                        tl.enabled = false;
                    }
                    None => {
                        for h in &reading.hints {
                            if let Some(f) = resolve_hint(h, &catalog) {
                                if !fonts.contains(&f) {
                                    fonts.push(f);
                                }
                            }
                        }
                        let widths = catalog::measured_widths(&ink, &reading.string);
                        let stem = catalog::measured_stem(&ink);
                        for f in catalog.shortlist(&reading.string, &widths, stem, SHORTLIST) {
                            if !fonts.contains(&f) {
                                fonts.push(f);
                            }
                        }
                    }
                }
                text_lines.push(tl);
                inks.push(Arc::new(ink));
                wanted.push(fonts);
            }
            Ok((text_lines, inks, wanted))
        })
        .await?
    };
    // the Google ones downloaded, best effort
    let mut fonts_offline = false;
    if let Some(gf) = deps.fonts.clone() {
        let google: Vec<FontRef> = wanted.iter().flatten().filter(|f| f.source == Source::Google).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
        let ensured = gf.ensure(&google).await;
        fonts_offline = ensured.offline;
    }
    // the fits, off the runtime; a family that won a line leads the next line's list
    let planned = {
        let src = source.clone();
        let catalog = deps.catalog.clone();
        let cache_dir = deps.cache_dir.clone();
        let hidden = deps.hidden_text;
        let image = image_id.to_string();
        crate::redraw::blocking(move || {
            let mut winners: Vec<FontRef> = Vec::new();
            for (i, line) in text_lines.iter_mut().enumerate() {
                if line.status != Status::Matched {
                    continue;
                }
                let mut fonts = winners.clone();
                fonts.extend(wanted[i].iter().filter(|f| !winners.contains(f)).cloned());
                let any_located = fonts.iter().any(|f| catalog.locate(f, &cache_dir).is_some());
                let mut fitted = fit_candidates(&line.string, &inks[i], &fonts, &catalog, &cache_dir);
                if let Some((best, _)) = fitted.first() {
                    let next = fitted.get(1).map(|(c, _)| c.iou);
                    let more = weight_neighbours(&best.font, next, best.iou, &catalog);
                    if !more.is_empty() {
                        fitted.extend(fit_candidates(&line.string, &inks[i], &more, &catalog, &cache_dir));
                        fitted.sort_by(|a, b| b.0.iou.partial_cmp(&a.0.iou).unwrap());
                    }
                }
                finish_line(line, fitted, any_located);
                if line.status == Status::Matched {
                    let f = line.candidates[0].font.clone();
                    if !winners.contains(&f) {
                        winners.insert(0, f);
                    }
                }
            }
            let plan = TextPlan { image, reader: reader_kind, lines: text_lines, ai_error, fonts_offline };
            let (painted, group) = paint_and_group(&src, &plan, &inks, hidden, &catalog, &cache_dir)?;
            Ok(Planned { plan, inks, painted, group })
        })
        .await
    };
    match &planned {
        Ok(_) => phase(Phase::Done),
        Err(_) => phase(Phase::Failed),
    }
    planned
}

pub async fn set_line(source: Arc<Source>, planned: &Planned, line_id: &str, change: LineChange, deps: Deps) -> Result<Planned, CommandError> {
    let mut plan = planned.plan.clone();
    let inks = planned.inks.clone();
    let at = plan.lines.iter().position(|l| l.id == line_id).ok_or_else(|| CommandError::new(404, "no_line", "There is no such line of text."))?;
    if let Some(f) = &change.font {
        if f.source == Source::Google {
            if let Some(gf) = deps.fonts.clone() {
                plan.fonts_offline = gf.ensure(std::slice::from_ref(f)).await.offline;
            }
        }
    }
    let catalog = deps.catalog.clone();
    let cache_dir = deps.cache_dir.clone();
    let hidden = deps.hidden_text;
    crate::redraw::blocking(move || {
        let line = &mut plan.lines[at];
        let mut fonts: Vec<FontRef> = line.candidates.iter().map(|c| c.font.clone()).collect();
        let mut refit = false;
        if let Some(s) = change.string {
            let s = s.trim().to_string();
            if !s.is_empty() && s != line.string {
                line.string = s;
                refit = true;
            }
        }
        let asked_font = change.font.clone();
        if let Some(f) = change.font {
            if let Some(i) = fonts.iter().position(|c| *c == f) {
                fonts.swap(0, i);
            } else {
                fonts.insert(0, f);
            }
            refit = true;
        }
        if refit {
            let any_located = fonts.iter().any(|f| catalog.locate(f, &cache_dir).is_some());
            let fitted = fit_candidates(&line.string, &inks[at], &fonts, &catalog, &cache_dir);
            finish_line(line, fitted, any_located);
            // a font the user asked for stays chosen whatever its score
            if let Some(cf) = asked_font {
                if let Some(i) = line.candidates.iter().position(|c| c.font == cf) {
                    line.chosen = Some(i);
                    line.status = Status::Matched;
                    line.enabled = true;
                    line.fit = refit_one(&line.string, &inks[at], &cf, &catalog, &cache_dir);
                }
            }
        }
        if let Some(on) = change.enabled {
            match (on, line.chosen) {
                (true, Some(_)) => {
                    line.status = Status::Matched;
                    line.enabled = true;
                }
                (true, None) => {
                    // forcing a declined line on with no font: the best candidate if any
                    if !line.candidates.is_empty() {
                        line.chosen = Some(0);
                        let f = line.candidates[0].font.clone();
                        line.fit = refit_one(&line.string, &inks[at], &f, &catalog, &cache_dir);
                        line.status = Status::Matched;
                        line.enabled = true;
                    }
                }
                (false, _) => {
                    line.status = Status::Off;
                    line.enabled = false;
                }
            }
        }
        let (painted, group) = paint_and_group(&source, &plan, &inks, hidden, &catalog, &cache_dir)?;
        Ok(Planned { plan, inks, painted, group })
    })
    .await
}

fn refit_one(string: &str, ink: &ink::LineInk, font: &FontRef, catalog: &catalog::Catalog, cache_dir: &Path) -> Option<Fit> {
    fit_candidates(string, ink, std::slice::from_ref(font), catalog, cache_dir).into_iter().next().map(|(_, f)| f)
}

pub struct Entry {
    pub planned: Arc<Planned>,
    pub hash: [u8; 32],
}

struct Running {
    generation: u64,
    abort: Option<Box<dyn Fn() + Send + Sync>>,
}

#[derive(Default)]
pub struct Plans {
    entries: Mutex<HashMap<String, Entry>>,
    running: Mutex<HashMap<String, Running>>,
}

impl Plans {
    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn get(&self, image: &str, hash: &[u8; 32]) -> Option<Arc<Planned>> {
        Self::lock(&self.entries).get(image).filter(|e| &e.hash == hash).map(|e| e.planned.clone())
    }

    pub fn put(&self, image: &str, hash: [u8; 32], planned: Planned) -> Arc<Planned> {
        let p = Arc::new(planned);
        Self::lock(&self.entries).insert(image.to_string(), Entry { planned: p.clone(), hash });
        p
    }

    pub fn forget(&self, image: &str) {
        Self::lock(&self.entries).remove(image);
        if let Some(r) = Self::lock(&self.running).remove(image) {
            if let Some(a) = r.abort {
                a();
            }
        }
    }

    pub fn for_trace(&self, image: &str, hash: &[u8; 32]) -> Option<(Arc<Vec<u8>>, String)> {
        let entries = Self::lock(&self.entries);
        let e = entries.get(image).filter(|e| &e.hash == hash)?;
        e.planned.painted.as_ref().map(|p| (Arc::new(p.clone()), e.planned.group.clone()))
    }

    pub fn reserve(&self, image: &str) -> u64 {
        let mut running = Self::lock(&self.running);
        let next = running.get(image).map(|r| r.generation + 1).unwrap_or(1);
        if let Some(old) = running.insert(image.to_string(), Running { generation: next, abort: None }) {
            if let Some(a) = old.abort {
                a();
            }
        }
        next
    }

    pub fn is_current(&self, image: &str, generation: u64) -> bool {
        Self::lock(&self.running).get(image).is_some_and(|r| r.generation == generation)
    }

    pub fn arm(&self, image: &str, generation: u64, abort: Box<dyn Fn() + Send + Sync>) {
        if let Some(r) = Self::lock(&self.running).get_mut(image).filter(|r| r.generation == generation) {
            r.abort = Some(abort);
        }
    }

    pub fn cancel(&self, image: &str) -> bool {
        match Self::lock(&self.running).remove(image) {
            Some(r) => {
                if let Some(a) = r.abort {
                    a();
                }
                true
            }
            None => false,
        }
    }
}
```

`for_trace` clones the painted PNG into an `Arc` each call; store `painted: Option<Arc<Vec<u8>>>` in `Planned` instead if the clone shows in profiling — it is a few megabytes once per trace.

- [ ] **Step 5: Add an orchestration test on the fixture set**

In `tests/text_compose.rs` add:

```rust
struct FixtureFinder(Vec<studi0trace_desktop::text::reader::LineBox>);
impl studi0trace_desktop::text::reader::Finder for FixtureFinder {
    fn find(&self, _: &studi0trace_desktop::redraw::Source) -> Result<Vec<studi0trace_desktop::text::reader::LineBox>, studi0trace_desktop::error::CommandError> {
        Ok(self.0.clone())
    }
}

#[test]
fn plans_a_fixture_line_end_to_end_and_an_edit_recomposes() {
    use studi0trace_desktop::redraw::Source as Img;
    use studi0trace_desktop::text::reader::LineBox;
    use studi0trace_desktop::text::{plan, set_line, Deps, LineChange, Phase};
    use std::sync::{Arc, Mutex};
    let (dir, font) = cache_with_fixture_font();
    let img = Arc::new(Img::decode(&std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/montserrat-est-30-t0-white.png")).unwrap()).unwrap());
    let (w, h) = (img.width as f32, img.height as f32);
    let finder = FixtureFinder(vec![LineBox { quad: [[20.0, 14.0], [w - 20.0, 14.0], [w - 20.0, 58.0_f32.min(h)], [20.0, 58.0_f32.min(h)]], string: "EST. 2026".into(), confidence: 0.9 }]);
    let phases: Arc<Mutex<Vec<Phase>>> = Arc::default();
    let seen = phases.clone();
    let deps = Deps { finder: Arc::new(finder), reader: None, catalog: Arc::new(catalog()), fonts: None, cache_dir: dir.clone(), hidden_text: true };
    let planned = tauri::async_runtime::block_on(plan("img", img.clone(), deps.clone(), Arc::new(move |p| seen.lock().unwrap().push(p)))).unwrap();
    assert_eq!(*phases.lock().unwrap(), vec![Phase::Finding, Phase::Matching, Phase::Done]);
    let line = &planned.plan.lines[0];
    assert_eq!(line.status, Status::Matched, "{:?}", line);
    assert_eq!(line.candidates[0].font, font);
    assert!(line.fit.as_ref().unwrap().iou > 0.85);
    assert!(planned.painted.is_some() && planned.group.contains("<path "));
    // painted out: the ink pixels are gone
    let painted = Img::decode(planned.painted.as_ref().unwrap()).unwrap();
    assert!(painted.rgba.chunks(4).all(|p| p[0] > 200), "no dark pixel remains");
    // an edit: the string changes and the group follows
    let edited = tauri::async_runtime::block_on(set_line(img.clone(), &planned, "img-t0", LineChange { string: Some("EST. 2027".into()), ..Default::default() }, deps.clone())).unwrap();
    assert!(edited.group.contains(">EST. 2027</text>"));
    let off = tauri::async_runtime::block_on(set_line(img, &edited, "img-t0", LineChange { enabled: Some(false), ..Default::default() }, deps)).unwrap();
    assert_eq!(off.plan.lines[0].status, Status::Off);
    assert!(off.painted.is_none() && off.group == "<g id=\"text\"></g>");
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 6: Run the tests**

Run: `cd apps/desktop/src-tauri && cargo test --release --test text_compose 2>&1 | tail -12`
Expected: 4 passed. (The end-to-end test's catalogue has one family with zero widths; the shortlist still returns it, and the fit scores it.)

- [ ] **Step 7: Commit**

```bash
git add apps/desktop/src-tauri/src apps/desktop/src-tauri/tests/text_compose.rs
git commit -m "text: the plan — find, read, ink, match, fit, paint out, compose — and edits that refit one line; Plans caches it per image"
```

---

### Task 10: Commands, state, settings, `vectorize` integration, boundary test

**Files:**
- Modify: `apps/desktop/src-tauri/src/settings.rs` (fields, `from_value`, defaults test), `apps/desktop/src-tauri/src/lib.rs` (state, registration), `apps/desktop/src-tauri/src/commands.rs` (commands, `vectorize`, `close_image`, settings window size), `apps/desktop/src-tauri/tests/redraw_boundary.rs`

**Interfaces:**
- Produces: commands `text_plan(image_id) -> TextPlan`, `text_cancel(image_id) -> bool`, `text_set_line(image_id, line_id, change: LineChange) -> LineAnswer { plan, text_group }` (camelCase `textGroup`), `text_search_fonts(query) -> Vec<FontRef>`; `vectorize`'s reply gains `"text_group": string | null`; event `text-phase`.
- `pub struct TextState { pub plans: text::Plans, pub catalog: Arc<text::catalog::Catalog>, pub fonts: Option<Arc<text::fonts::GoogleFonts>>, pub cache_dir: PathBuf }`, managed in `setup`.

- [ ] **Step 1: Settings — write the failing test change, then the fields**

In `settings.rs`'s `defaults_and_camel_case` test, the expected JSON becomes:

```rust
serde_json::json!({"appearance": "system", "exportTo": "ask", "revealAfterExport": false, "traceOnOpen": false, "liveUpdate": true, "checkForUpdates": true, "redrawModel": "gpt-image-2", "redrawQuality": "medium", "suggestRedraw": true, "recogniseText": true, "readTextWithAi": false, "textModel": "gpt-5-mini", "hiddenText": true, "recent": []})
```

Run `cargo test --release --lib settings` → fails. Then add to `Settings` after `suggest_redraw`:

```rust
    /// Text re-setting (text/): whether lines of text are recognised and re-set on every trace, whether the AI
    /// reader runs (needs the stored key), its model, and whether exports carry the invisible <text>.
    pub recognise_text: bool,
    pub read_text_with_ai: bool,
    pub text_model: String,
    pub hidden_text: bool,
```

defaults `true, false, "gpt-5-mini".into(), true`; in `from_value`: `take(value, "recogniseText", &mut s.recognise_text); take(value, "readTextWithAi", &mut s.read_text_with_ai); take(value, "textModel", &mut s.text_model); take(value, "hiddenText", &mut s.hidden_text);`. Run the test again → passes.

- [ ] **Step 2: Boundary test — add the text assertions (failing first)**

In `tests/redraw_boundary.rs`:

```rust
#[test]
fn text_stays_out_of_the_worker_and_talks_to_the_network_from_two_files_only() {
    let worker = read("src/worker.rs");
    assert!(!worker.contains("text::") && !worker.contains("crate::text"), "worker.rs uses the text module");
    for entry in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/text")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(&path).unwrap();
        let may = name == "fonts.rs";
        assert!(may || !src.contains("reqwest"), "{name} mentions reqwest");
        assert!(name == "reader_openai.rs" || name == "fonts.rs" || !src.contains("openai"), "{name} mentions openai");
    }
}

#[test]
fn every_text_command_is_registered() {
    let lib = read("src/lib.rs");
    for cmd in ["text_plan", "text_cancel", "text_set_line", "text_search_fonts"] {
        assert!(lib.contains(&format!("commands::{cmd},")), "{cmd} is not in the handler list");
    }
}
```

(`fonts.rs` mentions `openai` only to borrow `is_loopback_base`; the assertion allows it.) Run → the registration test fails.

- [ ] **Step 3: State and commands**

`lib.rs`: add `pub mod text;` (done in Task 2); add after `AppState`:

```rust
/// Text re-setting's own state: the plans per image, the font catalogue (built once; fontdb scans the installed
/// fonts), the Google Fonts fetcher and its cache folder in the app's data directory.
pub struct TextState {
    pub plans: text::Plans,
    pub catalog: std::sync::Arc<text::catalog::Catalog>,
    pub fonts: Option<std::sync::Arc<text::fonts::GoogleFonts>>,
    pub cache_dir: std::path::PathBuf,
}
```

In `run()`'s `.setup(|app| { … })`, before the menu lines:

```rust
            let cache_dir = app.path().app_data_dir().map(|d| d.join("fonts")).unwrap_or_else(|_| std::env::temp_dir().join("studi0trace-fonts"));
            let fonts = text::fonts::GoogleFonts::new(cache_dir.clone(), &text::fonts::base_url()).ok().map(std::sync::Arc::new);
            app.manage(TextState { plans: text::Plans::default(), catalog: std::sync::Arc::new(text::catalog::Catalog::load()), fonts, cache_dir });
```

and register `commands::text_plan, commands::text_cancel, commands::text_set_line, commands::text_search_fonts,` after `commands::revert_redraw,`.

`commands.rs`:

```rust
use crate::text::{self, LineChange, TextPlan};
use crate::TextState;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineAnswer {
    pub plan: TextPlan,
    pub text_group: String,
}

fn text_deps(texts: &TextState, settings: &crate::settings::Settings, reader: Option<Arc<dyn text::reader::Reader>>) -> text::Deps {
    text::Deps { finder: Arc::new(text::vision::Vision), reader, catalog: texts.catalog.clone(), fonts: texts.fonts.clone(), cache_dir: texts.cache_dir.clone(), hidden_text: settings.hidden_text }
}

/// Finds, reads and fits the text of an image; the plan is kept for the image's bytes and answered again from
/// the cache. With Read text with AI on, the key is read first; without one the Vision reading stands and the
/// plan says `no_key`.
#[tauri::command]
pub async fn text_plan(app: tauri::AppHandle, state: State<'_, AppState>, texts: State<'_, TextState>, image_id: String) -> Result<TextPlan, CommandError> {
    use tauri::Emitter;
    let bytes = state.images.bytes(&image_id).ok_or_else(CommandError::expired)?;
    let hash = text::bytes_hash(&bytes);
    if let Some(p) = texts.plans.get(&image_id, &hash) {
        return Ok(p.plan.clone());
    }
    let settings = settings::load(&app);
    let generation = texts.plans.reserve(&image_id);
    let source = Arc::new(redraw::blocking(move || Source::decode(&bytes)).await?);
    let mut ai_error = None;
    let reader: Option<Arc<dyn text::reader::Reader>> = if settings.read_text_with_ai {
        match redraw::blocking(keychain::read_key).await {
            Ok(Some(key)) => text::reader_openai::AiReader::new(&redraw::openai::base_url(), &key, &settings.text_model).ok().map(|r| Arc::new(r) as Arc<dyn text::reader::Reader>),
            Ok(None) => {
                ai_error = Some("no_key".to_string());
                None
            }
            Err(e) => {
                ai_error = Some(e.code().unwrap_or("keychain").to_string());
                None
            }
        }
    } else {
        None
    };
    if !texts.plans.is_current(&image_id, generation) {
        return Err(redraw::error("cancelled"));
    }
    let (emitter, id) = (app.clone(), image_id.clone());
    let phase: Arc<dyn Fn(text::Phase) + Send + Sync> = Arc::new(move |p| {
        let _ = emitter.emit("text-phase", serde_json::json!({ "id": id, "phase": p }));
    });
    let deps = text_deps(&texts, &settings, reader);
    let task = tauri::async_runtime::spawn({
        let (source, image_id) = (source.clone(), image_id.clone());
        async move { text::plan(&image_id, source, deps, phase).await }
    });
    texts.plans.arm(&image_id, generation, Box::new({
        let h = task.abort_handle();
        move || h.abort()
    }));
    let mut planned = match task.await {
        Ok(r) => r?,
        Err(e) => return Err(join_failure(e)),
    };
    if !texts.plans.is_current(&image_id, generation) {
        return Err(redraw::error("cancelled"));
    }
    if planned.plan.ai_error.is_none() {
        planned.plan.ai_error = ai_error;
    }
    let kept = texts.plans.put(&image_id, hash, planned);
    Ok(kept.plan.clone())
}
```

(`Deps` is owned — `Arc`s and a `PathBuf`, Task 9 — so the spawned task is `'static`.)

```rust
#[tauri::command]
pub fn text_cancel(texts: State<'_, TextState>, image_id: String) -> bool {
    texts.plans.cancel(&image_id)
}

#[tauri::command]
pub async fn text_set_line(app: tauri::AppHandle, state: State<'_, AppState>, texts: State<'_, TextState>, image_id: String, line_id: String, change: LineChange) -> Result<LineAnswer, CommandError> {
    let bytes = state.images.bytes(&image_id).ok_or_else(CommandError::expired)?;
    let hash = text::bytes_hash(&bytes);
    let planned = texts.plans.get(&image_id, &hash).ok_or_else(|| CommandError::new(409, "no_plan", "The text of this image has not been read yet."))?;
    let settings = settings::load(&app);
    let source = Arc::new(redraw::blocking(move || Source::decode(&bytes)).await?);
    let deps = text_deps(&texts, &settings, None);
    let next = text::set_line(source, &planned, &line_id, change, deps).await?;
    let kept = texts.plans.put(&image_id, hash, next);
    Ok(LineAnswer { plan: kept.plan.clone(), text_group: kept.group.clone() })
}

#[tauri::command]
pub fn text_search_fonts(texts: State<'_, TextState>, query: String) -> Vec<text::FontRef> {
    texts.catalog.search(&query, 20)
}
```

`vectorize` becomes:

```rust
#[tauri::command]
pub async fn vectorize(app: tauri::AppHandle, state: State<'_, AppState>, texts: State<'_, TextState>, image_id: String, parameters: Value, auto: bool, job: String) -> Result<Value, CommandError> {
    let original = state.images.bytes(&image_id).ok_or_else(CommandError::expired)?;
    // with text re-set, the worker traces the image with the text painted out, and the group rides on the answer
    let (bytes, group) = match texts.plans.for_trace(&image_id, &text::bytes_hash(&original)) {
        Some((painted, group)) => (painted, Some(group)),
        None => (original, None),
    };
    let (emitter, job_id) = (app.clone(), job.clone());
    let rx = state.queue.submit(JobSpec { id: job, image_id, bytes, parameters, auto, test: None }, move || {
        let _ = emitter.emit("trace-phase", serde_json::json!({ "job": job_id, "phase": "tracing" }));
    });
    let answer = tauri::async_runtime::spawn_blocking(move || rx.recv().unwrap_or_else(|_| Err(CommandError::cancelled()))).await.unwrap_or_else(|e| Err(CommandError::crashed(e)))?;
    let mut answer = answer;
    if let Value::Object(map) = &mut answer {
        map.insert("text_group".into(), group.map(Value::String).unwrap_or(Value::Null));
    }
    Ok(answer)
}
```

`close_image` → after `close(...)`, `texts.plans.forget(&id)` (add `texts: State<'_, TextState>` to its signature). In `show_settings_window`, `.inner_size(520.0, 900.0)` with the comment `// the AI redraw and Text groups made the sheet taller`.

- [ ] **Step 4: Build and run the whole crate's tests**

Run: `cd frontend && npm run build 2>&1 | tail -2 && cd ../apps/desktop/src-tauri && cargo test --release 2>&1 | grep -E "^test result|FAILED|panicked" | head -20`
Expected: every target passes, including `worker.rs`'s tests on the real binary (the worker is unchanged) and `redraw_boundary`. Fix what `cargo check` names: `State` extraction order does not matter to Tauri; `LineChange` must derive `Deserialize` (it does); `TextPlan` is `Serialize`.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri
git commit -m "text: the commands (plan, cancel, set line, search fonts), the state, four settings, the painted bytes into the trace and the group onto its answer"
```

### Task 11: Frontend platform — types, native wiring, web stubs, `textsvg.ts`, `text.ts` copy

**Files:**
- Modify: `frontend/src/platform/types.ts`, `frontend/src/platform/native.ts`, `frontend/src/platform/web.ts`, `frontend/src/lib/api.ts:105-114`, `frontend/src/lib/redraw.ts` (export `ERROR_WORDS`)
- Create: `frontend/src/lib/textsvg.ts`, `frontend/src/lib/textsvg.test.ts`, `frontend/src/lib/text.ts`, `frontend/src/lib/text.test.ts`
- Test: `frontend/src/platform/native.test.ts` (one case)

**Interfaces:**
- Produces (`types.ts`):

```ts
export type TextSource = "google" | "installed";
export interface FontRef { family: string; weight: number; italic: boolean; source: TextSource }
export type TextClass = "type" | "script" | "custom";
export type DeclineReason = "script" | "custom" | "gradient" | "mismatch" | "no_fit" | "rotated" | "fonts";
export type TextStatus = { kind: "matched" } | { kind: "declined"; reason: DeclineReason } | { kind: "off" };
export interface TextFit { size: number; baseline: number; x: number; tracking: number; nudges: number[]; iou: number }
export interface TextCandidate { font: FontRef; iou: number }
export interface TextLine { id: string; quad: [number, number][]; string: string; class: TextClass; colour: [number, number, number]; status: TextStatus; candidates: TextCandidate[]; chosen: number | null; enabled: boolean; fit: TextFit | null }
export type TextPhase = "finding" | "reading" | "matching" | "done" | "failed";
export interface TextPlan { image: string; reader: "vision" | "ai"; lines: TextLine[]; aiError: string | null; fontsOffline: boolean }
export interface TextLineChange { string?: string; font?: FontRef; enabled?: boolean }
export interface TextOptions { signal: AbortSignal; onPhase?: (phase: TextPhase) => void }
// Settings gains: recogniseText: boolean; readTextWithAi: boolean; textModel: string; hiddenText: boolean  (defaults true, false, "gpt-5-mini", true)
// Platform gains:
//   textPlan(id: string, opts: TextOptions): Promise<TextPlan>;
//   textSetLine(id: string, lineId: string, change: TextLineChange): Promise<{ plan: TextPlan; textGroup: string }>;
//   textSearchFonts(query: string): Promise<FontRef[]>;
```

- `lib/api.ts`: `VectorizeResponse` gains `text_group?: string | null;`.
- `lib/textsvg.ts`: `export function composeSvg(base: string, group: string | null | undefined): string` — the group inserted before the last `</svg>`; `base` unchanged when the group is empty/null or there is no `</svg>`.
- `lib/text.ts`: `TEXT_PHASE_TEXT: Record<TextPhase, string>`, `declineText(reason: DeclineReason): string`, `textErrorText(code: string): string`, `fitPercent(iou: number): string` ("94 %", floored), `fontLabel(f: FontRef): string` ("Josefin Sans 300", "Josefin Sans 300 Italic").

- [ ] **Step 1: Write the failing tests**

`frontend/src/lib/textsvg.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import { composeSvg } from "./textsvg";

const base = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><defs></defs><path d="M0 0h10v10z" fill="#fff"/></svg>';
const group = '<g id="text"><path d="M1 1h2" fill="#000" data-line="a"/></g>';

describe("composeSvg", () => {
  it("inserts the group before the closing tag", () => {
    expect(composeSvg(base, group)).toBe(base.replace("</svg>", `${group}</svg>`));
  });
  it("keeps the group a sibling of an upsample root group", () => {
    const up = '<svg viewBox="0 0 10 10"><g transform="scale(0.5)"><path d="M0 0h20v20z"/></g></svg>';
    const out = composeSvg(up, group);
    expect(out).toBe(`<svg viewBox="0 0 10 10"><g transform="scale(0.5)"><path d="M0 0h20v20z"/></g>${group}</svg>`);
  });
  it("leaves the base alone without a group or a closing tag", () => {
    expect(composeSvg(base, null)).toBe(base);
    expect(composeSvg(base, "")).toBe(base);
    expect(composeSvg("<svg>", group)).toBe("<svg>");
  });
});
```

`frontend/src/lib/text.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import { declineText, fitPercent, fontLabel, TEXT_PHASE_TEXT, textErrorText } from "./text";

describe("text copy", () => {
  it("names every phase and every reason", () => {
    for (const p of ["finding", "reading", "matching"] as const) expect(TEXT_PHASE_TEXT[p]).toBeTruthy();
    expect(declineText("script")).toBe("Traced as shapes — script lettering");
    expect(declineText("no_fit")).toBe("Traced as shapes — no font fits");
    expect(declineText("gradient")).toBe("Traced as shapes — gradient fill");
    expect(declineText("mismatch")).toBe("Traced as shapes — the text and the pixels disagree");
    expect(declineText("fonts")).toBe("Traced as shapes — no font available");
    expect(declineText("rotated")).toBe("Traced as shapes — rotated text");
    expect(declineText("custom")).toBe("Traced as shapes — custom lettering");
  });
  it("floors the fit percentage and labels fonts", () => {
    expect(fitPercent(0.946)).toBe("94 %");
    expect(fontLabel({ family: "Josefin Sans", weight: 300, italic: false, source: "google" })).toBe("Josefin Sans 300");
    expect(fontLabel({ family: "Lora", weight: 400, italic: true, source: "installed" })).toBe("Lora 400 Italic");
  });
  it("uses redraw's words for the AI codes and its own for the rest", () => {
    expect(textErrorText("no_key")).toBe("Add your OpenAI API key in Settings ▸ Text to read text with AI.");
    expect(textErrorText("quota")).toBe("Your OpenAI account is out of credit or rate limited.");
    expect(textErrorText("text_unavailable")).toBe("Text recognition needs macOS 10.15 or later.");
    expect(textErrorText("whatever")).toBe("Reading the text failed.");
  });
});
```

In `frontend/src/platform/native.test.ts` add, following the file's `mockIPC` pattern:

```ts
it("textPlan listens for text-phase before invoking and cancels on abort", async () => {
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(cmd);
    if (cmd === "text_plan") {
      void emit("text-phase", { id: (args as { imageId: string }).imageId, phase: "finding" });
      return { image: "i1", reader: "vision", lines: [], aiError: null, fontsOffline: false };
    }
    if (cmd === "text_cancel") return true;
    return undefined;
  });
  const phases: string[] = [];
  const plan = await nativePlatform().textPlan("i1", { signal: new AbortController().signal, onPhase: (p) => phases.push(p) });
  expect(plan.lines).toEqual([]);
  expect(phases).toEqual(["finding"]);
  const aborted = new AbortController();
  const p = nativePlatform().textPlan("i1", { signal: aborted.signal });
  aborted.abort();
  await expect(p).rejects.toMatchObject({ code: "cancelled" });
  expect(calls).toContain("text_cancel");
});
```

Run: `cd frontend && npm run test:run -- src/lib/textsvg.test.ts src/lib/text.test.ts src/platform/native.test.ts 2>&1 | tail -6` → fails (modules missing).

- [ ] **Step 2: Implement**

`types.ts`: the types above; `Settings` and `DEFAULT_SETTINGS` gain the four fields; `Platform` gains the three methods with doc comments ("Text re-setting (the Mac app; a browser answers "not available")…").

`lib/api.ts`: `text_group?: string | null;` in `VectorizeResponse`.

`lib/textsvg.ts`:

```ts
/** The trace's SVG with the text group inside it: before the closing tag, a sibling of everything else (an upsampled trace's own root group included). */
export function composeSvg(base: string, group: string | null | undefined): string {
  if (!group) return base;
  const at = base.lastIndexOf("</svg>");
  if (at < 0) return base;
  return base.slice(0, at) + group + base.slice(at);
}
```

`lib/text.ts`:

```ts
/** Text re-setting's words: the phases, the reasons a line is traced as shapes, the failures. */
import type { DeclineReason, FontRef, TextPhase } from "@/platform/types";
import { ERROR_WORDS } from "./redraw";

export const TEXT_PHASE_TEXT: Record<TextPhase, string> = {
  finding: "Finding text",
  reading: "Reading with AI",
  matching: "Matching fonts",
  done: "",
  failed: "",
};

const REASONS: Record<DeclineReason, string> = {
  script: "script lettering",
  custom: "custom lettering",
  gradient: "gradient fill",
  mismatch: "the text and the pixels disagree",
  no_fit: "no font fits",
  rotated: "rotated text",
  fonts: "no font available",
};

export const declineText = (reason: DeclineReason) => `Traced as shapes — ${REASONS[reason]}`;

/** Floored, as the drift check floors: 0.946 reads 94 %, never 95. */
export const fitPercent = (iou: number) => `${Math.floor(iou * 100 + 1e-9)} %`;

export const fontLabel = (f: FontRef) => `${f.family} ${f.weight}${f.italic ? " Italic" : ""}`;

const OWN: Record<string, string> = {
  no_key: "Add your OpenAI API key in Settings ▸ Text to read text with AI.",
  text_unavailable: "Text recognition needs macOS 10.15 or later.",
};

export function textErrorText(code: string): string {
  return OWN[code] ?? ERROR_WORDS[code] ?? "Reading the text failed.";
}
```

In `lib/redraw.ts` change `const ERROR_WORDS` to `export const ERROR_WORDS`.

`native.ts`, inside `nativePlatform()`'s returned object (after `revertRedraw`):

```ts
    async textPlan(id, { signal, onPhase }) {
      const cancelled = () => new ApiError("cancelled", "The text reading was cancelled", 499);
      if (signal.aborted) throw cancelled();
      const offPhase = await listening<{ id: string; phase: TextPhase }>("text-phase", (p) => {
        if (p.id === id && !signal.aborted) onPhase?.(p.phase);
      }).catch(() => () => {});
      const abort = () => void invoke("text_cancel", { imageId: id }).catch(() => {});
      try {
        if (signal.aborted) throw cancelled();
        signal.addEventListener("abort", abort, { once: true });
        const plan = await call<TextPlan>("text_plan", { imageId: id });
        if (signal.aborted) throw cancelled();
        return plan;
      } finally {
        offPhase();
        signal.removeEventListener("abort", abort);
      }
    },
    textSetLine: (id, lineId, change) => call<{ plan: TextPlan; textGroup: string }>("text_set_line", { imageId: id, lineId, change }),
    textSearchFonts: (query) => call<FontRef[]>("text_search_fonts", { query }),
```

(import `TextPhase`, `TextPlan`, `FontRef` from `./types`). The abort test expects a rejection: with `mockIPC`, `call("text_plan")` resolves at once, then `signal.aborted` is true and `cancelled()` is thrown — matching the redraw pattern.

`web.ts`: `const noText = () => Promise.reject(new ApiError("unavailable", "Text re-setting needs the Mac app"));` and `textPlan: noText, textSetLine: noText, textSearchFonts: async () => [],`. Settings defaults there read from `DEFAULT_SETTINGS`, so nothing else changes.

- [ ] **Step 3: Run the tests and the type check**

Run: `cd frontend && npm run test:run -- src/lib/textsvg.test.ts src/lib/text.test.ts src/platform/native.test.ts 2>&1 | tail -6 && npx tsc --noEmit -p . 2>&1 | tail -5`
Expected: all pass; `tsc` reports the places that build a `Platform` or a `Settings` by hand (test mocks in `App.*.test.tsx`, `library.test.ts`'s `fakePlatform`, `platform/web.ts`) — add the three methods to the full mocks (`vi.fn()`) and the four settings where a whole `Settings` is spelled out. `LibraryPlatform` keeps them optional (Task 12).

- [ ] **Step 4: Commit**

```bash
git add frontend/src
git commit -m "frontend: the text plan's types and platform calls, the SVG composition, and the words for phases, reasons and failures"
```

---

### Task 12: `library.ts` — the text state, the plan before the trace, composition, edits

**Files:**
- Modify: `frontend/src/state/library.ts`, `frontend/src/state/library.test.ts`

**Interfaces:**
- Produces:

```ts
export interface TextState { plan: TextPlan | null; phase: TextPhase | null; error: ApiError | null; asked: boolean }
export const NO_TEXT: TextState = { plan: null, phase: null, error: null, asked: false };
export const textOf = (item: Pick<ImageItem, "text">): TextState => item.text ?? NO_TEXT;
// ImageItem gains `text?: TextState`; TraceAnswer gains `base: string` (the SVG as traced) and `textGroup: string | null`; `svg` is composed.
// LibraryPlatform gains Partial<Pick<Platform, "textPlan" | "textSetLine">>.
// Library gains:
//   setTextLine(id: string, lineId: string, change: TextLineChange): Promise<void>;  // an `enabled` change drops the image's traces and traces again; a string/font change recomposes every trace
//   cancelText(id: string): void;
```

- [ ] **Step 1: Write the failing tests**

In `frontend/src/state/library.test.ts`, following the file's `fakePlatform()` and fake-timer setup, add:

```ts
const PLAN = (lines: TextLine[]): TextPlan => ({ image: "a", reader: "vision", lines, aiError: null, fontsOffline: false });
const LINE: TextLine = { id: "a-t0", quad: [[0, 0], [10, 0], [10, 5], [0, 5]], string: "EST", class: "type", colour: [0, 0, 0], status: { kind: "matched" }, candidates: [{ font: { family: "Montserrat", weight: 400, italic: false, source: "google" }, iou: 0.9 }], chosen: 0, enabled: true, fit: { size: 10, baseline: 5, x: 0, tracking: 0, nudges: [], iou: 0.9 } };
const GROUP = '<g id="text"><path d="M0 0h1" fill="#000" data-line="a-t0"/></g>';

describe("text", () => {
  it("asks for the plan before the first trace and composes the answer", async () => {
    const order: string[] = [];
    const platform = fakePlatform();
    platform.textPlan = vi.fn(async () => {
      order.push("plan");
      return PLAN([LINE]);
    });
    platform.vectorize = vi.fn(async () => {
      order.push("trace");
      return response({ svg: "<svg></svg>", text_group: GROUP });
    });
    const lib = createLibrary(platform, catalog());
    lib.setSettings({ ...DEFAULT_SETTINGS, recogniseText: true });
    lib.add([opened("a")]);
    lib.generate("a");
    await flush();
    expect(order).toEqual(["plan", "trace"]);
    const item = lib.selectedItem()!;
    expect(textOf(item).plan?.lines).toHaveLength(1);
    expect(shownAnswer(item)?.svg).toBe(`<svg>${GROUP}</svg>`);
    expect(shownAnswer(item)?.base).toBe("<svg></svg>");
  });

  it("does not ask when Recognise text is off, and traces the original", async () => {
    const platform = fakePlatform();
    platform.textPlan = vi.fn();
    const lib = createLibrary(platform, catalog());
    lib.setSettings({ ...DEFAULT_SETTINGS, recogniseText: false });
    lib.add([opened("a")]);
    lib.generate("a");
    await flush();
    expect(platform.textPlan).not.toHaveBeenCalled();
    expect(lib.selectedItem()!.shown).not.toBeNull();
  });

  it("a plan failure is kept on the item and the trace still runs", async () => {
    const platform = fakePlatform();
    platform.textPlan = vi.fn(async () => Promise.reject(new ApiError("text_unavailable", "no", 501)));
    const lib = createLibrary(platform, catalog());
    lib.add([opened("a")]);
    lib.generate("a");
    await flush();
    expect(textOf(lib.selectedItem()!).error?.code).toBe("text_unavailable");
    expect(lib.selectedItem()!.shown).not.toBeNull();
  });

  it("a string edit recomposes every trace without tracing; an enabled change traces again", async () => {
    const platform = fakePlatform();
    platform.textPlan = vi.fn(async () => PLAN([LINE]));
    platform.vectorize = vi.fn(async () => response({ svg: "<svg></svg>", text_group: GROUP }));
    const G2 = GROUP.replace("h1", "h2");
    platform.textSetLine = vi.fn(async (_id, _line, change) => ({ plan: PLAN([{ ...LINE, string: "EST.", enabled: change.enabled ?? true }]), textGroup: G2 }));
    const lib = createLibrary(platform, catalog());
    lib.add([opened("a")]);
    lib.generate("a");
    await flush();
    await lib.setTextLine("a", "a-t0", { string: "EST." });
    expect(platform.vectorize).toHaveBeenCalledTimes(1);
    expect(shownAnswer(lib.selectedItem()!)?.svg).toBe(`<svg>${G2}</svg>`);
    expect(textOf(lib.selectedItem()!).plan?.lines[0].string).toBe("EST.");
    await lib.setTextLine("a", "a-t0", { enabled: false });
    await flush();
    expect(platform.vectorize).toHaveBeenCalledTimes(2);
  });

  it("using a redraw forgets the plan so it is read again", async () => {
    const platform = fakePlatform();
    platform.textPlan = vi.fn(async () => PLAN([LINE]));
    platform.redrawImage = vi.fn(async () => ({ redraw: opened("a-r"), drift: { edgeF1: 1, deltaE: 0, verdict: "close" } }));
    platform.acceptRedraw = vi.fn(async () => ({ image: opened("a"), original: opened("a-original") }));
    const lib = createLibrary(platform, catalog());
    lib.add([opened("a")]);
    lib.generate("a");
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(1);
    await lib.redraw("a");
    await lib.acceptRedraw("a");
    lib.generate("a");
    await flush();
    expect(platform.textPlan).toHaveBeenCalledTimes(2);
  });
});
```

Use the file's own helpers for `opened`, `response`, `catalog` and `flush` (whatever they are named there — read the top of the file and match; `response({svg, text_group})` should build a `VectorizeResponse` whose `results.vexel.svg` is the svg and whose `text_group` is set).

Run: `cd frontend && npm run test:run -- src/state/library.test.ts 2>&1 | tail -6` → fails.

- [ ] **Step 2: Implement**

In `library.ts`:

- Imports: `composeSvg` from `@/lib/textsvg`; `TextLine`, `TextLineChange`, `TextPhase`, `TextPlan` from `@/platform/types`.
- `TraceAnswer` gains `base: string; textGroup: string | null;`.
- `ImageItem` gains `/** Text re-setting (the Mac app); absent until asked. */ text?: TextState;`, and the `TextState`, `NO_TEXT`, `textOf` definitions from the interface block.
- `LibraryPlatform`: `& Partial<Pick<Platform, "imageRoughness" | "redrawImage" | "acceptRedraw" | "discardRedraw" | "revertRedraw" | "textPlan" | "textSetLine">>`.
- `answerOf`: `return { svg: composeSvg(r.svg, res.text_group), base: r.svg, textGroup: res.text_group ?? null, elapsedMs: …, stats: … }`. In `finish`, Auto candidates: `traces[…] = { svg: composeSvg(c.svg, res.text_group), base: c.svg, textGroup: res.text_group ?? null, … }`.
- A `patchText(id, part: Partial<TextState>)` helper like `patchRedraw`.
- `ensureText(id): Promise<void>`:

```ts
  const texting = new Map<string, AbortController>();
  /** The plan, once per image bytes, before the first trace; a failure is kept and the trace goes on. */
  const ensureText = async (id: string) => {
    const item = find(id);
    if (!item || !settings.recogniseText || !platform.textPlan) return;
    const t = textOf(item);
    if (t.plan || t.asked) return;
    const controller = new AbortController();
    texting.set(id, controller);
    patchText(id, { asked: true, phase: "finding", error: null });
    try {
      const plan = await platform.textPlan(id, {
        signal: controller.signal,
        onPhase: (phase) => {
          if (texting.get(id) === controller && find(id) && phase !== "done" && phase !== "failed") patchText(id, { phase });
        },
      });
      if (texting.get(id) === controller && find(id)) patchText(id, { plan, phase: null });
    } catch (err) {
      if (texting.get(id) !== controller || !find(id)) return;
      const e = toApiError(err);
      patchText(id, { phase: null, error: e.code === "cancelled" ? null : e });
    } finally {
      if (texting.get(id) === controller) texting.delete(id);
    }
  };
```

- `generate(id)`: becomes `void ensureText(id).then(() => startTrace(id))` where `startTrace` is the current body (the `item.traces[key]` short-circuit stays in `generate` before `ensureText`, so a known answer shows without a plan call). `settle` calls `generate` as before.
- `setTextLine`:

```ts
    async setTextLine(id, lineId, change) {
      const item = find(id);
      if (!item || !platform.textSetLine) return;
      const answer = await platform.textSetLine(id, lineId, change);
      const now = find(id);
      if (!now) return;
      if (change.enabled !== undefined) {
        // what is painted out changed: every trace of this image is stale
        abortJob(now);
        clearTimer(id);
        patch(id, { traces: {}, shown: null, exported: null, auto: null, job: null, error: null, errorKey: null, text: { ...textOf(now), plan: answer.plan } });
        generate(id);
        return;
      }
      const traces = Object.fromEntries(Object.entries(now.traces).map(([k, t]) => [k, { ...t, svg: composeSvg(t.base, answer.textGroup), textGroup: answer.textGroup }]));
      patch(id, { traces, text: { ...textOf(now), plan: answer.plan } });
    },
    cancelText(id) {
      texting.get(id)?.abort();
      texting.delete(id);
      if (find(id)) patchText(id, { phase: null });
    },
```

- `acceptRedraw` and `revertRedraw`: add `text: undefined` to their `patch` calls (the bytes changed; the plan is asked again). `remove`/`clear`: abort any `texting` controller for the id(s).

- [ ] **Step 3: Run the tests and the type check**

Run: `cd frontend && npm run test:run -- src/state/library.test.ts 2>&1 | tail -6 && npx tsc --noEmit -p . 2>&1 | tail -3`
Expected: all pass, including the file's existing tests (a trace's `svg` with no `text_group` equals its `base`).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/state
git commit -m "frontend: the library asks for the text plan before the first trace, composes the group onto every answer, and refits or re-traces on an edit"
```

---

### Task 13: The Text section, the canvas mark, and the App wiring

**Files:**
- Create: `frontend/src/components/TextSection.tsx`, `frontend/src/components/TextSection.test.tsx`, `frontend/src/components/TextOverlay.tsx`
- Modify: `frontend/src/components/VectorizePanel.tsx` (a `text?: ReactNode` slot), `frontend/src/App.tsx`

**Interfaces:**
- Produces:

```tsx
export interface TextSectionProps {
  item: ImageItem;
  /** Settings ▸ Text ▸ Recognise text. */
  enabled: boolean;
  onChange: (lineId: string, change: TextLineChange) => void;
  onHover: (lineId: string | null) => void;
  onCancel: () => void;
  searchFonts: (query: string) => Promise<FontRef[]>;
}
export function TextSection(props: TextSectionProps): JSX.Element | null;
export function TextOverlay({ plan, hovered, width, height, scale }: { plan: TextPlan; hovered: string | null; width: number; height: number; scale: number }): JSX.Element | null;
```

- [ ] **Step 1: Write the failing tests**

`frontend/src/components/TextSection.test.tsx`:

```tsx
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { ImageItem } from "@/state/library";
import type { TextLine, TextPlan } from "@/platform/types";
import { TextSection } from "./TextSection";

const line = (over: Partial<TextLine> = {}): TextLine => ({ id: "a-t0", quad: [[0, 0], [10, 0], [10, 5], [0, 5]], string: "EST. 2026", class: "type", colour: [0, 0, 0], status: { kind: "matched" }, candidates: [{ font: { family: "Montserrat", weight: 400, italic: false, source: "google" }, iou: 0.946 }, { font: { family: "Lato", weight: 400, italic: false, source: "installed" }, iou: 0.8 }], chosen: 0, enabled: true, fit: { size: 10, baseline: 5, x: 0, tracking: 0, nudges: [], iou: 0.946 }, ...over });
const item = (plan: TextPlan | null, phase: "finding" | null = null, error: null | { code: string } = null): ImageItem =>
  ({ image: { id: "a", name: "a.png", path: null, width: 100, height: 50, format: "PNG", previewUrl: "" }, preset: "auto", params: {}, traces: {}, shown: null, exported: null, job: null, error: null, errorKey: null, auto: null, text: { plan, phase, error: error as never, asked: true } }) as ImageItem;
const plan = (lines: TextLine[], reader: "vision" | "ai" = "vision"): TextPlan => ({ image: "a", reader, lines, aiError: null, fontsOffline: false });
const props = () => ({ enabled: true, onChange: vi.fn(), onHover: vi.fn(), onCancel: vi.fn(), searchFonts: vi.fn(async () => []) });

describe("TextSection", () => {
  it("lists a matched line with its string, font, fit and switch", () => {
    const p = props();
    render(<TextSection item={item(plan([line()]))} {...p} />);
    expect(screen.getByRole("heading", { name: /Text/ })).toHaveTextContent("1 line");
    expect(screen.getByText("On-device")).toBeInTheDocument();
    expect(screen.getByDisplayValue("EST. 2026")).toBeInTheDocument();
    expect(screen.getByText("Fit 94 %")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /Re-set/ })).toBeChecked();
    expect(screen.getByRole("combobox", { name: /Font/ })).toHaveValue("0");
  });

  it("edits call onChange with the string, the font or the switch", () => {
    const p = props();
    render(<TextSection item={item(plan([line()]))} {...p} />);
    const input = screen.getByDisplayValue("EST. 2026");
    fireEvent.change(input, { target: { value: "EST. 2027" } });
    fireEvent.blur(input);
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { string: "EST. 2027" });
    fireEvent.change(screen.getByRole("combobox", { name: /Font/ }), { target: { value: "1" } });
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { font: { family: "Lato", weight: 400, italic: false, source: "installed" } });
    fireEvent.click(screen.getByRole("switch", { name: /Re-set/ }));
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { enabled: false });
  });

  it("says why a line is traced as shapes, with its switch off", () => {
    render(<TextSection item={item(plan([line({ status: { kind: "declined", reason: "script" }, enabled: false, chosen: null, fit: null })]))} {...props()} />);
    expect(screen.getByText("Traced as shapes — script lettering")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /Re-set/ })).not.toBeChecked();
  });

  it("hovering a row reports the line, leaving reports null", () => {
    const p = props();
    render(<TextSection item={item(plan([line()]))} {...p} />);
    const row = screen.getByRole("group", { name: /Line 1/ });
    fireEvent.mouseEnter(row);
    expect(p.onHover).toHaveBeenCalledWith("a-t0");
    fireEvent.mouseLeave(row);
    expect(p.onHover).toHaveBeenCalledWith(null);
  });

  it("shows the phase while reading, the empty state, the AI reader and its error, and nothing when off", () => {
    const p = props();
    const { rerender } = render(<TextSection item={item(null, "finding")} {...p} />);
    expect(screen.getByText("Finding text")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(p.onCancel).toHaveBeenCalled();
    rerender(<TextSection item={item(plan([]))} {...p} />);
    expect(screen.getByText("No text found")).toBeInTheDocument();
    rerender(<TextSection item={item({ ...plan([line()], "ai"), aiError: "quota" })} {...p} />);
    expect(screen.getByText("AI")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("out of credit");
    rerender(<TextSection item={item(plan([line()]))} {...p} enabled={false} />);
    expect(screen.queryByRole("heading", { name: /Text/ })).toBeNull();
  });

  it("searches fonts from the picker and picks a result", async () => {
    const p = props();
    p.searchFonts = vi.fn(async () => [{ family: "Josefin Sans", weight: 300, italic: false, source: "google" as const }]);
    render(<TextSection item={item(plan([line()]))} {...p} />);
    fireEvent.change(screen.getByRole("combobox", { name: /Font/ }), { target: { value: "search" } });
    const box = await screen.findByRole("textbox", { name: /Search fonts/ });
    fireEvent.change(box, { target: { value: "jos" } });
    const hit = await screen.findByRole("button", { name: "Josefin Sans 300" });
    fireEvent.click(hit);
    expect(p.onChange).toHaveBeenCalledWith("a-t0", { font: { family: "Josefin Sans", weight: 300, italic: false, source: "google" } });
  });
});
```

Run: `cd frontend && npm run test:run -- src/components/TextSection.test.tsx 2>&1 | tail -5` → fails.

- [ ] **Step 2: Implement `TextSection.tsx`**

```tsx
import { Type } from "lucide-react";
import { useEffect, useState } from "react";

import { declineText, fitPercent, fontLabel, TEXT_PHASE_TEXT, textErrorText } from "@/lib/text";
import type { FontRef, TextLine, TextLineChange } from "@/platform/types";
import { textOf, type ImageItem } from "@/state/library";
import { Switch } from "./Switch";

export interface TextSectionProps {
  item: ImageItem;
  /** Settings ▸ Text ▸ Recognise text. */
  enabled: boolean;
  onChange: (lineId: string, change: TextLineChange) => void;
  onHover: (lineId: string | null) => void;
  onCancel: () => void;
  searchFonts: (query: string) => Promise<FontRef[]>;
}

/** The font picker: the ranked candidates with their fit, then a search over Google Fonts and the installed families. */
function FontPicker({ line, onPick, searchFonts }: { line: TextLine; onPick: (font: FontRef) => void; searchFonts: (q: string) => Promise<FontRef[]> }) {
  const [searching, setSearching] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<FontRef[]>([]);
  useEffect(() => {
    if (!searching || query.trim().length < 2) {
      setHits([]);
      return;
    }
    let live = true;
    const t = setTimeout(() => {
      searchFonts(query).then((r) => live && setHits(r), () => live && setHits([]));
    }, 200);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [query, searching, searchFonts]);
  const value = searching ? "search" : String(line.chosen ?? "");
  return (
    <div className="space-y-1">
      <select
        aria-label="Font"
        className="mac-select h-7 w-full text-[12px]"
        value={value}
        onChange={(e) => {
          if (e.target.value === "search") {
            setSearching(true);
            return;
          }
          setSearching(false);
          const c = line.candidates[Number(e.target.value)];
          if (c) onPick(c.font);
        }}
      >
        {line.chosen === null && <option value="">Choose a font…</option>}
        {line.candidates.map((c, i) => (
          <option key={`${c.font.source}:${c.font.family}:${c.font.weight}:${c.font.italic}`} value={String(i)}>
            {fontLabel(c.font)} · {fitPercent(c.iou)}
          </option>
        ))}
        <option value="search">Search fonts…</option>
      </select>
      {searching && (
        <div className="space-y-1">
          <input aria-label="Search fonts" className="mac-input h-7 w-full text-[12px]" placeholder="Google Fonts or installed" value={query} onChange={(e) => setQuery(e.target.value)} autoFocus />
          {hits.length > 0 && (
            <ul className="max-h-40 overflow-y-auto rounded-md border bg-card">
              {hits.map((f) => (
                <li key={`${f.source}:${f.family}:${f.weight}`}>
                  <button
                    type="button"
                    className="mac-ghost h-7 w-full justify-start px-2 text-[12px]"
                    onClick={() => {
                      setSearching(false);
                      setQuery("");
                      onPick(f);
                    }}
                  >
                    {fontLabel(f)}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}

function LineRow({ line, n, onChange, onHover, searchFonts }: { line: TextLine; n: number; onChange: (c: TextLineChange) => void; onHover: (id: string | null) => void; searchFonts: (q: string) => Promise<FontRef[]> }) {
  const [draft, setDraft] = useState(line.string);
  useEffect(() => setDraft(line.string), [line.string]);
  const commit = () => {
    const s = draft.trim();
    if (s && s !== line.string) onChange({ string: s });
    else setDraft(line.string);
  };
  const declined = line.status.kind === "declined" ? declineText(line.status.reason) : null;
  return (
    <div role="group" aria-label={`Line ${n}`} className="space-y-1.5 rounded-lg px-2 py-2 hover:bg-muted/60" onMouseEnter={() => onHover(line.id)} onMouseLeave={() => onHover(null)}>
      <div className="flex items-center gap-2">
        <input
          aria-label={`Text of line ${n}`}
          className="mac-input h-7 min-w-0 flex-1 text-[12px]"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") setDraft(line.string);
          }}
        />
        <Switch aria-label={`Re-set line ${n} in a font`} checked={line.enabled} onCheckedChange={(on) => onChange({ enabled: on })} />
      </div>
      <FontPicker line={line} onPick={(font) => onChange({ font })} searchFonts={searchFonts} />
      <div className="flex items-center justify-between text-[11px] text-muted-foreground">
        {declined ? <span>{declined}</span> : line.status.kind === "off" ? <span>Traced as shapes</span> : <span className="mac-tint inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 font-medium">{line.fit ? `Fit ${fitPercent(line.fit.iou)}` : ""}</span>}
      </div>
    </div>
  );
}

/** The inspector's Text: the lines found in the image, each with its string, its font, how well it fits and whether it is re-set. */
export function TextSection({ item, enabled, onChange, onHover, onCancel, searchFonts }: TextSectionProps) {
  if (!enabled) return null;
  const t = textOf(item);
  const count = t.plan?.lines.length ?? 0;
  return (
    <section aria-labelledby="text-section-title" className="space-y-2">
      <div className="flex items-center justify-between">
        <h4 id="text-section-title" className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted-foreground">
          <Type className="h-3.5 w-3.5" aria-hidden="true" />
          Text{t.plan ? ` · ${count} ${count === 1 ? "line" : "lines"}` : ""}
        </h4>
        {t.plan && <span className="text-[11px] text-muted-foreground">{t.plan.reader === "ai" ? "AI" : "On-device"}</span>}
      </div>
      {t.phase && (
        <div className="space-y-1.5">
          <p className="flex items-start gap-2 text-[12px] leading-snug text-muted-foreground" aria-live="polite">
            <span className="mac-dot mt-[5px] shrink-0 animate-pulse" aria-hidden="true" />
            <span className="min-w-0">
              <span className="block text-foreground">Reading the text…</span>
              <span className="block text-[11px]">{TEXT_PHASE_TEXT[t.phase]}</span>
            </span>
          </p>
          <button type="button" className="mac-button h-8 w-full" onClick={onCancel}>
            Cancel
          </button>
        </div>
      )}
      {t.error && (
        <p role="alert" className="text-[12px] leading-snug text-destructive">
          {textErrorText(t.error.code)}
        </p>
      )}
      {t.plan?.aiError && (
        <p role="alert" className="text-[12px] leading-snug text-destructive">
          {textErrorText(t.plan.aiError)}
        </p>
      )}
      {t.plan?.fontsOffline && <p className="mac-tint rounded-lg px-2.5 py-1.5 text-[11px]">Google Fonts unreachable — installed fonts only</p>}
      {t.plan && count === 0 && !t.phase && <p className="text-[12px] text-muted-foreground">No text found</p>}
      {t.plan && count > 0 && (
        <div className="-mx-2 space-y-0.5">
          {t.plan.lines.map((l, i) => (
            <LineRow key={l.id} line={l} n={i + 1} onChange={(c) => onChange(l.id, c)} onHover={onHover} searchFonts={searchFonts} />
          ))}
        </div>
      )}
    </section>
  );
}
```

If `mac-select` / `mac-input` classes do not exist in `frontend/src/styles.css`, use the classes the app's existing inputs use (search `className="mac-` in `components/` and `styles.css`; `ParamControl.tsx` has a number input, `SettingsView.tsx`'s key field has a text input) and match them — never invent a new look.

`TextOverlay.tsx`:

```tsx
import type { TextPlan } from "@/platform/types";

/** The hovered line's box on the canvas, in the vector's coordinates: dashed in the accent, as the inspector marks a shape. */
export function TextOverlay({ plan, hovered, width, height, scale }: { plan: TextPlan; hovered: string | null; width: number; height: number; scale: number }) {
  const line = hovered ? plan.lines.find((l) => l.id === hovered) : undefined;
  if (!line) return null;
  const w = 1 / Math.max(scale, 0.01);
  const points = line.quad.map(([x, y]) => `${x},${y}`).join(" ");
  return (
    <svg viewBox={`0 0 ${width} ${height}`} width={width} height={height} className="pointer-events-none absolute left-0 top-0 overflow-visible" aria-hidden="true">
      <polygon points={points} fill="hsl(var(--brand-accent) / 0.08)" stroke="hsl(var(--brand-accent))" strokeWidth={w} strokeDasharray={`${4 * w} ${3 * w}`} />
    </svg>
  );
}
```

`VectorizePanel.tsx`: add `/** Text re-setting's section (the Mac app). */ text?: ReactNode;` to the props, destructure it, render it after `{redraw}` inside the same top `div`, and include it in the `(redrawChip || redraw || text)` conditions.

`App.tsx`:
- `const CAN_TEXT = platform.kind === "native";`
- `const [textHover, setTextHover] = useState<string | null>(null);`
- `text={CAN_TEXT ? <TextSection item={item} enabled={settings.recogniseText} onChange={(lineId, change) => void lib.setTextLine(item.image.id, lineId, change)} onHover={setTextHover} onCancel={() => lib.cancelText(item.image.id)} searchFonts={(q) => platform.textSearchFonts(q)} /> : undefined}` on the `VectorizePanel`.
- The Viewer's `marks`: `(scale) => (<>{layers.doc && <InspectorOverlay doc={layers.doc} state={layers.liveState} scale={scale} />}{item && textOf(item).plan && <TextOverlay plan={textOf(item).plan!} hovered={textHover} width={item.image.width} height={item.image.height} scale={scale} />}</>)`, passed whenever either is present.

- [ ] **Step 3: Run the tests and the build**

Run: `cd frontend && npm run test:run -- src/components/TextSection.test.tsx src/App 2>&1 | tail -8 && npm run build 2>&1 | tail -3`
Expected: TextSection's 6 pass; the App tests still pass (their platform mocks gained the three methods in Task 11); the build is clean. Check both themes by eye later in Task 15.

- [ ] **Step 4: Commit**

```bash
git add frontend/src
git commit -m "frontend: the Text section under AI redraw — string, font picker with search, fit, switch — and the hovered line's mark on the canvas"
```

---

### Task 14: Settings group, consent variant, privacy copy, README

**Files:**
- Modify: `frontend/src/components/SettingsView.tsx`, `frontend/src/components/SettingsView.test.tsx`, `frontend/src/components/RedrawConsentSheet.tsx`, `frontend/src/components/RedrawConsentSheet.test.tsx`, `frontend/src/lib/redraw.ts`, `frontend/src/lib/redraw.test.ts`, `frontend/src/App.tsx` (SettingsWindow), `README.md:76`, `docs/superpowers/plans/2026-10-06-ai-redraw.md` (the quoted sentence, if a test reads it — check with grep)

**Interfaces:**
- `SettingsViewProps` gains `text?: { keyStored: boolean | null; onNeedKey: () => void }` — present in the Mac app; when *Read text with AI* is switched on and `keyStored === false`, `onNeedKey()` is called and the setting is not changed.
- `RedrawConsentSheetProps` gains `variant?: "redraw" | "text"` (default `"redraw"`): the title ("Redraw with AI" / "Read text with AI") and the first paragraph (`CONSENT_UPLOAD` / `CONSENT_TEXT_UPLOAD`); the second paragraph (`CONSENT_CHANGES`) only for redraw.
- `lib/redraw.ts`: `PRIVACY_SENTENCE` = the Global Constraints' text; `CONSENT_TEXT_UPLOAD = "Each image with text is uploaded to OpenAI with your API key and billed to your OpenAI account; the reply is the text and a font name, and the image is never changed."`

- [ ] **Step 1: Write the failing tests**

`redraw.test.ts`: change the `PRIVACY_SENTENCE` expectation to the new sentence and add `expect(CONSENT_TEXT_UPLOAD).toContain("the image is never changed")`.

`SettingsView.test.tsx` (follow the file's render helper):

```tsx
it("has a Text group whose AI switch asks for a key when none is stored", () => {
  const onChange = vi.fn();
  const onNeedKey = vi.fn();
  render(<SettingsView settings={DEFAULT_SETTINGS} onChange={onChange} text={{ keyStored: false, onNeedKey }} />);
  expect(screen.getByRole("switch", { name: "Recognise text" })).toBeChecked();
  expect(screen.getByRole("switch", { name: "Hidden text in exports" })).toBeChecked();
  fireEvent.click(screen.getByRole("switch", { name: "Read text with AI" }));
  expect(onNeedKey).toHaveBeenCalled();
  expect(onChange).not.toHaveBeenCalled();
  fireEvent.change(screen.getByRole("textbox", { name: "Model" }), { target: { value: "gpt-5" } });
  expect(onChange).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, textModel: "gpt-5" });
});

it("turns Read text with AI on at once when a key is stored", () => {
  const onChange = vi.fn();
  render(<SettingsView settings={DEFAULT_SETTINGS} onChange={onChange} text={{ keyStored: true, onNeedKey: vi.fn() }} />);
  fireEvent.click(screen.getByRole("switch", { name: "Read text with AI" }));
  expect(onChange).toHaveBeenCalledWith({ ...DEFAULT_SETTINGS, readTextWithAi: true });
});

it("has no Text group in a browser", () => {
  render(<SettingsView settings={DEFAULT_SETTINGS} onChange={vi.fn()} />);
  expect(screen.queryByRole("switch", { name: "Recognise text" })).toBeNull();
});
```

`RedrawConsentSheet.test.tsx`: add

```tsx
it("has a text-reading variant", () => {
  render(<RedrawConsentSheet open variant="text" onOpenChange={vi.fn()} onSave={vi.fn()} />);
  expect(screen.getByRole("dialog", { name: "Read text with AI" })).toBeInTheDocument();
  expect(screen.getByText(/the image is never changed/)).toBeInTheDocument();
  expect(screen.queryByText(/may change shapes/)).toBeNull();
});
```

Run: `cd frontend && npm run test:run -- src/lib/redraw.test.ts src/components/SettingsView.test.tsx src/components/RedrawConsentSheet.test.tsx 2>&1 | tail -6` → fails.

- [ ] **Step 2: Implement**

`lib/redraw.ts`: the two constants. `RedrawConsentSheet.tsx`: the `variant` prop; `<Dialog.Title>{variant === "text" ? "Read text with AI" : "Redraw with AI"}</Dialog.Title>`; `<p>{variant === "text" ? CONSENT_TEXT_UPLOAD : CONSENT_UPLOAD}</p>{variant !== "text" && <p>{CONSENT_CHANGES}</p>}`.

`SettingsView.tsx`: after the AI redraw group:

```tsx
      {text && (
        <Group title="Text">
          <Row label="Recognise text" hint="Lines of text are set in a matching font and drawn as outlines">
            <Toggle label="Recognise text" checked={settings.recogniseText} onChange={(v) => set("recogniseText", v)} />
          </Row>
          <Row label="Read text with AI" hint="OpenAI reads each line and names its typeface, with your key">
            <Toggle
              label="Read text with AI"
              checked={settings.readTextWithAi}
              onChange={(v) => {
                if (v && text.keyStored === false) text.onNeedKey();
                else set("readTextWithAi", v);
              }}
            />
          </Row>
          <Row label="Model">
            <input aria-label="Model" className="mac-input h-7 w-40 text-[12px]" value={settings.textModel} onChange={(e) => set("textModel", e.target.value)} />
          </Row>
          <Row label="Hidden text in exports" hint="An invisible text layer, for search and screen readers">
            <Toggle label="Hidden text in exports" checked={settings.hiddenText} onChange={(v) => set("hiddenText", v)} />
          </Row>
        </Group>
      )}
```

`App.tsx`'s `SettingsWindow`:

```tsx
function SettingsWindow() {
  const { settings, change } = useSettings();
  const key = useRedrawKey();
  const [consent, setConsent] = useState(false);
  return (
    <div className="min-h-dvh bg-background">
      <SettingsView settings={settings} onChange={change} redraw={CAN_REDRAW ? { stored: key.stored, onSave: key.save, onRemove: key.remove } : undefined} text={CAN_TEXT ? { keyStored: key.stored, onNeedKey: () => setConsent(true) } : undefined} />
      {CAN_TEXT && (
        <RedrawConsentSheet
          open={consent}
          variant="text"
          onOpenChange={setConsent}
          onSave={async (k) => {
            await key.save(k);
            setConsent(false);
            change({ ...settings, readTextWithAi: true });
          }}
        />
      )}
    </div>
  );
}
```

README line 76: replace the two sentences from "Your images stay on your Mac" to the end of the paragraph with the Global Constraints' `PRIVACY_SENTENCE` followed by: "Those two and the fonts are the only things in the app that touch the network: a matched Google Font is downloaded from Google with no image attached, and the AI features are off until you turn them on." Add, just before that paragraph, a short **Text** paragraph: "**Small text comes out as type.** Lines of text are recognised on your Mac, matched to a Google Font or one installed on it, set in that font and drawn as outlines; the Text section lets you correct the words, pick another font or leave a line to the trace. Read text with AI (Settings ▸ Text) asks OpenAI for the words and the typeface, with your own key." Then `grep -rn "sends that one image" --include='*.rs' --include='*.ts' --include='*.tsx' --include='*.md' . | grep -v node_modules` and update every remaining copy of the old sentence that a test compares (the 2026-10-06 plan's quote is history; leave it).

- [ ] **Step 3: Run the frontend suite, the type check and the Rust notice test**

Run: `cd frontend && npm run test:run 2>&1 | tail -6 && npx tsc --noEmit -p . && cd ../apps/desktop/src-tauri && cargo test --release --test notices 2>&1 | tail -3`
Expected: all green (`notices.rs` holds the README's notices; if it compares the privacy sentence, its expectation is updated to the new one).

- [ ] **Step 4: Commit**

```bash
git add frontend/src README.md
git commit -m "settings: the Text group with the AI reader behind the consent sheet; the privacy sentence names both AI features and the font download"
```

---

### Task 15: Verification — the whole build, the example images, both themes

**Files:**
- Create: `apps/desktop/src-tauri/tests/text_examples.rs` (`#[ignore]`, driven by an env var; never reads the repo for images)
- Modify: `docs/superpowers/specs/2026-10-09-text-resetting-design.md` (a Status line with what was measured)

- [ ] **Step 1: The example harness**

```rust
//! By hand: `STUDI0TRACE_TEXT_EXAMPLES=<dir with 1.png 2.png 3.png> cargo test --release --test text_examples -- --ignored --nocapture`
//! plans each image with Vision and the real catalogue (Google Fonts fetched into <dir>/fonts), writes
//! <dir>/out/N-plan.json, N-painted.png and N-text.svg, and renders the text group over the painted image to
//! N-check.png for a look. Nothing of this is in git.
use std::path::PathBuf;
use std::sync::Arc;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::catalog::Catalog;
use studi0trace_desktop::text::fonts::GoogleFonts;
use studi0trace_desktop::text::vision::Vision;
use studi0trace_desktop::text::{plan, Deps};

#[test]
#[ignore]
fn plans_the_example_images() {
    let Ok(dir) = std::env::var("STUDI0TRACE_TEXT_EXAMPLES") else { return };
    let dir = PathBuf::from(dir);
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let cache = dir.join("fonts");
    let fonts = Arc::new(GoogleFonts::new(cache.clone(), &studi0trace_desktop::text::fonts::base_url()).unwrap());
    let catalog = Arc::new(Catalog::load());
    for n in ["1", "2", "3"] {
        let Ok(bytes) = std::fs::read(dir.join(format!("{n}.png"))) else { continue };
        let source = Arc::new(Source::decode(&bytes).unwrap());
        let deps = Deps { finder: Arc::new(Vision), reader: None, catalog: catalog.clone(), fonts: Some(fonts.clone()), cache_dir: cache.clone(), hidden_text: true };
        let t0 = std::time::Instant::now();
        let planned = tauri::async_runtime::block_on(plan(n, source.clone(), deps, Arc::new(|p| eprintln!("  {p:?}")))).unwrap();
        eprintln!("{n}: {} lines in {:.1}s", planned.plan.lines.len(), t0.elapsed().as_secs_f32());
        for l in &planned.plan.lines {
            eprintln!("  {:?} {:?} {:?} {:?}", l.string, l.status, l.candidates.first().map(|c| (&c.font.family, c.font.weight, c.iou)), l.fit.as_ref().map(|f| f.size));
        }
        std::fs::write(out.join(format!("{n}-plan.json")), serde_json::to_string_pretty(&planned.plan).unwrap()).unwrap();
        if let Some(p) = &planned.painted {
            std::fs::write(out.join(format!("{n}-painted.png")), p).unwrap();
        }
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>{}</svg>", source.width, source.height, planned.group);
        std::fs::write(out.join(format!("{n}-text.svg")), &svg).unwrap();
        let px = studi0trace_core::render::render(&svg, source.width, source.height, false).unwrap();
        let mut check = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(source.width, source.height, px).unwrap().write_to(&mut check, image::ImageFormat::Png).unwrap();
        std::fs::write(out.join(format!("{n}-check.png")), check.into_inner()).unwrap();
    }
}
```

(`Deps` in its owned form from Task 10; adjust if the implementation kept references.)

- [ ] **Step 2: Run it on Tim's images and look**

Run: `cd apps/desktop/src-tauri && STUDI0TRACE_TEXT_EXAMPLES=/private/tmp/claude-502/-Users-TimNice-Development-tracer--claude-worktrees-svg-text-handling-fbf2c8/33ac6b5f-d9f7-489b-ab30-1b0e7edbf73b/scratchpad/text cargo test --release --test text_examples -- --ignored --nocapture 2>&1 | grep -v "^test " | tail -40`
Then view `out/1-check.png`, `2-check.png`, `3-check.png` with the Read tool, and crop/zoom the taglines against the sources (the script `scratchpad/text/trace_examples.py` crops are a guide: image 1 tagline at (180,970)-(1080,1040); image 2 "LIFE IS BETTER OUTSIDE" at (460,580)-(1080,620), "EST. 2026" at (50,95)-(300,135); image 3 "IDEAS FOR A BRIGHTER TOMORROW" at (320,835)-(1220,870)).
Expected: every tagline matched (status `matched`, a plausible geometric sans at a light weight, IoU above FIT_ACCEPT), the scripts ("Awaits", "Creative", "Explore") and the big display words either matched well or declined, never wrongly re-set in a font that does not look like the source; the painted PNGs show no ghost of the text. Record per image in the spec's Status: lines found, matched, declined with reasons, the fonts picked, the run time.
If a tagline is declined as `mismatch` because Vision's string carries extra spaces, the segment count rule is right and the string is wrong: the AI reader is the fix at runtime; for the record, note it. If one is declined `no_fit` with the best IoU just under FIT_ACCEPT, look at `fit.rs`'s initial guess on that line before touching the constant.

- [ ] **Step 3: Full-trace check through the real worker**

Run `python3 - <<'EOF'` … no: use the app's own path. Run `cd apps/desktop && npm run smoke 2>&1 | tail -5` (the app launches, opens, traces). Then trace image 1 through the backend with the painted PNG to see the whole: `cd backend && VEXEL_BACKEND=rust .venv/bin/python -c "..."` is not needed — the composed result is `trace(painted) + group`; render `out/1-painted.png` through `scratchpad/text/trace_examples.py` (point it at the `out` dir with the painted PNG renamed `1.png`) and compose `1-text.svg`'s group into the SVG it writes, then render and view. Note what you see in the spec's Status.

- [ ] **Step 4: Everything green, both themes**

Run: `cd frontend && npm run test:run 2>&1 | tail -4 && npm run build 2>&1 | tail -2 && cd ../apps/desktop/src-tauri && cargo test --release 2>&1 | grep -E "^test result" && cargo clippy --release 2>&1 | grep -E "^(warning|error)" | sort | uniq -c | head`
Expected: every suite green; clippy adds no new warnings over `main`'s count (`git stash` is forbidden here — compare against `cargo clippy` on a fresh `git worktree add` of main if needed, or just read the warnings: none from `src/text/`).
Launch the app (`cd apps/desktop && npm run dev`), open an example image, confirm: the Text section appears under AI redraw with the lines, hovering marks the line, editing the string recomposes without a trace, the switch re-traces, Settings shows the Text group in light and dark appearance, and the consent sheet opens from *Read text with AI* when no key is stored. Take one screenshot per theme into the scratchpad and look at them.

- [ ] **Step 5: Commit, and write the Status**

Append to the spec a `## Status` section: date, what every suite reported, the three images' results, and anything deferred (with a reason). Commit:

```bash
git add apps/desktop/src-tauri/tests/text_examples.rs docs/superpowers/specs/2026-10-09-text-resetting-design.md
git commit -m "text: the example harness and the spec's status after the first build"
```

---

## Self-review

- **Spec coverage.** Flow 1 (find/read) → Tasks 7–8; 2 (ink, decline) → 3; 3 (match, hints, winners first) → 5, 9; 4 (fit, nudges, FIT_ACCEPT) → 4; 5 (paint out, cache by hash) → 3, 9, 10; 6 (compose, hidden text, frontend composition, Auto candidates, export) → 9, 11, 12; edits (refit vs re-trace) → 9, 12; Fonts (installed, index, download, offline chip, boundary) → 5, 6, 10, 13; AI reader (one call, schema, model setting, no key, errors never fail the plan) → 8, 10; Data/commands/events → 10, 11; UI (section, picker, chip, switch, declined copy, hover mark, empty state, hidden when off) → 13; Settings group, consent, window size, copy → 10, 14; Errors → 2, 11; Tests → each task; manual check → 15. Out of scope items need no task.
- **Placeholders.** None: every step has its code or its exact command. `Deps` is owned (`Arc`s and a `PathBuf`) everywhere it appears (Tasks 9, 10, 15).
- **Type consistency.** `Status` serialises as `{kind, reason}` on both sides; `TextPlan` fields `aiError`/`fontsOffline` camelCase on both; `LineAnswer.textGroup` camelCase; `VectorizeResponse.text_group` snake_case (it rides on the core's snake_case reply); `Deps`/`Planned`/`LineChange` names match between Tasks 9, 10 and 15; `FontRef` has the same four fields in Rust and TS; `fitPercent` floors as `edgesText` does.


