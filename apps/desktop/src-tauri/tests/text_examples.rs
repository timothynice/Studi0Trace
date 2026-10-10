//! By hand: `STUDI0TRACE_TEXT_EXAMPLES=<dir with 1.png 2.png 3.png> cargo test --release --test text_examples -- --ignored --nocapture`
//! plans each image with Vision and the real catalogue (Google Fonts fetched into <dir>/fonts), writes
//! <dir>/out/N-plan.json, N-painted.png and N-text.svg (the group the frontend composes from the chosen candidates' paths, and the
//! painted image of the full mask), and renders the text group over the painted image to
//! N-check.png (over white: N-text.png) for a look. With `STUDI0TRACE_TEXT_FORCE=1` every declined line that has a
//! candidate is also set in its best candidate, to N-forced-plan.json, N-forced-painted.png and N-forced-check.png. Nothing of this is in git.
use std::path::PathBuf;
use std::sync::Arc;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::catalog::Catalog;
use studi0trace_desktop::text::fonts::GoogleFonts;
use studi0trace_desktop::text::vision::Vision;
use studi0trace_desktop::text::{paint_mask, plan, set_line, Deps, LineChange, Planned, Status, TextPlan};

fn png(rgba: Vec<u8>, w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(w, h, rgba).unwrap().write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// What the frontend composes: the chosen candidate's path of every matched, enabled line in the ink's colour, and the
/// ids of those lines (the mask a trace paints out).
fn group_and_mask(plan: &TextPlan) -> (String, Vec<String>) {
    let mut group = String::from("<g id=\"text\">");
    let mut mask = Vec::new();
    for l in plan.lines.iter().filter(|l| l.status == Status::Matched && l.enabled) {
        let Some(c) = l.chosen.and_then(|i| l.candidates.get(i)) else { continue };
        let [r, g, b] = l.colour;
        group.push_str(&format!("<path d=\"{}\" fill=\"#{r:02x}{g:02x}{b:02x}\" data-line=\"{}\"/>", c.path, l.id));
        mask.push(l.id.clone());
    }
    group.push_str("</g>");
    (group, mask)
}

/// The source with the lines of the full mask painted out (the source itself when there are none).
fn painted_image(source: &Source, planned: &Planned, mask: &[String]) -> image::RgbaImage {
    match paint_mask(source, planned, mask).unwrap() {
        Some(p) => image::load_from_memory(&p).unwrap().to_rgba8(),
        None => image::RgbaImage::from_raw(source.width, source.height, source.rgba.clone()).unwrap(),
    }
}

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
        let deps = Deps { finder: Arc::new(Vision), reader: None, catalog: catalog.clone(), fonts: Some(fonts.clone()), cache_dir: cache.clone() };
        let t0 = std::time::Instant::now();
        let planned = tauri::async_runtime::block_on(plan(n, source.clone(), deps, Arc::new(|p| eprintln!("  {p:?}")))).unwrap();
        eprintln!("{n}: {} lines in {:.1}s", planned.plan.lines.len(), t0.elapsed().as_secs_f32());
        for l in &planned.plan.lines {
            let top: Vec<String> = l.candidates.iter().take(3).map(|c| format!("{} {} {:.3}", c.font.family, c.font.weight, c.iou)).collect();
            eprintln!("  {:?} {:?} {:?} {:?}", l.string, l.status, l.fit.as_ref().map(|f| (f.size, f.iou, f.stroke)), top);
        }
        std::fs::write(out.join(format!("{n}-plan.json")), serde_json::to_string_pretty(&planned.plan).unwrap()).unwrap();
        let (group, mask) = group_and_mask(&planned.plan);
        if let Some(p) = paint_mask(&source, &planned, &mask).unwrap() {
            std::fs::write(out.join(format!("{n}-painted.png")), p).unwrap();
        }
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>{}</svg>", source.width, source.height, group);
        std::fs::write(out.join(format!("{n}-text.svg")), &svg).unwrap();
        let px = studi0trace_core::render::render(&svg, source.width, source.height, false).unwrap();
        std::fs::write(out.join(format!("{n}-text.png")), png(px, source.width, source.height)).unwrap();
        // the group over the painted image (or over the source when no line was composed)
        let overlay = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">{}</svg>", source.width, source.height, group);
        let top = studi0trace_core::render::render(&overlay, source.width, source.height, false).unwrap();
        let mut base = painted_image(&source, &planned, &mask);
        image::imageops::overlay(&mut base, &image::RgbaImage::from_raw(source.width, source.height, top).unwrap(), 0, 0);
        std::fs::write(out.join(format!("{n}-check.png")), png(base.into_raw(), source.width, source.height)).unwrap();
        // for a look at what the fit drew on the lines it declined: every declined line with a candidate forced on
        // (its best candidate), to N-forced-plan.json and N-forced-check.png
        if std::env::var("STUDI0TRACE_TEXT_FORCE").is_ok() {
            let mut cur = planned;
            let ids: Vec<String> = cur.plan.lines.iter().filter(|l| l.status != studi0trace_desktop::text::Status::Matched && !l.candidates.is_empty()).map(|l| l.id.clone()).collect();
            for id in ids {
                let font = cur.plan.lines.iter().find(|l| l.id == id).and_then(|l| l.candidates.first()).map(|c| c.font.clone()).unwrap();
                let d = Deps { finder: Arc::new(Vision), reader: None, catalog: catalog.clone(), fonts: Some(fonts.clone()), cache_dir: cache.clone() };
                cur = tauri::async_runtime::block_on(set_line(source.clone(), &cur, &id, LineChange { font: Some(font), ..Default::default() }, d)).unwrap();
            }
            for l in &cur.plan.lines {
                if let (Some(f), Some(c)) = (&l.fit, l.chosen.and_then(|i| l.candidates.get(i))) {
                    eprintln!("  forced {:?}: {} {} iou {:.3} stroke {:.2}", l.string, c.font.family, c.font.weight, f.iou, f.stroke);
                }
            }
            std::fs::write(out.join(format!("{n}-forced-plan.json")), serde_json::to_string_pretty(&cur.plan).unwrap()).unwrap();
            // a forced line is set in its best candidate whatever its enabled flag: the harness composes it all the same
            for l in &mut cur.plan.lines {
                if l.status == Status::Matched {
                    l.enabled = true;
                }
            }
            let (group, mask) = group_and_mask(&cur.plan);
            let overlay = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">{}</svg>", source.width, source.height, group);
            let top = studi0trace_core::render::render(&overlay, source.width, source.height, false).unwrap();
            let mut base = painted_image(&source, &cur, &mask);
            // the forced plan's full mask painted out, to be traced and composed over (the layering check)
            std::fs::write(out.join(format!("{n}-forced-painted.png")), png(base.clone().into_raw(), source.width, source.height)).unwrap();
            image::imageops::overlay(&mut base, &image::RgbaImage::from_raw(source.width, source.height, top).unwrap(), 0, 0);
            std::fs::write(out.join(format!("{n}-forced-check.png")), png(base.into_raw(), source.width, source.height)).unwrap();
        }
    }
}
