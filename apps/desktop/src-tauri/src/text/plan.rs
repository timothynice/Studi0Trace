//! The orchestration: `plan` finds, reads, matches and fits every line of an image, paints the matched ones out
//! and composes the group; `set_line` changes one line and rebuilds both. Everything that is pixels runs on the
//! blocking pool.
use super::*;
use crate::redraw::Source as Image;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A line's early stop: once a candidate of the ink's weight fits this well, the rest of the list is not fitted.
/// What the true font scores on a crisp line (the fixture set's true fonts score 0.84 up); a real source's right
/// face scores less, so its line fits the whole list, and a family that won an earlier line, scoring above the
/// accept but below this, does not end the search before a better one is tried.
pub const FIT_SURE: f32 = 0.84;
/// Acceptable candidates within this IoU of the best are tied, and the stroke nearest the ink's breaks the tie.
const TIE_IOU: f32 = 0.05;
/// What a tied candidate pays per unit of |ln stroke| (see `pick`).
const STROKE_CHARGE: f32 = 0.25;
/// An ink stroke under this share of the ink's height is a hairline: the family's 200 and 300 are fitted too.
const HAIRLINE: f32 = 0.08;
/// Screened candidates fitted in full per line.
const REFINED: usize = 4;
/// Families whose neighbouring weights are screened per line.
const NEIGHBOUR_FAMILIES: usize = 3;
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

/// Whether a fit draws the ink's weight: its stroke within `STROKE_AGREE` of the ink's (an unmeasured one is not
/// held against it).
pub fn stroke_agrees(fit: &Fit) -> bool {
    fit.stroke <= 0.0 || (fit.stroke - 1.0).abs() <= STROKE_AGREE
}

/// Whether a fit is good enough to re-set the line: its IoU at least `FIT_ACCEPT` and its stroke the ink's.
pub fn acceptable(fit: &Fit) -> bool {
    fit.iou >= FIT_ACCEPT && stroke_agrees(fit)
}

/// How far a fit's stroke is from the ink's, for the tie-break; an unmeasured one counts as at the limit.
fn stroke_error(fit: &Fit) -> f32 {
    if fit.stroke > 0.0 { fit.stroke.ln().abs() } else { (1.0 + STROKE_AGREE).ln() }
}

/// The candidate a line is set in: among the acceptable fits, those within `TIE_IOU` of the best IoU are a tie
/// the IoU cannot break (on a hairline source it is edge-dominated, and a 500 can outscore the 300 the source is),
/// and the tie goes to the best IoU less `STROKE_CHARGE` per unit of |ln stroke|: a 500 drawn 30 % heavier than
/// the ink pays 0.066 and loses to a 300 of the ink's weight 0.05 below it, while two faces whose strokes are
/// both within a few per cent of the ink's stay ordered by their IoU. None when no fit is acceptable.
fn pick(fitted: &[(Candidate, Fit)]) -> Option<usize> {
    let best = fitted.iter().filter(|(_, f)| acceptable(f)).map(|(_, f)| f.iou).fold(f32::MIN, f32::max);
    let merit = |f: &Fit| f.iou - STROKE_CHARGE * stroke_error(f);
    (0..fitted.len()).filter(|&i| acceptable(&fitted[i].1) && fitted[i].1.iou >= best - TIE_IOU).max_by(|&a, &b| merit(&fitted[a].1).total_cmp(&merit(&fitted[b].1)).then(b.cmp(&a)))
}

/// `fonts` fitted in order, best first: in full (`fit::fit_line`) when `full`, else screened (`fit_line_quick`).
/// The list is not run to the end: the first `eager` fonts are always fitted (the reader's hints, the previous
/// lines' winners, the top of the shortlist, a font the user asked for), and after them each font is fitted only
/// while no fit of the ink's weight reaches `FIT_SURE`, stopping as soon as one does (a fit of the wrong weight
/// does not stop it). A font that is never fitted is not a candidate; one that cannot be located or loaded is
/// left out.
fn fit_candidates(string: &str, ink: &ink::LineInk, fonts: &[FontRef], eager: usize, catalog: &catalog::Catalog, cache_dir: &Path, full: bool) -> Vec<(Candidate, Fit)> {
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
        let fitted = if full { fit::fit_line(&shaped, ink, caps) } else { fit::fit_line_quick(&shaped, ink, caps) };
        if let Some(fit) = fitted {
            if stroke_agrees(&fit) {
                best = best.max(fit.iou);
            }
            out.push((Candidate { font: f.clone(), iou: fit.iou }, fit));
        }
    }
    sort_best_first(&mut out);
    out
}

fn sort_best_first(fitted: &mut [(Candidate, Fit)]) {
    fitted.sort_by(|a, b| b.0.iou.total_cmp(&a.0.iou));
}

/// The screened candidates in the order they are worth a full fit: those of the ink's weight first, each group by
/// its IoU less the stroke's charge (`pick`'s merit).
fn by_promise(fitted: &mut [(Candidate, Fit)]) {
    let merit = |f: &Fit| f.iou - STROKE_CHARGE * stroke_error(f);
    fitted.sort_by(|a, b| stroke_agrees(&b.1).cmp(&stroke_agrees(&a.1)).then(merit(&b.1).total_cmp(&merit(&a.1))));
}

/// The `REFINED` most promising screened candidates fitted in full (a screen is a tenth of a full fit, so every font
/// is screened and only these few are fitted), first and best first, then the rest as screened. Returns the list
/// and how many at its head are full fits.
fn refine(string: &str, ink: &ink::LineInk, mut fitted: Vec<(Candidate, Fit)>, catalog: &catalog::Catalog, cache_dir: &Path) -> (Vec<(Candidate, Fit)>, usize) {
    by_promise(&mut fitted);
    let rest = fitted.split_off(fitted.len().min(REFINED));
    let fonts: Vec<FontRef> = fitted.iter().map(|(c, _)| c.font.clone()).collect();
    let mut full = fit_candidates(string, ink, &fonts, fonts.len(), catalog, cache_dir, true);
    let n = full.len();
    let mut rest: Vec<(Candidate, Fit)> = rest.into_iter().filter(|(c, _)| !full.iter().any(|(f, _)| f.font == c.font)).collect();
    sort_best_first(&mut rest);
    full.extend(rest);
    (full, n)
}

/// The other weights worth fitting of the `NEIGHBOUR_FAMILIES` best families by IoU: each one's neighbours ±100,
/// and its 200 and 300 when the ink's stroke is under `HAIRLINE` of its height (the weight the stem picked is a
/// guess, a face at the wrong weight scores below a worse face at the right one, and a light source is where IoU
/// most often prefers the heavier face). Those already fitted are left out.
fn weight_neighbours(ink: &ink::LineInk, fitted: &[(Candidate, Fit)], catalog: &catalog::Catalog) -> Vec<FontRef> {
    let hairline = match (fit::ink_stroke(ink), ink.ink_bounds) {
        (Some(w), Some((_, t, _, b))) if b > t => w / ((b - t) as f32) < HAIRLINE,
        _ => false,
    };
    let mut ranked: Vec<&Candidate> = fitted.iter().map(|(c, _)| c).collect();
    ranked.sort_by(|a, b| b.iou.total_cmp(&a.iou));
    let mut families: Vec<&Candidate> = Vec::new();
    for c in ranked {
        if families.len() < NEIGHBOUR_FAMILIES && !families.iter().any(|f| f.font.family == c.font.family && f.font.source == c.font.source) {
            families.push(c);
        }
    }
    let mut out: Vec<FontRef> = Vec::new();
    for best in families {
        for weight in catalog.weights_of(&best.font.family, best.font.source) {
            let f = FontRef { weight, ..best.font.clone() };
            if ((weight as i32 - best.font.weight as i32).abs() == 100 || (hairline && (weight == 200 || weight == 300))) && !fitted.iter().any(|(c, _)| c.font == f) && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// The best families' other weights, screened where they are already on this Mac or in the cache (a refit runs
/// off the runtime and downloads nothing).
fn with_weight_neighbours(string: &str, ink: &ink::LineInk, fitted: &mut Vec<(Candidate, Fit)>, catalog: &catalog::Catalog, cache_dir: &Path) {
    let more = weight_neighbours(ink, fitted, catalog);
    if !more.is_empty() {
        let n = more.len();
        fitted.extend(fit_candidates(string, ink, &more, n, catalog, cache_dir, false));
        sort_best_first(fitted);
    }
}

/// The line's result from its candidates, the first `full` of them fitted in full: the `pick` among those, moved to
/// the front of the list (the rest stay in order). A `forced` font (the user's choice) that was fitted is chosen
/// and the line matched and enabled whatever its score.
fn finish_line(line: &mut TextLine, mut fitted: Vec<(Candidate, Fit)>, full: usize, any_located: bool, forced: Option<&FontRef>) {
    if fitted.is_empty() {
        line.status = Status::Declined(if any_located { Reason::NoFit } else { Reason::Fonts });
        line.candidates.clear();
        line.chosen = None;
        line.fit = None;
        line.enabled = false;
        return;
    }
    let at = forced.and_then(|f| fitted.iter().position(|(c, _)| c.font == *f));
    let pick = at.or_else(|| pick(&fitted[..full.min(fitted.len())]));
    if let Some(p) = pick {
        let chosen = fitted.remove(p);
        fitted.insert(0, chosen);
    }
    let fit = fitted[0].1.clone();
    line.candidates = fitted.into_iter().map(|(c, _)| c).collect();
    if pick.is_some() {
        line.status = Status::Matched;
        line.chosen = Some(0);
        line.fit = Some(fit);
        line.enabled = true;
    } else {
        line.status = Status::Declined(Reason::NoFit);
        line.chosen = None;
        line.fit = None;
        line.enabled = false;
    }
}

/// The painted PNG (when a line is composed) and the group.
type Painted = (Option<Arc<Vec<u8>>>, String);

/// The group, and the source with painted out only the lines the group carries: a matched line whose font cannot
/// be located is not composed, so its pixels stay in what the trace sees.
fn paint_and_group(source: &Image, plan: &TextPlan, inks: &[Arc<ink::LineInk>], hidden_text: bool, catalog: &catalog::Catalog, cache_dir: &Path) -> Result<Painted, CommandError> {
    let composed = compose::compose_group(plan, hidden_text, catalog, cache_dir);
    let used: Vec<&ink::LineInk> = plan.lines.iter().zip(inks).filter(|(l, _)| composed.lines.contains(&l.id)).map(|(_, i)| i.as_ref()).collect();
    let painted = if used.is_empty() {
        None
    } else {
        let rgba = ink::paint_out(source, &used);
        Some(Arc::new(crate::redraw::encode_png(&rgba, source.width, source.height, image::ExtendedColorType::Rgba8)?))
    };
    Ok((painted, composed.group))
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
    // the reader cannot add a line, so an image in which Vision found none is never uploaded
    if let Some(reader) = deps.reader.clone().filter(|_| !lines.is_empty()) {
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
                // the word spaces the ink shows and the reader dropped
                let string = ink::repair_spaces(&ink, &reading.string);
                let mut tl = TextLine { id: format!("{image}-t{n}"), quad: line.quad, string: string.clone(), class: reading.class, colour: ink.colour, status: Status::Matched, candidates: Vec::new(), chosen: None, enabled: true, fit: None };
                let mut hints: Vec<FontRef> = Vec::new();
                let mut shortlist: Vec<FontRef> = Vec::new();
                match ink::decline(&ink, &string, reading.class, line.quad) {
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
                        let widths = catalog::measured_widths(&ink, &string);
                        let stem = catalog::measured_stem(&ink);
                        for f in catalog.shortlist(&string, &widths, stem, SHORTLIST) {
                            if !hints.contains(&f) {
                                shortlist.push(f);
                            }
                        }
                        // a line the reader gave no hints also tries the plain sans faces, after its ranked ones
                        if hints.is_empty() {
                            for f in catalog.floor(stem) {
                                if !shortlist.iter().any(|s| s.family == f.family) {
                                    shortlist.push(f);
                                }
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
    // the fits, off the runtime, a line at a time: the best family's other weights are only known once its line is
    // fitted, and they are downloaded before they are fitted. A family that won a line leads the next line's list
    let mut winners: Vec<FontRef> = Vec::new();
    for i in 0..text_lines.len() {
        if text_lines[i].status != Status::Matched {
            continue;
        }
        let (hints, shortlist) = &wanted[i];
        let mut fonts: Vec<FontRef> = winners.clone();
        let lead = fonts.len();
        fonts.extend(hints.iter().filter(|f| !winners.contains(f)).cloned());
        let hinted = fonts.len() - lead;
        fonts.extend(shortlist.iter().filter(|f| !winners.contains(f)).cloned());
        let (catalog, cache_dir, ink, string) = (deps.catalog.clone(), deps.cache_dir.clone(), inks[i].clone(), text_lines[i].string.clone());
        // every font screened (the winners and the shortlist's head share the four-family budget; the hints are on top
        // of it), the best families' other weights downloaded and screened, and the most promising fitted in full
        let (mut fitted, any_located) = crate::redraw::blocking(move || {
            let any_located = fonts.iter().any(|f| catalog.locate(f, &cache_dir).is_some());
            Ok((fit_candidates(&string, &ink, &fonts, hinted + EAGER_FAMILIES, &catalog, &cache_dir, false), any_located))
        })
        .await?;
        let more = weight_neighbours(&inks[i], &fitted, &deps.catalog);
        if !more.is_empty() {
            if let Some(gf) = deps.fonts.clone() {
                let google: Vec<FontRef> = more.iter().filter(|f| f.source == Source::Google).cloned().collect();
                fonts_offline |= gf.ensure(&google).await.offline;
            }
            let (catalog, cache_dir, ink, string) = (deps.catalog.clone(), deps.cache_dir.clone(), inks[i].clone(), text_lines[i].string.clone());
            let n = more.len();
            fitted.extend(crate::redraw::blocking(move || Ok(fit_candidates(&string, &ink, &more, n, &catalog, &cache_dir, false))).await?);
        }
        let (catalog, cache_dir, ink, string) = (deps.catalog.clone(), deps.cache_dir.clone(), inks[i].clone(), text_lines[i].string.clone());
        let (fitted, full) = crate::redraw::blocking(move || Ok(refine(&string, &ink, fitted, &catalog, &cache_dir))).await?;
        let line = &mut text_lines[i];
        finish_line(line, fitted, full, any_located, None);
        if line.status == Status::Matched {
            let f = line.candidates[0].font.clone();
            winners.retain(|w| *w != f);
            winners.insert(0, f);
            winners.truncate(MAX_WINNERS);
        }
    }
    let src = source;
    let catalog = deps.catalog.clone();
    let cache_dir = deps.cache_dir.clone();
    let hidden = deps.hidden_text;
    let image = image_id.to_string();
    crate::redraw::blocking(move || {
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
        let was_chosen = matches!(line.status, Status::Matched | Status::Off);
        let mut restring = false;
        if let Some(s) = change.string {
            let s = s.trim().to_string();
            if !s.is_empty() && s != line.string {
                line.string = s;
                restring = true;
            }
        }
        if restring {
            // every candidate again against the new string. The font the line stands on (the user's, or the one
            // the match chose) is kept whatever the new string scores; one asked for in this call leads
            let keep = change.font.clone().or_else(|| if was_chosen { line.chosen.and_then(|i| line.candidates.get(i)).map(|c| c.font.clone()) } else { None });
            let mut fonts: Vec<FontRef> = line.candidates.iter().map(|c| c.font.clone()).collect();
            if let Some(f) = &keep {
                fonts.retain(|c| c != f);
                fonts.insert(0, f.clone());
            }
            // a line that has none (it was declined before any was fitted) starts from the shortlist
            if fonts.is_empty() {
                let stem = catalog::measured_stem(ink);
                fonts = catalog.shortlist(&line.string, &catalog::measured_widths(ink, &line.string), stem, SHORTLIST);
                for f in catalog.floor(stem) {
                    if !fonts.iter().any(|s| s.family == f.family) {
                        fonts.push(f);
                    }
                }
            }
            let any_located = fonts.iter().any(|f| catalog.locate(f, &cache_dir).is_some());
            let mut fitted = fit_candidates(&line.string, ink, &fonts, EAGER_FAMILIES, &catalog, &cache_dir, false);
            with_weight_neighbours(&line.string, ink, &mut fitted, &catalog, &cache_dir);
            // the font the line stands on is fitted in full whatever its promise
            if let Some(k) = keep.as_ref().and_then(|k| fitted.iter().position(|(c, _)| c.font == *k)) {
                let kept = fitted.remove(k);
                if let Some(fit) = refit_one(&line.string, ink, &kept.0.font, &catalog, &cache_dir) {
                    fitted.push((Candidate { font: kept.0.font, iou: fit.iou }, fit));
                }
            }
            let (fitted, full) = refine(&line.string, ink, fitted, &catalog, &cache_dir);
            finish_line(line, fitted, full, any_located, keep.as_ref());
            // a line the user switched off stays off, unless this call asks for a font or for it on
            if was_off && change.enabled != Some(true) && change.font.is_none() && line.status == Status::Matched {
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

/// Whether an edit changed what the trace sees: the painted image `after` differs from `before`'s (a line composed or
/// no longer composed — a font picked for a declined or Off line, a line switched on or off, a font that cannot be
/// located). A string or font edit of a line that stays composed paints out the same ink and is not a repaint:
/// the traces are composed again, not traced again.
pub fn repainted(before: &Planned, after: &Planned) -> bool {
    match (&before.painted, &after.painted) {
        (None, None) => false,
        (Some(a), Some(b)) => !(Arc::ptr_eq(a, b) || a == b),
        _ => true,
    }
}

fn refit_one(string: &str, ink: &ink::LineInk, font: &FontRef, catalog: &catalog::Catalog, cache_dir: &Path) -> Option<Fit> {
    fit_candidates(string, ink, std::slice::from_ref(font), 1, catalog, cache_dir, true).into_iter().next().map(|(_, f)| f)
}


#[cfg(test)]
mod tests {
    use super::*;

    fn fitted(family: &str, weight: u16, iou: f32, stroke: f32) -> (Candidate, Fit) {
        let font = FontRef { family: family.into(), weight, italic: false, source: Source::Google };
        (Candidate { font, iou }, Fit { size: 20.0, baseline: 30.0, x: 0.0, tracking: 0.0, nudges: Vec::new(), iou, stroke })
    }

    #[test]
    fn a_fit_of_the_wrong_weight_is_not_acceptable_whatever_its_iou() {
        assert!(acceptable(&fitted("A", 400, 0.7, 1.2).1));
        assert!(!acceptable(&fitted("A", 400, 0.9, 1.5).1), "50 % heavier than the ink");
        assert!(!acceptable(&fitted("A", 400, 0.9, 0.6).1), "40 % lighter");
        assert!(!acceptable(&fitted("A", 400, FIT_ACCEPT - 0.01, 1.0).1));
        assert!(acceptable(&fitted("A", 400, 0.7, 0.0).1), "an unmeasured stroke is not held against a fit");
    }

    #[test]
    fn on_a_hairline_the_light_weight_beats_the_heavier_one_that_outscores_it() {
        // the 500 scores 0.04 higher (the IoU of a hairline is edge-dominated) but draws 30 % heavier than the ink
        let list = vec![fitted("Kumbh Sans", 500, 0.80, 1.30), fitted("Kumbh Sans", 300, 0.76, 1.02), fitted("Other", 400, 0.95, 1.6)];
        assert_eq!(pick(&list), Some(1));
        // two faces whose strokes are both the ink's within a few per cent stay ordered by their IoU
        let list = vec![fitted("Castoro", 400, 0.777, 1.035), fitted("Plantagenet", 400, 0.728, 0.969)];
        assert_eq!(pick(&list), Some(0));
        // a better stroke more than TIE_IOU below the best does not win
        let list = vec![fitted("A", 500, 0.80, 1.30), fitted("A", 300, 0.74, 1.0)];
        assert_eq!(pick(&list), Some(0));
        // nothing acceptable: declined
        assert_eq!(pick(&[fitted("A", 700, 0.9, 1.6)]), None);
    }

    #[test]
    fn the_chosen_candidate_leads_the_list_and_the_line_is_declined_when_none_is_acceptable() {
        let mut line = TextLine { id: "i-t0".into(), quad: [[0.0; 2]; 4], string: "AB".into(), class: Class::Type, colour: [0; 3], status: Status::Matched, candidates: Vec::new(), chosen: None, enabled: true, fit: None };
        finish_line(&mut line, vec![fitted("K", 500, 0.80, 1.30), fitted("K", 300, 0.76, 1.02)], 2, true, None);
        assert_eq!((line.status, line.chosen, line.candidates[0].font.weight), (Status::Matched, Some(0), 300));
        assert_eq!(line.fit.as_ref().unwrap().stroke, 1.02);
        finish_line(&mut line, vec![fitted("K", 700, 0.9, 1.6)], 1, true, None);
        assert_eq!(line.status, Status::Declined(Reason::NoFit));
        assert!(line.fit.is_none() && line.chosen.is_none() && line.candidates.len() == 1);
        // only a full fit is chosen: a screened candidate, however promising, stays in the list behind it
        finish_line(&mut line, vec![fitted("K", 400, 0.70, 1.0), fitted("L", 400, 0.74, 1.0)], 1, true, None);
        assert_eq!((line.status, line.candidates[0].font.family.as_str(), line.candidates.len()), (Status::Matched, "K", 2));
        finish_line(&mut line, vec![fitted("L", 400, 0.74, 1.0)], 0, true, None);
        assert_eq!(line.status, Status::Declined(Reason::NoFit), "nothing fitted in full, nothing chosen");
    }

    #[test]
    fn the_best_family_s_neighbours_are_fitted_and_its_light_weights_too_on_a_hairline() {
        let widths = vec!["0"; 62].join(",");
        let variants: Vec<String> = [200, 300, 400, 500, 600, 700].iter().map(|w| format!(r#"{{"weight":{w},"italic":false}}"#)).collect();
        let json = format!(r#"{{"families":[{{"name":"K","category":"sans-serif","variants":[{}],"features":{{"cap":0.7,"x":0.5,"stem":0.1,"widths":[{widths}]}}}}]}}"#, variants.join(","));
        let cat = catalog::Catalog::from_index(&json, fontdb::Database::new());
        // a line of vertical bars 3 px wide and 30 tall (stroke 0.1 of the height), and 1.5 px wide (0.05)
        let ink_of = |stroke: f32| {
            let (w, h) = (40u32, 40u32);
            let mut coverage = vec![0f32; (w * h) as usize];
            for y in 5..35 {
                for x0 in [10u32, 25] {
                    let whole = stroke.floor() as u32;
                    for x in x0..x0 + whole {
                        coverage[(y * w + x) as usize] = 1.0;
                    }
                    coverage[(y * w + x0 + whole) as usize] = stroke.fract();
                }
            }
            let mask = coverage.iter().map(|c| *c >= 0.5).collect();
            ink::LineInk { left: 0, top: 0, width: w, height: h, coverage, mask, backdrop: [255; 3], colour: [0; 3], spread: 0.0, segments: vec![(10, 14), (25, 29)], ink_bounds: Some((10, 5, 29, 35)) }
        };
        let list = vec![fitted("K", 500, 0.7, 1.3)];
        let weights = |stroke: f32| -> Vec<u16> { weight_neighbours(&ink_of(stroke), &list, &cat).iter().map(|f| f.weight).collect() };
        assert_eq!(weights(3.0), [400, 600]);
        assert_eq!(weights(1.5), [200, 300, 400, 600]);
        // the three best families each get theirs, a fourth does not: a face at the wrong weight scores under a
        // worse face at the right one, and must still reach its right weight
        let json4 = ["K", "L", "M", "N"].iter().map(|n| format!(r#"{{"name":"{n}","category":"sans-serif","variants":[{}],"features":{{"cap":0.7,"x":0.5,"stem":0.1,"widths":[{widths}]}}}}"#, variants.join(","))).collect::<Vec<_>>().join(",");
        let cat4 = catalog::Catalog::from_index(&format!(r#"{{"families":[{json4}]}}"#), fontdb::Database::new());
        let list = vec![fitted("K", 400, 0.72, 0.9), fitted("K", 500, 0.71, 1.1), fitted("L", 400, 0.66, 1.6), fitted("M", 600, 0.65, 1.0), fitted("N", 400, 0.64, 1.0)];
        let more: Vec<(String, u16)> = weight_neighbours(&ink_of(3.0), &list, &cat4).iter().map(|f| (f.family.clone(), f.weight)).collect();
        assert_eq!(more, [("K".to_string(), 300), ("L".to_string(), 300), ("L".to_string(), 500), ("M".to_string(), 500), ("M".to_string(), 700)]);
    }
}
