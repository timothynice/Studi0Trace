//! Text re-setting: the lines of text in an image are found and read (vision.rs, reader_openai.rs), the font is
//! matched (catalog.rs, fonts.rs) and fitted (glyphs.rs, ink.rs, fit.rs), the text is painted out of what the
//! trace sees (ink.rs) and the glyph outlines are composed on top (compose.rs). Nothing here is seen by the
//! engine, the core or the worker.
pub mod catalog;
pub mod compose;
pub mod fit;
pub mod fonts;
pub mod glyphs;
pub mod ink;
pub mod reader;
pub mod reader_openai;
pub mod vision;

use crate::error::CommandError;
use serde::{Deserialize, Serialize};

/// A line whose best fit scores under this is traced as shapes (set from the fixture set, tests/text_fit.rs).
pub const FIT_ACCEPT: f32 = 0.79;
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

use crate::redraw::Source as Image;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A line's early stop: once a candidate fits this well, the rest of the shortlist is not fitted.
const FIT_SURE: f32 = FIT_ACCEPT + 0.05;
/// Shortlist families (and the previous lines' winners) fitted before the early stop may apply.
const EAGER_FAMILIES: usize = 4;
/// Reader hints fitted per line.
const MAX_HINTS: usize = 3;
/// Previous lines' winning families that lead the next line's list.
const MAX_WINNERS: usize = EAGER_FAMILIES;

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
    /// The source with the matched, enabled lines painted out, as PNG; None when no line is. Shared, so a trace
    /// takes it without a copy.
    pub painted: Option<Arc<Vec<u8>>>,
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
        if let Some(weight) = weights.iter().copied().min_by_key(|w| (*w as i32 - h.weight as i32).abs()) {
            return Some(FontRef { family: h.family.clone(), weight, italic: h.italic, source });
        }
    }
    None
}

/// `fonts` fitted in order, best first. A full fit costs a quarter of a second, so the list is not run to the
/// end: the first `eager` fonts are always fitted (the reader's hints, the previous lines' winners, the top of
/// the shortlist, a font the user asked for), and after them each font is fitted only while the best IoU so far
/// is under `FIT_ACCEPT + 0.05`, stopping as soon as one reaches it. A font that is never fitted is not a
/// candidate; one that cannot be located or loaded is left out.
fn fit_candidates(string: &str, ink: &ink::LineInk, fonts: &[FontRef], eager: usize, catalog: &catalog::Catalog, cache_dir: &Path) -> Vec<(Candidate, Fit)> {
    let caps = all_caps(string);
    let mut out: Vec<(Candidate, Fit)> = Vec::new();
    let mut best = 0.0f32;
    for (n, f) in fonts.iter().enumerate() {
        if n >= eager && best >= FIT_SURE {
            break;
        }
        let Some((path, index)) = catalog.locate(f, cache_dir) else { continue };
        let Ok(font) = glyphs::Font::load(&path, index) else { continue };
        let shaped = font.shape(string);
        if let Some(fit) = fit::fit_line(&shaped, ink, caps) {
            best = best.max(fit.iou);
            out.push((Candidate { font: f.clone(), iou: fit.iou }, fit));
        }
    }
    sort_best_first(&mut out);
    out
}

fn sort_best_first(fitted: &mut [(Candidate, Fit)]) {
    fitted.sort_by(|a, b| b.0.iou.total_cmp(&a.0.iou));
}

/// The fit of the best family's neighbouring weights, when the best is within 0.03 of the next family's score.
fn with_weight_neighbours(string: &str, ink: &ink::LineInk, fitted: &mut Vec<(Candidate, Fit)>, catalog: &catalog::Catalog, cache_dir: &Path) {
    let Some((best, _)) = fitted.first() else { return };
    let next = fitted.get(1).map(|(c, _)| c.iou);
    if next.is_none_or(|n| best.iou - n > 0.03) {
        return;
    }
    let more: Vec<FontRef> = catalog
        .weights_of(&best.font.family, best.font.source)
        .into_iter()
        .filter(|w| (*w as i32 - best.font.weight as i32).abs() == 100)
        .map(|weight| FontRef { weight, ..best.font.clone() })
        .filter(|f| !fitted.iter().any(|(c, _)| c.font == *f))
        .collect();
    if !more.is_empty() {
        let n = more.len();
        fitted.extend(fit_candidates(string, ink, &more, n, catalog, cache_dir));
        sort_best_first(fitted);
    }
}

/// The line's result from its fitted candidates. A `forced` font (the user's choice) that was fitted is chosen
/// and the line matched and enabled whatever its score.
fn finish_line(line: &mut TextLine, fitted: Vec<(Candidate, Fit)>, any_located: bool, forced: Option<&FontRef>) {
    if fitted.is_empty() {
        line.status = Status::Declined(if any_located { Reason::NoFit } else { Reason::Fonts });
        line.candidates.clear();
        line.chosen = None;
        line.fit = None;
        line.enabled = false;
        return;
    }
    let at = forced.and_then(|f| fitted.iter().position(|(c, _)| c.font == *f));
    let pick = at.unwrap_or(0);
    let fit = fitted[pick].1.clone();
    line.candidates = fitted.into_iter().map(|(c, _)| c).collect();
    if at.is_some() || fit.iou >= FIT_ACCEPT {
        line.status = Status::Matched;
        line.chosen = Some(pick);
        line.fit = Some(fit);
        line.enabled = true;
    } else {
        line.status = Status::Declined(Reason::NoFit);
        line.chosen = None;
        line.fit = None;
        line.enabled = false;
    }
}

/// The painted PNG (when a line is matched and enabled) and the group.
type Painted = (Option<Arc<Vec<u8>>>, String);

fn paint_and_group(source: &Image, plan: &TextPlan, inks: &[Arc<ink::LineInk>], hidden_text: bool, catalog: &catalog::Catalog, cache_dir: &Path) -> Result<Painted, CommandError> {
    let used: Vec<&ink::LineInk> = plan.lines.iter().zip(inks).filter(|(l, _)| l.status == Status::Matched && l.enabled).map(|(_, i)| i.as_ref()).collect();
    let painted = if used.is_empty() {
        None
    } else {
        let rgba = ink::paint_out(source, &used);
        Some(Arc::new(crate::redraw::encode_png(&rgba, source.width, source.height, image::ExtendedColorType::Rgba8)?))
    };
    Ok((painted, compose::text_group(plan, hidden_text, catalog, cache_dir)))
}

/// The source on white with alpha 255: what Vision and the AI reader are shown, since light ink on a transparent
/// logo is invisible to them otherwise. The real alpha stays with `source` for the ink and the paint-out.
fn flattened(source: &Image) -> Image {
    let rgb = studi0trace_core::color::rgb_on_white(&source.rgba);
    let mut rgba = Vec::with_capacity(rgb.len() / 3 * 4);
    for p in rgb.as_chunks::<3>().0 {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    Image { rgba, width: source.width, height: source.height }
}

/// Finds, reads, matches and fits every line of `source`, paints the matched ones out and composes the group.
/// Reports each phase; ends with `Done`, or `Failed` when it returns an error.
pub async fn plan(image_id: &str, source: Arc<Image>, deps: Deps, phase: Arc<dyn Fn(Phase) + Send + Sync>) -> Result<Planned, CommandError> {
    let planned = run(image_id, source, deps, &phase).await;
    phase(if planned.is_ok() { Phase::Done } else { Phase::Failed });
    planned
}

async fn run(image_id: &str, source: Arc<Image>, deps: Deps, phase: &Arc<dyn Fn(Phase) + Send + Sync>) -> Result<Planned, CommandError> {
    phase(Phase::Finding);
    let (flat, lines) = {
        let src = source.clone();
        let finder = deps.finder.clone();
        crate::redraw::blocking(move || {
            let flat = Arc::new(flattened(&src));
            let lines = finder.find(&flat)?;
            Ok((flat, lines))
        })
        .await?
    };
    let mut reader_kind = ReaderKind::Vision;
    let mut ai_error = None;
    let mut readings = reader::vision_readings(&lines);
    if let Some(reader) = deps.reader.clone() {
        phase(Phase::Reading);
        match reader.read(&flat, &lines).await {
            Ok(r) if r.len() == lines.len() => {
                readings = r;
                reader_kind = ReaderKind::Ai;
            }
            Ok(_) => ai_error = Some("bad_reply".to_string()),
            Err(e) => ai_error = Some(e.code().unwrap_or("bad_reply").to_string()),
        }
    }
    drop(flat);
    phase(Phase::Matching);
    // ink, decline, shortlist: pixels, off the runtime
    let (mut text_lines, inks, wanted) = {
        let src = source.clone();
        let catalog = deps.catalog.clone();
        let image = image_id.to_string();
        crate::redraw::blocking(move || {
            let mut text_lines = Vec::new();
            let mut inks = Vec::new();
            let mut wanted: Vec<(Vec<FontRef>, Vec<FontRef>)> = Vec::new();
            for (n, (line, reading)) in lines.iter().zip(&readings).enumerate() {
                let ink = ink::line_ink(&src, line.quad);
                let mut tl = TextLine { id: format!("{image}-t{n}"), quad: line.quad, string: reading.string.clone(), class: reading.class, colour: ink.colour, status: Status::Matched, candidates: Vec::new(), chosen: None, enabled: true, fit: None };
                let mut hints: Vec<FontRef> = Vec::new();
                let mut shortlist: Vec<FontRef> = Vec::new();
                match ink::decline(&ink, &reading.string, reading.class, line.quad) {
                    Some(reason) => {
                        tl.status = Status::Declined(reason);
                        tl.enabled = false;
                    }
                    None => {
                        for h in &reading.hints {
                            if let Some(f) = resolve_hint(h, &catalog) {
                                if !hints.contains(&f) && hints.len() < MAX_HINTS {
                                    hints.push(f);
                                }
                            }
                        }
                        let widths = catalog::measured_widths(&ink, &reading.string);
                        let stem = catalog::measured_stem(&ink);
                        for f in catalog.shortlist(&reading.string, &widths, stem, SHORTLIST) {
                            if !hints.contains(&f) {
                                shortlist.push(f);
                            }
                        }
                    }
                }
                text_lines.push(tl);
                inks.push(Arc::new(ink));
                wanted.push((hints, shortlist));
            }
            Ok((text_lines, inks, wanted))
        })
        .await?
    };
    // the Google ones downloaded, best effort
    let mut fonts_offline = false;
    if let Some(gf) = deps.fonts.clone() {
        let mut google: Vec<FontRef> = Vec::new();
        for f in wanted.iter().flat_map(|(h, s)| h.iter().chain(s)).filter(|f| f.source == Source::Google) {
            if !google.contains(f) {
                google.push(f.clone());
            }
        }
        fonts_offline = gf.ensure(&google).await.offline;
    }
    // the fits, off the runtime; a family that won a line leads the next line's list
    let src = source;
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
            let (hints, shortlist) = &wanted[i];
            let mut fonts: Vec<FontRef> = winners.clone();
            let lead = fonts.len();
            fonts.extend(hints.iter().filter(|f| !winners.contains(f)).cloned());
            let hinted = fonts.len() - lead;
            fonts.extend(shortlist.iter().filter(|f| !winners.contains(f)).cloned());
            let any_located = fonts.iter().any(|f| catalog.locate(f, &cache_dir).is_some());
            // the winners and the shortlist's head share the four-family budget; the hints are on top of it
            let mut fitted = fit_candidates(&line.string, &inks[i], &fonts, hinted + EAGER_FAMILIES, &catalog, &cache_dir);
            with_weight_neighbours(&line.string, &inks[i], &mut fitted, &catalog, &cache_dir);
            finish_line(line, fitted, any_located, None);
            if line.status == Status::Matched {
                let f = line.candidates[0].font.clone();
                winners.retain(|w| *w != f);
                winners.insert(0, f);
                winners.truncate(MAX_WINNERS);
            }
        }
        let plan = TextPlan { image, reader: reader_kind, lines: text_lines, ai_error, fonts_offline };
        let (painted, group) = paint_and_group(&src, &plan, &inks, hidden, &catalog, &cache_dir)?;
        Ok(Planned { plan, inks, painted, group })
    })
    .await
}

/// One line changed: a new string refits the line's candidates, a font the user asked for is chosen and the line
/// matched and enabled whatever its score, `enabled` switches it. The painted image and the group are rebuilt.
/// An unknown line is a 404 `no_line`.
pub async fn set_line(source: Arc<Image>, planned: &Planned, line_id: &str, change: LineChange, deps: Deps) -> Result<Planned, CommandError> {
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
        let ink = &inks[at];
        let line = &mut plan.lines[at];
        let was_off = line.status == Status::Off;
        let mut restring = false;
        if let Some(s) = change.string {
            let s = s.trim().to_string();
            if !s.is_empty() && s != line.string {
                line.string = s;
                restring = true;
            }
        }
        if restring {
            // every candidate again against the new string, the asked-for font leading; a line that has none
            // (it was declined before any was fitted) starts from the shortlist
            let mut fonts: Vec<FontRef> = line.candidates.iter().map(|c| c.font.clone()).collect();
            if let Some(f) = &change.font {
                fonts.retain(|c| c != f);
                fonts.insert(0, f.clone());
            }
            if fonts.is_empty() {
                fonts = catalog.shortlist(&line.string, &catalog::measured_widths(ink, &line.string), catalog::measured_stem(ink), SHORTLIST);
            }
            let any_located = fonts.iter().any(|f| catalog.locate(f, &cache_dir).is_some());
            let mut fitted = fit_candidates(&line.string, ink, &fonts, EAGER_FAMILIES, &catalog, &cache_dir);
            with_weight_neighbours(&line.string, ink, &mut fitted, &catalog, &cache_dir);
            finish_line(line, fitted, any_located, change.font.as_ref());
            if was_off && change.enabled != Some(true) && line.status == Status::Matched {
                line.status = Status::Off;
                line.enabled = false;
            }
        } else if let Some(f) = &change.font {
            // the string stands: only the asked-for font is fitted, the others keep their scores
            if let Some(fit) = refit_one(&line.string, ink, f, &catalog, &cache_dir) {
                line.candidates.retain(|c| c.font != *f);
                line.candidates.push(Candidate { font: f.clone(), iou: fit.iou });
                line.candidates.sort_by(|a, b| b.iou.total_cmp(&a.iou));
                line.chosen = line.candidates.iter().position(|c| c.font == *f);
                line.fit = Some(fit);
                line.status = Status::Matched;
                line.enabled = true;
            }
        }
        if let Some(on) = change.enabled {
            match (on, line.chosen) {
                (true, Some(_)) => {
                    line.status = Status::Matched;
                    line.enabled = true;
                }
                (true, None) => {
                    // forcing a declined line on: its best candidate, if it has one
                    if let Some(first) = line.candidates.first().map(|c| c.font.clone()) {
                        if let Some(fit) = refit_one(&line.string, ink, &first, &catalog, &cache_dir) {
                            line.chosen = Some(0);
                            line.fit = Some(fit);
                            line.status = Status::Matched;
                            line.enabled = true;
                        }
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
    fit_candidates(string, ink, std::slice::from_ref(font), 1, catalog, cache_dir).into_iter().next().map(|(_, f)| f)
}

pub struct Entry {
    pub planned: Arc<Planned>,
    pub hash: [u8; 32],
}

struct Running {
    generation: u64,
    abort: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Per image: the plan with what it needs to be edited and used, and the plan that is running.
#[derive(Default)]
pub struct Plans {
    entries: Mutex<HashMap<String, Entry>>,
    running: Mutex<HashMap<String, Running>>,
}

impl Plans {
    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The plan, only while the image's bytes still hash to `hash`.
    pub fn get(&self, image: &str, hash: &[u8; 32]) -> Option<Arc<Planned>> {
        Self::lock(&self.entries).get(image).filter(|e| &e.hash == hash).map(|e| e.planned.clone())
    }

    pub fn put(&self, image: &str, hash: [u8; 32], planned: Planned) -> Arc<Planned> {
        let p = Arc::new(planned);
        Self::lock(&self.entries).insert(image.to_string(), Entry { planned: p.clone(), hash });
        p
    }

    /// The plan is dropped and a running one is aborted.
    pub fn forget(&self, image: &str) {
        Self::lock(&self.entries).remove(image);
        let running = Self::lock(&self.running).remove(image);
        if let Some(a) = running.and_then(|r| r.abort) {
            a();
        }
    }

    /// What a trace should use: the painted PNG and the group, when the plan has a matched, enabled line.
    pub fn for_trace(&self, image: &str, hash: &[u8; 32]) -> Option<(Arc<Vec<u8>>, String)> {
        let entries = Self::lock(&self.entries);
        let e = entries.get(image).filter(|e| &e.hash == hash)?;
        e.planned.painted.as_ref().map(|p| (p.clone(), e.planned.group.clone()))
    }

    /// A new generation for the image's plan; the one running is aborted. As `redraw::Redraws`.
    pub fn reserve(&self, image: &str) -> u64 {
        let (next, old) = {
            let mut running = Self::lock(&self.running);
            let next = running.get(image).map_or(1, |r| r.generation + 1);
            (next, running.insert(image.to_string(), Running { generation: next, abort: None }))
        };
        // the abort runs outside the lock
        if let Some(a) = old.and_then(|o| o.abort) {
            a();
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

    /// Aborts the running plan, if any.
    pub fn cancel(&self, image: &str) -> bool {
        let running = Self::lock(&self.running).remove(image);
        match running {
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
