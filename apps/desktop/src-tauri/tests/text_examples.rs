//! By hand: `STUDI0TRACE_TEXT_EXAMPLES=<dir with 1.png 2.png 3.png> cargo test --release --test text_examples -- --ignored --nocapture`
//! plans each image with Vision and the real catalogue (Google Fonts fetched into <dir>/fonts), writes
//! <dir>/out/N-plan.json, N-painted.png and N-text.svg, and renders the text group over the painted image to
//! N-check.png (over white: N-text.png) for a look. Nothing of this is in git.
use std::path::PathBuf;
use std::sync::Arc;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::catalog::Catalog;
use studi0trace_desktop::text::fonts::GoogleFonts;
use studi0trace_desktop::text::vision::Vision;
use studi0trace_desktop::text::{plan, Deps};

fn png(rgba: Vec<u8>, w: u32, h: u32) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_raw(w, h, rgba).unwrap().write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
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
        let deps = Deps { finder: Arc::new(Vision), reader: None, catalog: catalog.clone(), fonts: Some(fonts.clone()), cache_dir: cache.clone(), hidden_text: true };
        let t0 = std::time::Instant::now();
        let planned = tauri::async_runtime::block_on(plan(n, source.clone(), deps, Arc::new(|p| eprintln!("  {p:?}")))).unwrap();
        eprintln!("{n}: {} lines in {:.1}s", planned.plan.lines.len(), t0.elapsed().as_secs_f32());
        for l in &planned.plan.lines {
            let top: Vec<String> = l.candidates.iter().take(3).map(|c| format!("{} {} {:.3}", c.font.family, c.font.weight, c.iou)).collect();
            eprintln!("  {:?} {:?} {:?} {:?}", l.string, l.status, l.fit.as_ref().map(|f| (f.size, f.iou)), top);
        }
        std::fs::write(out.join(format!("{n}-plan.json")), serde_json::to_string_pretty(&planned.plan).unwrap()).unwrap();
        if let Some(p) = &planned.painted {
            std::fs::write(out.join(format!("{n}-painted.png")), p.as_slice()).unwrap();
        }
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"#fff\"/>{}</svg>", source.width, source.height, planned.group);
        std::fs::write(out.join(format!("{n}-text.svg")), &svg).unwrap();
        let px = studi0trace_core::render::render(&svg, source.width, source.height, false).unwrap();
        std::fs::write(out.join(format!("{n}-text.png")), png(px, source.width, source.height)).unwrap();
        // the group over the painted image (or over the source when no line was composed)
        let overlay = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\">{}</svg>", source.width, source.height, planned.group);
        let top = studi0trace_core::render::render(&overlay, source.width, source.height, false).unwrap();
        let base = match &planned.painted {
            Some(p) => image::load_from_memory(p).unwrap().to_rgba8(),
            None => image::RgbaImage::from_raw(source.width, source.height, source.rgba.clone()).unwrap(),
        };
        let mut base = base;
        image::imageops::overlay(&mut base, &image::RgbaImage::from_raw(source.width, source.height, top).unwrap(), 0, 0);
        std::fs::write(out.join(format!("{n}-check.png")), png(base.into_raw(), source.width, source.height)).unwrap();
    }
}
