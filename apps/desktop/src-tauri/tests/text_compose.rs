//! The text group: one path per matched, enabled line in the ink colour, the hidden <text> when asked, source
//! pixel coordinates, and the core's renderer draws the paths and nothing for the <text>.
use std::path::PathBuf;
use studi0trace_desktop::text::catalog::{cache_name, Catalog};
use studi0trace_desktop::text::compose::text_group;
use studi0trace_desktop::text::{Candidate, Class, Fit, FontRef, ReaderKind, Source, Status, TextLine, TextPlan};

fn cache_with_fixture_font() -> (PathBuf, FontRef) {
    // one folder per call: the tests run in parallel and each removes its own when it is done
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("studi0trace-compose-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    let font = FontRef { family: "Montserrat".into(), weight: 400, italic: false, source: Source::Google };
    std::fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts/Montserrat-Regular.ttf"), dir.join(cache_name(&font))).unwrap();
    (dir, font)
}

fn plan(font: &FontRef, enabled: bool, status: Status) -> TextPlan {
    let mut nudges = vec![0.0; 9];
    nudges[5] = 12.0;
    let fit = Fit { size: 40.0, baseline: 60.0, x: 10.0, tracking: 2.0, nudges, iou: 0.95, stroke: 1.0 };
    TextPlan {
        image: "img".into(),
        reader: ReaderKind::Vision,
        lines: vec![TextLine { id: "img-t0".into(), quad: [[8.0, 20.0], [220.0, 20.0], [220.0, 70.0], [8.0, 70.0]], string: "EST. 2026".into(), class: Class::Type, colour: [24, 28, 44], status, candidates: vec![Candidate { font: font.clone(), iou: 0.95, stroke: 1.0, fit: fit.clone(), path: String::new(), text: String::new() }], chosen: Some(0), enabled, fit: Some(fit), occluders: Vec::new() }],
        ai_error: None,
        fonts_offline: false,
    }
}

/// A number as the group writes it: two decimals, trailing zeros dropped.
fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
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
    // the hidden text carries one x per character, each glyph's origin: 10, then the advances at 40 px plus the
    // tracking of 2, and the nudge of the "2" after the space (12 px) on it alone
    let shaped = studi0trace_desktop::text::glyphs::Font::load(&dir.join(cache_name(&font)), 0).unwrap().shape("EST. 2026");
    let mut at = 10.0f32;
    let mut xs = Vec::new();
    for (i, g) in shaped.glyphs.iter().enumerate() {
        xs.push(num(at + g.x_offset * 40.0 + if i == 5 { 12.0 } else { 0.0 }));
        at += g.advance * 40.0 + 2.0;
    }
    assert_eq!(xs.len(), 9);
    let want = format!("<text x=\"{}\" y=\"60\" font-family=\"Montserrat\" font-size=\"40\" fill=\"none\" pointer-events=\"none\" data-line=\"img-t0\">EST. 2026</text>", xs.join(" "));
    assert!(with.contains(&want), "{with}\nwanted {want}");
    // the per-candidate helpers write exactly what the group does
    let fit = plan(&font, true, Status::Matched).lines[0].fit.clone().unwrap();
    assert_eq!(studi0trace_desktop::text::compose::candidate_text("img-t0", "EST. 2026", "Montserrat", &shaped, &fit), want);
    let d = studi0trace_desktop::text::compose::candidate_path(&shaped, &fit);
    assert!(with.contains(&format!("<path d=\"{d}\" fill=\"#181c2c\" data-line=\"img-t0\"/>")), "{with}");
    let without = text_group(&plan(&font, true, Status::Matched), false, &cat, &dir);
    assert!(!without.contains("<text"));
    assert_eq!(text_group(&plan(&font, false, Status::Off), true, &cat, &dir), "<g id=\"text\"></g>");
    // the path starts near the fit: an M within the line's box
    let d = with.split("<path d=\"").nth(1).unwrap();
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
    // the control characters XML cannot carry are dropped; tab, LF and CR stay
    assert_eq!(studi0trace_desktop::text::compose::escape_text("A\u{0}B\u{1b}C\u{7f}D\tE\nF\rG\u{8}"), "ABCD\tE\nF\rG");
}

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
    let finder = Arc::new(FixtureFinder(vec![LineBox { quad: [[20.0, 14.0], [w - 20.0, 14.0], [w - 20.0, 58.0_f32.min(h)], [20.0, 58.0_f32.min(h)]], string: "EST. 2026".into(), confidence: 0.9 }]));
    let phases: Arc<Mutex<Vec<Phase>>> = Arc::default();
    let seen = phases.clone();
    let deps = Deps { finder, reader: None, catalog: Arc::new(catalog()), fonts: None, cache_dir: dir.clone(), hidden_text: true };
    let planned = tauri::async_runtime::block_on(plan("img", img.clone(), deps.clone(), Arc::new(move |p| seen.lock().unwrap().push(p)))).unwrap();
    assert_eq!(*phases.lock().unwrap(), vec![Phase::Finding, Phase::Matching, Phase::Done]);
    let line = &planned.plan.lines[0];
    assert_eq!(line.status, Status::Matched, "{:?}", line);
    assert_eq!(line.candidates[0].font, font);
    assert!(line.fit.as_ref().unwrap().iou > 0.85);
    // every candidate carries its own fit, outline and hidden text, and the group's path is the chosen one's
    assert!(!line.candidates.is_empty());
    for c in &line.candidates {
        assert!(c.path.starts_with('M'), "{}", c.path);
        assert!(c.text.contains(">EST. 2026</text>") && c.text.contains("data-line=\"img-t0\""), "{}", c.text);
        assert_eq!(c.fit.iou, c.iou);
    }
    assert!(planned.group.contains(&format!("<path d=\"{}\"", line.candidates[line.chosen.unwrap()].path)));
    assert_eq!(line.candidates[line.chosen.unwrap()].fit, *line.fit.as_ref().unwrap());
    assert!(line.occluders.is_empty());
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

#[test]
fn an_unknown_line_is_a_404_a_forced_font_is_matched_and_plans_are_kept_by_hash() {
    use studi0trace_desktop::redraw::Source as Img;
    use studi0trace_desktop::text::reader::LineBox;
    use studi0trace_desktop::text::{bytes_hash, plan, set_line, Deps, LineChange, Plans};
    use std::sync::Arc;
    let (dir, font) = cache_with_fixture_font();
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/montserrat-est-30-t0-white.png")).unwrap();
    let img = Arc::new(Img::decode(&bytes).unwrap());
    let (w, h) = (img.width as f32, img.height as f32);
    let finder = Arc::new(FixtureFinder(vec![LineBox { quad: [[20.0, 14.0], [w - 20.0, 14.0], [w - 20.0, 58.0_f32.min(h)], [20.0, 58.0_f32.min(h)]], string: "EST. 2026".into(), confidence: 0.9 }]));
    let deps = Deps { finder, reader: None, catalog: Arc::new(catalog()), fonts: None, cache_dir: dir.clone(), hidden_text: false };
    let planned = tauri::async_runtime::block_on(plan("img", img.clone(), deps.clone(), Arc::new(|_| {}))).unwrap();
    assert!(planned.group.contains("<text"), "the hidden text is always written: the frontend leaves it out when the setting is off");
    let err = tauri::async_runtime::block_on(set_line(img.clone(), &planned, "img-t9", LineChange::default(), deps.clone())).err().unwrap();
    assert_eq!((err.status, err.code()), (404, Some("no_line")));
    // off, then a font asked for: back on with that font chosen
    let off = tauri::async_runtime::block_on(set_line(img.clone(), &planned, "img-t0", LineChange { enabled: Some(false), ..Default::default() }, deps.clone())).unwrap();
    let forced = tauri::async_runtime::block_on(set_line(img, &off, "img-t0", LineChange { font: Some(font.clone()), ..Default::default() }, deps)).unwrap();
    let line = &forced.plan.lines[0];
    assert!(line.status == Status::Matched && line.enabled && line.candidates[line.chosen.unwrap()].font == font);
    assert!(forced.painted.is_some());
    // the plans: found by image and hash, a trace gets the painted bytes and the group
    let hash = bytes_hash(&bytes);
    let plans = Plans::default();
    assert!(plans.get("img", &hash).is_none());
    let kept = plans.put("img", hash, forced);
    assert!(Arc::ptr_eq(&plans.get("img", &hash).unwrap(), &kept));
    assert!(plans.get("img", &bytes_hash(b"other")).is_none() && plans.for_trace("img", &bytes_hash(b"other")).is_none());
    let (png, group) = plans.for_trace("img", &hash).unwrap();
    assert!(Arc::ptr_eq(&png, kept.painted.as_ref().unwrap()) && group == kept.group);
    plans.forget("img");
    assert!(plans.get("img", &hash).is_none());
    // generations: a new reservation supersedes the old, an armed abort runs once on cancel
    let (g1, aborted) = (plans.reserve("img"), Arc::new(std::sync::atomic::AtomicUsize::new(0)));
    let a = aborted.clone();
    plans.arm("img", g1, Box::new(move || { a.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }));
    let g2 = plans.reserve("img");
    assert!(g2 > g1 && !plans.is_current("img", g1) && plans.is_current("img", g2));
    assert_eq!(aborted.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(plans.cancel("img") && !plans.cancel("img"));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- fix round 1: the generations, the refinements and the composed lines ----

mod round1 {
    use super::{cache_with_fixture_font, catalog, plan as sample_plan, FixtureFinder};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use studi0trace_desktop::redraw::Source as Img;
    use studi0trace_desktop::text::catalog::{cache_name, Catalog};
    use studi0trace_desktop::text::compose::compose_group;
    use studi0trace_desktop::text::reader::{Finder, LineBox, ReadFuture, Reader, Reading};
    use studi0trace_desktop::text::plan::FIT_SURE;
    use studi0trace_desktop::text::{bytes_hash, plan, set_line, Class, Deps, FontRef, LineChange, Phase, Plans, Source, Status, FIT_ACCEPT};

    const FONTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/fonts");
    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text");

    fn font(family: &str, weight: u16) -> FontRef {
        FontRef { family: family.into(), weight, italic: false, source: Source::Google }
    }

    /// A cache folder holding `file` under the cache name of each (family, weight), and an index of every family
    /// in `index` (order by name is the shortlist's order: every family's widths are zero), in a fresh folder.
    fn cache(files: &[(&str, &str, u16)]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("studi0trace-compose-r1-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).unwrap();
        for (file, family, weight) in files {
            std::fs::copy(Path::new(FONTS).join(file), dir.join(cache_name(&font(family, *weight)))).unwrap();
        }
        dir
    }

    fn index(families: &[(&str, u16)]) -> Catalog {
        let widths = vec!["0"; 62].join(",");
        let fams: Vec<String> = families.iter().map(|(n, w)| format!(r#"{{"name":"{n}","category":"Sans Serif","variants":[{{"weight":{w},"italic":false}}],"features":{{"cap":0.7,"x":0.5,"stem":0.1,"widths":[{widths}]}}}}"#)).collect();
        Catalog::from_index(&format!(r#"{{"families":[{}]}}"#, fams.join(",")), fontdb::Database::new())
    }

    fn image(name: &str) -> Arc<Img> {
        Arc::new(Img::decode(&std::fs::read(format!("{FIXTURES}/{name}")).unwrap()).unwrap())
    }

    fn line_box(img: &Img, top: f32, bottom: f32) -> LineBox {
        LineBox { quad: [[20.0, top], [img.width as f32 - 20.0, top], [img.width as f32 - 20.0, bottom], [20.0, bottom]], string: "EST. 2026".into(), confidence: 0.9 }
    }

    fn deps(finder: Arc<dyn Finder>, reader: Option<Arc<dyn Reader>>, catalog: Catalog, dir: &Path) -> Deps {
        Deps { finder, reader, catalog: Arc::new(catalog), fonts: None, cache_dir: dir.to_path_buf(), hidden_text: false }
    }

    fn run_plan(img: &Arc<Img>, deps: &Deps) -> studi0trace_desktop::text::Planned {
        tauri::async_runtime::block_on(plan("img", img.clone(), deps.clone(), Arc::new(|_: Phase| {}))).unwrap()
    }

    fn edit(img: &Arc<Img>, planned: &studi0trace_desktop::text::Planned, id: &str, change: LineChange, deps: &Deps) -> studi0trace_desktop::text::Planned {
        tauri::async_runtime::block_on(set_line(img.clone(), planned, id, change, deps.clone())).unwrap()
    }

    #[test]
    fn generations_never_repeat_and_a_cancelled_plan_cannot_store_its_result() {
        let (dir, f) = cache_with_fixture_font();
        let cat = catalog();
        let make = || {
            let mut p = sample_plan(&f, true, Status::Matched);
            p.image = "img".into();
            studi0trace_desktop::text::Planned { plan: p, inks: Vec::new(), painted: None, group: "<g id=\"text\"></g>".into() }
        };
        let _ = (&cat, &dir);
        let plans = Plans::default();
        let hash = bytes_hash(b"x");
        let g1 = plans.reserve("img");
        assert!(plans.cancel("img"));
        let g2 = plans.reserve("img");
        assert_ne!(g1, g2, "a generation is never handed out twice");
        assert!(!plans.is_current("img", g1) && plans.is_current("img", g2));
        assert!(plans.put_if_current("img", g1, hash, make()).is_none(), "the cancelled plan's result is refused");
        assert!(plans.get("img", &hash).is_none());
        // a late arm of the old task aborts it at once and leaves the newer task's abort alone
        let (old, new) = (Arc::new(Mutex::new(0)), Arc::new(Mutex::new(0)));
        let (o, n) = (old.clone(), new.clone());
        plans.arm("img", g2, Box::new(move || *n.lock().unwrap() += 1));
        plans.arm("img", g1, Box::new(move || *o.lock().unwrap() += 1));
        assert_eq!((*old.lock().unwrap(), *new.lock().unwrap()), (1, 0));
        // the current one stores, and ends: a later cancel finds nothing to cancel
        assert!(plans.put_if_current("img", g2, hash, make()).is_some());
        assert!(plans.get("img", &hash).is_some());
        assert!(!plans.cancel("img") && !plans.is_current("img", g2));
        assert_eq!(*new.lock().unwrap(), 0, "a finished plan is not aborted");
        // a failure ends the current one only
        let g3 = plans.reserve("img");
        assert!(plans.fail("img", g3) && !plans.fail("img", g3));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_off_line_given_a_string_and_a_font_is_matched_and_enabled() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400)]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        let off = edit(&img, &planned, "img-t0", LineChange { enabled: Some(false), ..Default::default() }, &d);
        // a string alone keeps it off
        let still = edit(&img, &off, "img-t0", LineChange { string: Some("EST. 2027".into()), ..Default::default() }, &d);
        assert_eq!(still.plan.lines[0].status, Status::Off);
        // a string and a font together turn it on
        let both = edit(&img, &off, "img-t0", LineChange { string: Some("EST. 2027".into()), font: Some(font("Montserrat", 400)), ..Default::default() }, &d);
        let line = &both.plan.lines[0];
        assert!(line.status == Status::Matched && line.enabled && line.string == "EST. 2027", "{line:?}");
        assert!(both.group.contains("<path "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_string_edit_keeps_the_font_the_user_forced_even_when_it_scores_under_the_accept() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400), ("JosefinSans-Light.ttf", "Josefin Sans", 300)]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Josefin Sans", 300), ("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        assert_eq!(planned.plan.lines[0].candidates[planned.plan.lines[0].chosen.unwrap()].font, font("Montserrat", 400));
        let jos = font("Josefin Sans", 300);
        let forced = edit(&img, &planned, "img-t0", LineChange { font: Some(jos.clone()), ..Default::default() }, &d);
        let l = &forced.plan.lines[0];
        assert!(l.fit.as_ref().unwrap().iou < FIT_ACCEPT, "Josefin must score under the accept on this line for the test to mean anything");
        assert!(l.status == Status::Matched && l.candidates[l.chosen.unwrap()].font == jos);
        let edited = edit(&img, &forced, "img-t0", LineChange { string: Some("EST. 2027".into()), ..Default::default() }, &d);
        let l = &edited.plan.lines[0];
        assert!(l.status == Status::Matched && l.enabled && l.candidates[l.chosen.unwrap()].font == jos, "{l:?}");
        assert!(edited.group.contains("<path ") && edited.painted.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Names sort in the shortlist's order (every width is zero). The first four positions are always fitted; then
    // a family is fitted only while no fit of the ink's weight has reached FIT_SURE. An unlocatable family still
    // holds its place.
    fn pruning_dir() -> (PathBuf, Catalog) {
        let dir = cache(&[("Montserrat-Regular.ttf", "A Montserrat", 400), ("JosefinSans-Light.ttf", "F Josefin Sans", 300), ("PlayfairDisplay-Regular.ttf", "G Playfair Display", 400)]);
        let cat = index(&[("A Montserrat", 400), ("B Gap", 400), ("C Gap", 400), ("D Gap", 400), ("E Gap", 400), ("F Josefin Sans", 300), ("G Playfair Display", 400)]);
        (dir, cat)
    }

    #[test]
    fn a_family_that_fits_well_within_the_eager_set_ends_the_search() {
        let (dir, cat) = pruning_dir();
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, cat, &dir);
        let planned = run_plan(&img, &d);
        let line = &planned.plan.lines[0];
        assert!(line.fit.as_ref().unwrap().iou >= FIT_SURE, "{line:?}");
        let names: Vec<&str> = line.candidates.iter().map(|c| c.font.family.as_str()).collect();
        assert_eq!(names, ["A Montserrat"], "the later families were never fitted");
        // and when the first does not fit well, the later ones are fitted
        let img = image("playfair-est-30-t0-white.png");
        let d = Deps { finder: Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), ..d };
        let planned = run_plan(&img, &d);
        let names: Vec<&str> = planned.plan.lines[0].candidates.iter().map(|c| c.font.family.as_str()).collect();
        assert!(names.contains(&"G Playfair Display") && names.contains(&"A Montserrat"), "{names:?} {:?}", planned.plan.lines[0]);
        assert_eq!(planned.plan.lines[0].candidates[0].font.family, "G Playfair Display");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same image twice, one above the other.
    fn stacked(name: &str) -> (Arc<Img>, f32) {
        let one = image(name);
        let mut rgba = one.rgba.clone();
        rgba.extend_from_slice(&one.rgba);
        (Arc::new(Img { rgba, width: one.width, height: one.height * 2 }), one.height as f32)
    }

    #[test]
    fn a_family_that_won_a_line_leads_the_next_lines_list() {
        // five Josefin copies sort before Montserrat, which wins the first line only after all of them are fitted
        let mut files = vec![("Montserrat-Regular.ttf", "F Montserrat", 400)];
        let gaps = ["A Josefin", "B Josefin", "C Josefin", "D Josefin", "E Josefin"];
        files.extend(gaps.iter().map(|g| ("JosefinSans-Light.ttf", *g, 300)));
        let dir = cache(&files);
        let mut fams: Vec<(&str, u16)> = gaps.iter().map(|g| (*g, 300)).collect();
        fams.push(("F Montserrat", 400));
        let (img, h) = stacked("montserrat-est-30-t0-white.png");
        let finder = FixtureFinder(vec![line_box(&img, 14.0, 58.0), line_box(&img, h + 14.0, h + 58.0)]);
        let d = deps(Arc::new(finder), None, index(&fams), &dir);
        let planned = run_plan(&img, &d);
        let (first, second) = (&planned.plan.lines[0], &planned.plan.lines[1]);
        assert_eq!(first.candidates[0].font.family, "F Montserrat");
        assert_eq!(first.candidates.len(), 6, "the first line fitted every family up to the winner");
        assert_eq!(second.candidates[0].font.family, "F Montserrat");
        // the winner led, took one of the four eager places, and its fit ended the search at the fifth
        assert_eq!(second.candidates.len(), 4, "{:?}", second.candidates.iter().map(|c| &c.font.family).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Failing(Result<Vec<Reading>, &'static str>);
    impl Reader for Failing {
        fn read<'a>(&'a self, _: &'a Img, _: &'a [LineBox]) -> ReadFuture<'a> {
            let r = self.0.clone().map_err(studi0trace_desktop::redraw::error);
            Box::pin(async move { r })
        }
    }

    #[test]
    fn a_reader_that_fails_or_answers_the_wrong_count_leaves_vision_s_strings() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400)]);
        let img = image("montserrat-est-30-t0-white.png");
        for (reader, code) in [(Failing(Err("quota")), "quota"), (Failing(Ok(Vec::new())), "bad_reply")] {
            let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), Some(Arc::new(reader)), index(&[("Montserrat", 400)]), &dir);
            let planned = run_plan(&img, &d);
            assert_eq!(planned.plan.ai_error.as_deref(), Some(code));
            assert_eq!(planned.plan.reader, studi0trace_desktop::text::ReaderKind::Vision);
            assert_eq!(planned.plan.lines[0].string, "EST. 2026");
            assert_eq!(planned.plan.lines[0].class, Class::Type);
            assert_eq!(planned.plan.lines[0].status, Status::Matched);
        }
        // and one that answers is used
        let ok = Failing(Ok(vec![Reading { string: "EST. 2026".into(), hints: Vec::new(), class: Class::Type }]));
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), Some(Arc::new(ok)), index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        assert!(planned.plan.ai_error.is_none() && planned.plan.reader == studi0trace_desktop::text::ReaderKind::Ai);
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Opaque(LineBox);
    impl Finder for Opaque {
        fn find(&self, source: &Img) -> Result<Vec<LineBox>, studi0trace_desktop::error::CommandError> {
            assert!(source.rgba.chunks(4).all(|p| p[3] == 255), "the finder is handed an opaque image");
            assert_eq!(&source.rgba[..4], &[255, 255, 255, 255], "a transparent pixel is white to it");
            Ok(vec![self.0.clone()])
        }
    }

    #[test]
    fn the_finder_sees_the_image_on_white_while_the_ink_and_the_paint_out_keep_its_alpha() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400)]);
        let mut img = Img::decode(&std::fs::read(format!("{FIXTURES}/montserrat-est-30-t0-white.png")).unwrap()).unwrap();
        for y in 0..4usize {
            for x in 0..4usize {
                let at = (y * img.width as usize + x) * 4;
                img.rgba[at..at + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        let img = Arc::new(img);
        let d = deps(Arc::new(Opaque(line_box(&img, 14.0, 58.0))), None, index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        assert_eq!(planned.plan.lines[0].status, Status::Matched);
        let painted = Img::decode(planned.painted.as_ref().unwrap()).unwrap();
        assert_eq!(painted.rgba[3], 0, "the painted image keeps the transparent corner");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_whose_font_is_not_in_the_cache_is_neither_composed_nor_painted_out() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400)]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        assert_eq!(compose_group(&planned.plan, false, &d.catalog, &dir).lines, ["img-t0"]);
        assert!(planned.painted.is_some());
        // the same plan against a cache that has lost the file
        let empty = cache(&[]);
        let composed = compose_group(&planned.plan, false, &d.catalog, &empty);
        assert!(composed.lines.is_empty() && composed.group == "<g id=\"text\"></g>");
        let lost = Deps { cache_dir: empty.clone(), ..d.clone() };
        let again = edit(&img, &planned, "img-t0", LineChange { enabled: Some(true), ..Default::default() }, &lost);
        assert!(again.painted.is_none(), "the text is not painted out when nothing is drawn in its place");
        assert_eq!(again.group, "<g id=\"text\"></g>");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }

    // ---- final fix wave ----

    #[test]
    fn a_font_picked_for_a_declined_line_is_a_repaint_and_a_string_edit_of_a_matched_line_is_not() {
        use studi0trace_desktop::text::repainted;
        // only Josefin Light is offered: it scores under the accept on Montserrat ink, so the line is declined
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400), ("JosefinSans-Light.ttf", "Josefin Sans", 300)]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Josefin Sans", 300), ("Montserrat", 400)]), &dir);
        let only_josefin = Deps { catalog: Arc::new(index(&[("Josefin Sans", 300)])), ..d.clone() };
        let declined = run_plan(&img, &only_josefin);
        assert!(matches!(declined.plan.lines[0].status, Status::Declined(_)), "{:?}", declined.plan.lines[0]);
        assert!(declined.painted.is_none());
        let picked = edit(&img, &declined, "img-t0", LineChange { font: Some(font("Montserrat", 400)), ..Default::default() }, &d);
        assert_eq!(picked.plan.lines[0].status, Status::Matched);
        assert!(repainted(&declined, &picked), "the line is now painted out: the trace must be redone");
        // a string edit of the matched line paints out the same ink
        let restrung = edit(&img, &picked, "img-t0", LineChange { string: Some("EST. 2027".into()), ..Default::default() }, &d);
        assert_eq!(restrung.plan.lines[0].status, Status::Matched);
        assert!(!repainted(&picked, &restrung), "same composed lines, same painted bytes");
        // and switching it off is a repaint again
        let off = edit(&img, &restrung, "img-t0", LineChange { enabled: Some(false), ..Default::default() }, &d);
        assert!(repainted(&restrung, &off));
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Panicking;
    impl Reader for Panicking {
        fn read<'a>(&'a self, _: &'a Img, _: &'a [LineBox]) -> ReadFuture<'a> {
            panic!("an image with no text must never reach the AI reader");
        }
    }

    #[test]
    fn an_image_in_which_vision_found_no_text_is_never_handed_to_the_ai_reader() {
        let dir = cache(&[]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(Vec::new())), Some(Arc::new(Panicking)), index(&[("Montserrat", 400)]), &dir);
        let phases: Arc<Mutex<Vec<Phase>>> = Arc::default();
        let seen = phases.clone();
        let planned = tauri::async_runtime::block_on(plan("img", img, d, Arc::new(move |p| seen.lock().unwrap().push(p)))).unwrap();
        assert!(planned.plan.lines.is_empty());
        assert_eq!(planned.plan.reader, studi0trace_desktop::text::ReaderKind::Vision);
        assert!(planned.plan.ai_error.is_none());
        assert!(!phases.lock().unwrap().contains(&Phase::Reading));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn planned_with(painted: Option<Vec<u8>>, group: &str) -> studi0trace_desktop::text::Planned {
        let p = sample_plan(&font("Montserrat", 400), true, Status::Matched);
        studi0trace_desktop::text::Planned { plan: p, inks: Vec::new(), painted: painted.map(Arc::new), group: group.into() }
    }

    #[test]
    fn an_edit_is_stored_only_while_the_image_keeps_the_bytes_it_was_made_for() {
        let plans = Plans::default();
        let (hash, other) = (bytes_hash(b"one"), bytes_hash(b"two"));
        assert!(plans.put_if("img", hash, planned_with(None, "<g id=\"text\"></g>")).is_none(), "no entry: nothing stored");
        assert!(plans.get("img", &hash).is_none());
        plans.put("img", hash, planned_with(None, "a"));
        assert!(plans.put_if("img", other, planned_with(None, "b")).is_none(), "the bytes were swapped while the edit ran");
        assert_eq!(plans.get("img", &hash).unwrap().group, "a");
        let kept = plans.put_if("img", hash, planned_with(None, "c")).unwrap();
        assert!(Arc::ptr_eq(&plans.get("img", &hash).unwrap(), &kept));
        plans.forget("img");
        assert!(plans.put_if("img", hash, planned_with(None, "d")).is_none(), "closed while the edit ran");
    }

    #[test]
    fn a_finished_trace_takes_the_group_of_an_edit_that_kept_its_painted_bytes() {
        let plans = Plans::default();
        let hash = bytes_hash(b"one");
        let submitted = plans.put("img", hash, planned_with(Some(vec![1, 2, 3]), "old")).painted.clone().unwrap();
        // a string edit: a new group, the same painted bytes (encoded again, so another allocation)
        plans.put_if("img", hash, planned_with(Some(vec![1, 2, 3]), "new")).unwrap();
        assert_eq!(plans.group_for("img", &hash, &submitted).as_deref(), Some("new"));
        // an edit that repainted: the trace keeps what it was submitted with
        plans.put_if("img", hash, planned_with(Some(vec![9]), "other")).unwrap();
        assert!(plans.group_for("img", &hash, &submitted).is_none());
        plans.put_if("img", hash, planned_with(None, "none")).unwrap();
        assert!(plans.group_for("img", &hash, &submitted).is_none());
        assert!(plans.group_for("img", &bytes_hash(b"two"), &submitted).is_none(), "other bytes");
    }

    #[test]
    fn a_control_character_in_an_edit_never_reaches_the_svg() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400)]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        let edited = edit(&img, &planned, "img-t0", LineChange { string: Some("EST.\u{0} 2027\u{1b}".into()), ..Default::default() }, &d);
        assert_eq!(edited.plan.lines[0].string, "EST. 2027");
        assert!(!edited.group.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f), "{:?}", edited.group);
        assert!(edited.group.contains(">EST. 2027</text>"));
        // and from a reader
        let reader = Failing(Ok(vec![Reading { string: "EST.\u{7}\u{0} 2026".into(), hints: Vec::new(), class: Class::Type }]));
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), Some(Arc::new(reader)), index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        assert_eq!(planned.plan.lines[0].string, "EST. 2026");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A Google Fonts stand-in on loopback serving `ttf` for every family, each connection on its own thread;
    /// returns its base and how many requests it answered.
    fn google_mock(ttf: Vec<u8>) -> (String, Arc<Mutex<usize>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let count: Arc<Mutex<usize>> = Arc::default();
        let (seen, own) = (count.clone(), base.clone());
        std::thread::spawn(move || {
            for s in listener.incoming() {
                let Ok(mut s) = s else { return };
                let (ttf, base, seen) = (ttf.clone(), own.clone(), seen.clone());
                std::thread::spawn(move || {
                    let mut buf = vec![0u8; 8192];
                    let n = s.read(&mut buf).unwrap_or(0);
                    *seen.lock().unwrap() += 1;
                    let path = String::from_utf8_lossy(&buf[..n]).split(' ').nth(1).unwrap_or("").to_string();
                    let (ctype, body) = if path.starts_with("/css2") {
                        ("text/css", format!("@font-face {{ src: url({base}/s/f.ttf) format('truetype'); }}").into_bytes())
                    } else {
                        ("font/ttf", ttf)
                    };
                    let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(&body);
                });
            }
        });
        (base, count)
    }

    #[test]
    fn a_string_edit_of_a_line_declined_for_want_of_fonts_downloads_its_shortlist_first() {
        let dir = cache(&[]);
        let img = image("montserrat-est-30-t0-white.png");
        // no fetcher: the shortlist's only font is never downloaded and the line is declined for want of fonts
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        let line = &planned.plan.lines[0];
        assert!(line.status == Status::Declined(studi0trace_desktop::text::Reason::Fonts) && line.candidates.is_empty(), "{line:?}");
        // the edit, with a fetcher: the shortlist is downloaded before the fit, and the line matches
        let ttf = std::fs::read(Path::new(FONTS).join("Montserrat-Regular.ttf")).unwrap();
        let (base, count) = google_mock(ttf);
        let gf = studi0trace_desktop::text::fonts::GoogleFonts::new(dir.clone(), &base).unwrap();
        let online = Deps { fonts: Some(Arc::new(gf)), ..d.clone() };
        let edited = edit(&img, &planned, "img-t0", LineChange { string: Some("EST. 2027".into()), ..Default::default() }, &online);
        assert_eq!(*count.lock().unwrap(), 2, "the stylesheet and the file");
        assert!(dir.join(cache_name(&font("Montserrat", 400))).exists());
        let line = &edited.plan.lines[0];
        assert!(line.status == Status::Matched && line.enabled && line.string == "EST. 2027", "{line:?}");
        assert!(!edited.plan.fonts_offline && edited.painted.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_change_naming_an_unknown_font_or_an_overlong_string_is_refused() {
        let dir = cache(&[("Montserrat-Regular.ttf", "Montserrat", 400)]);
        let img = image("montserrat-est-30-t0-white.png");
        let d = deps(Arc::new(FixtureFinder(vec![line_box(&img, 14.0, 58.0)])), None, index(&[("Montserrat", 400)]), &dir);
        let planned = run_plan(&img, &d);
        let refused = |change: LineChange| tauri::async_runtime::block_on(set_line(img.clone(), &planned, "img-t0", change, d.clone())).err().map(|e| (e.status, e.code().map(str::to_string)));
        let bad = Some((400, Some("bad_request".to_string())));
        // a family the index does not hold (its name would be a file in the cache and a download)
        assert_eq!(refused(LineChange { font: Some(font("../../Evil", 400)), ..Default::default() }), bad);
        // an installed family this Mac does not have (the test's fontdb is empty)
        assert_eq!(refused(LineChange { font: Some(FontRef { source: Source::Installed, ..font("Montserrat", 400) }), ..Default::default() }), bad);
        // "EST. 2026" is 9 characters: 3 x 9 + 16 = 43 is the most an edit may hold
        assert_eq!(refused(LineChange { string: Some("E".repeat(44)), ..Default::default() }), bad);
        assert_eq!(refused(LineChange { string: Some("EST. 2027".into()), ..Default::default() }), None);
        assert_eq!(refused(LineChange { font: Some(font("Montserrat", 400)), ..Default::default() }), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
