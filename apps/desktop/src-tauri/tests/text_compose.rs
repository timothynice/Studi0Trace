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
    assert!(!planned.group.contains("<text"), "hidden text is off");
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
