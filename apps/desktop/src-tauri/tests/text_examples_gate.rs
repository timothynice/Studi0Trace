//! The real-image gate (by hand, never in CI: the images are not in git):
//! `STUDI0TRACE_TEXT_EXAMPLES=<dir with 1.png 2.png 3.png> cargo test --release --test text_examples_gate -- --ignored --nocapture`
//! plans Tim's three example images with Vision, the real catalogue and real Google downloads (into <dir>/fonts)
//! and holds what the text re-setting must do on them: every tracked all-caps tagline Matched in the ink's weight,
//! under the string with its word spaces put back, the scripts and the gradient declined, and image 3 planned in
//! under 25 s, and the layering: image 2's ADVENTURE reports the gold script's colour among its occluders, image 1's LUMORA none. It writes <dir>/out/N-plan.json and N-check.png (the group the frontend composes from the chosen candidates' paths, over the full mask's painted image) as the harness
//! does: the gate is also the eye's, and every Matched line's crop is to be looked at against the source.
use std::path::PathBuf;
use std::sync::Arc;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::catalog::Catalog;
use studi0trace_desktop::text::fonts::GoogleFonts;
use studi0trace_desktop::text::ink::delta_e;
use studi0trace_desktop::text::plan::stroke_agrees;
use studi0trace_desktop::text::vision::Vision;
use studi0trace_desktop::text::{paint_mask, plan, Deps, Status, TextLine, TextPlan};

fn png(rgba: Vec<u8>, w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(w, h, rgba).unwrap().write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// The group the frontend composes (every matched, enabled line's chosen candidate path in the ink's colour) and the
/// mask of those lines' ids.
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

fn chosen(l: &TextLine) -> String {
    l.chosen.and_then(|i| l.candidates.get(i)).map(|c| format!("{} {}", c.font.family, c.font.weight)).unwrap_or_else(|| "-".into())
}

#[test]
#[ignore]
fn the_taglines_are_matched_and_the_scripts_declined() {
    let Ok(dir) = std::env::var("STUDI0TRACE_TEXT_EXAMPLES") else { return };
    let dir = PathBuf::from(dir);
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let cache = dir.join("fonts");
    let fonts = Arc::new(GoogleFonts::new(cache.clone(), &studi0trace_desktop::text::fonts::base_url()).unwrap());
    let catalog = Arc::new(Catalog::load());
    let mut failures: Vec<String> = Vec::new();
    let mut total = 0f32;
    // per image: the lines that must be Matched, the ones that must be declined, the ones that may be either but
    // only in a sans-serif, and how many of the serif small words must be Matched
    // (each must-decline entry is the readings it may be found under; one of them must be found)
    type Musts<'a> = (&'a str, &'a [&'a str], &'a [&'a [&'a str]], &'a [&'a str]);
    let musts: [Musts; 3] = [
        ("1", &["A BRIGHTER TOMORROW"], &[], &["LUMORA"]),
        ("2", &["EST. 2026", "DENVER, CO", "LIFE IS BETTER OUTSIDE", "FIND WHAT MATTERS", "MORE EVERY DAY", "REACH HIGHER"], &[&["Awaits"], &["Explore"]], &["ADVENTURE", "SUMMIT", "SUMMTT", "MOUNTAINS / FORESTS / LAKES / NEW PERSPECTIVES"]),
        ("3", &["IDEAS FOR A BRIGHTER TOMORROW", "EST. 2026"], &[&["Creative", "reative"]], &["HORIZON"]),
    ];
    let serif_words = ["GOOD", "PEOPLE", "GREAT", "PLACES", "EXPLORE", "CREATE", "BELONG"];
    for (n, matched, declined, sans) in musts {
        let Ok(bytes) = std::fs::read(dir.join(format!("{n}.png"))) else {
            failures.push(format!("{n}.png is missing"));
            continue;
        };
        let source = Arc::new(Source::decode(&bytes).unwrap());
        let deps = Deps { finder: Arc::new(Vision), reader: None, catalog: catalog.clone(), fonts: Some(fonts.clone()), cache_dir: cache.clone() };
        let t0 = std::time::Instant::now();
        let planned = tauri::async_runtime::block_on(plan(n, source.clone(), deps, Arc::new(|_| {}))).unwrap();
        let secs = t0.elapsed().as_secs_f32();
        total += secs;
        let lines = &planned.plan.lines;
        let (m, d) = (lines.iter().filter(|l| l.status == Status::Matched).count(), lines.iter().filter(|l| matches!(l.status, Status::Declined(_))).count());
        eprintln!("{n}: {} lines, {m} matched, {d} declined, {secs:.1} s", lines.len());
        for l in lines {
            let f = l.fit.as_ref();
            eprintln!("  {:<48} {:<22} {:<28} iou {:.3} stroke {:.2}", format!("{:?}", l.string), format!("{:?}", l.status), chosen(l), f.map_or(l.candidates.first().map_or(0.0, |c| c.iou), |f| f.iou), f.map_or(0.0, |f| f.stroke));
        }
        let find = |s: &str| lines.iter().find(|l| l.string == s);
        for s in matched {
            match find(s) {
                Some(l) if l.status == Status::Matched && l.enabled => {}
                Some(l) => failures.push(format!("{n}: {s:?} is {:?}, not Matched", l.status)),
                None => failures.push(format!("{n}: no line reads {s:?} (strings: {:?})", lines.iter().map(|l| &l.string).collect::<Vec<_>>())),
            }
        }
        for readings in declined {
            match readings.iter().find_map(|s| find(s)) {
                Some(l) if !matches!(l.status, Status::Declined(_)) => failures.push(format!("{n}: {:?} is {:?} in {}, not declined", l.string, l.status, chosen(l))),
                Some(_) => {}
                None => failures.push(format!("{n}: no line reads any of {readings:?}")),
            }
        }
        // Wander may be re-set only in a serif
        if n == "2" {
            match find("Wander") {
                Some(l) if l.status == Status::Matched => {
                    let family = &l.candidates[l.chosen.unwrap()].font.family;
                    if catalog.category(family) != Some("Serif") {
                        failures.push(format!("{n}: Wander is Matched in {family}, not a serif"));
                    }
                }
                Some(_) => {}
                None => failures.push(format!("{n}: no line reads \"Wander\"")),
            }
        }
        for s in sans {
            if let Some(l) = find(s).filter(|l| l.status == Status::Matched) {
                let family = &l.candidates[l.chosen.unwrap()].font.family;
                if let Some(cat) = catalog.category(family).filter(|c| *c != "Sans Serif") {
                    failures.push(format!("{n}: {s:?} is Matched in {family}, a {cat}"));
                }
            }
        }
        if n == "2" {
            let words = serif_words.iter().filter(|w| find(w).is_some_and(|l| l.status == Status::Matched)).count();
            if words < 6 {
                failures.push(format!("2: {words} of the 7 serif small words Matched, under 6"));
            }
        }
        // the layering: ADVENTURE's letters lie under the gold script "Awaits", and the plan must say so (the frontend
        // composes the line under that colour's shapes); LUMORA's have nothing over them
        if n == "2" {
            // the script's own colour, sampled: the commonest colour well off the page's in the window round the A of
            // Awaits. The window (x 470-520, y 440-500, the page sampled at 470,440) is hardcoded for this one image
            // (1536 x 1024), where the A of "Awaits" lies; another 2.png needs it chosen again
            let px = |x: u32, y: u32| -> [u8; 3] { let i = ((y * source.width + x) * 4) as usize; [source.rgba[i], source.rgba[i + 1], source.rgba[i + 2]] };
            let page = px(470, 440);
            let mut counts: std::collections::HashMap<[u8; 3], u32> = std::collections::HashMap::new();
            for y in 440..500 {
                for x in 470..520 {
                    if delta_e(px(x, y), page) > 25.0 {
                        *counts.entry(px(x, y)).or_default() += 1;
                    }
                }
            }
            let gold = counts.into_iter().max_by_key(|&(c, k)| (k, c)).map(|(c, _)| c).unwrap();
            match find("ADVENTURE") {
                Some(l) => {
                    eprintln!("  ADVENTURE ({:?}) occluders {:?}, the script's colour sampled at {gold:?}: {:?}", l.status, l.occluders, l.occluders.iter().map(|&o| delta_e(o, gold)).collect::<Vec<_>>());
                    if !l.occluders.iter().any(|&o| delta_e(o, gold) <= 12.0) {
                        failures.push(format!("2: ADVENTURE's occluders {:?} hold nothing within 12 delta-E of the script's {gold:?}", l.occluders));
                    }
                }
                None => failures.push("2: no line reads \"ADVENTURE\"".into()),
            }
        }
        if n == "1" {
            match find("LUMORA") {
                Some(l) => {
                    eprintln!("  LUMORA occluders {:?}", l.occluders);
                    if !l.occluders.is_empty() {
                        failures.push(format!("1: LUMORA reports occluders {:?}", l.occluders));
                    }
                }
                None => failures.push("1: no line reads \"LUMORA\"".into()),
            }
        }
        for l in lines.iter().filter(|l| l.status == Status::Matched) {
            // a stroke that was never measured (0) does not pass here
            if !l.fit.as_ref().is_some_and(|f| f.stroke > 0.0 && stroke_agrees(f)) {
                failures.push(format!("{n}: {:?} is Matched with stroke {:?}", l.string, l.fit.as_ref().map(|f| f.stroke)));
            }
        }
        if n == "3" && secs >= 25.0 {
            failures.push(format!("3: planned in {secs:.1} s, not under 25"));
        }
        std::fs::write(out.join(format!("{n}-plan.json")), serde_json::to_string_pretty(&planned.plan).unwrap()).unwrap();
        let (group, mask) = group_and_mask(&planned.plan);
        let overlay = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">{}</svg>", source.width, source.height, group);
        let top = studi0trace_core::render::render(&overlay, source.width, source.height, false).unwrap();
        let mut base = match paint_mask(&source, &planned, &mask).unwrap() {
            Some(p) => image::load_from_memory(&p).unwrap().to_rgba8(),
            None => image::RgbaImage::from_raw(source.width, source.height, source.rgba.clone()).unwrap(),
        };
        image::imageops::overlay(&mut base, &image::RgbaImage::from_raw(source.width, source.height, top).unwrap(), 0, 0);
        std::fs::write(out.join(format!("{n}-check.png")), png(base.into_raw(), source.width, source.height)).unwrap();
    }
    eprintln!("total {total:.1} s");
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
