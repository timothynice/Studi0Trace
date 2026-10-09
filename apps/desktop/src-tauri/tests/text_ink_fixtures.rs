//! The ink reader on the fixture set: real anti-aliased text, thin strokes included (Josefin Sans Light at 12 px
//! caps has 1-2 px strokes), is never declined as a gradient or a mismatch, its ink colour is the truth's and it
//! never finds more segments than the string has characters.
use serde::Deserialize;
use std::path::PathBuf;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::ink::{decline, delta_e, line_ink};
use studi0trace_desktop::text::Class;

#[derive(Deserialize)]
struct Item {
    file: String,
    string: String,
    cap_px: f32,
    baseline: f32,
    colour: [u8; 3],
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/text")
}

#[test]
fn no_fixture_line_is_declined_and_the_ink_reads_true() {
    let items: Vec<Item> = serde_json::from_str(&std::fs::read_to_string(dir().join("truth.json")).unwrap()).unwrap();
    assert_eq!(items.len(), 108);
    let mut failures = Vec::new();
    let mut worst = 0f32;
    for item in &items {
        let source = Source::decode(&std::fs::read(dir().join(&item.file)).unwrap()).unwrap();
        let (w, h) = (source.width as f32, source.height as f32);
        let quad = [[20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, (item.baseline + 8.0).min(h)], [20.0, (item.baseline + 8.0).min(h)]];
        let ink = line_ink(&source, quad);
        worst = worst.max(ink.spread);
        if let Some(reason) = decline(&ink, &item.string, Class::Type, quad) {
            failures.push(format!("{}: declined {reason:?} (spread {:.2}, {} segments)", item.file, ink.spread, ink.segments.len()));
        }
        let off = delta_e(ink.colour, item.colour);
        if off > 12.0 {
            failures.push(format!("{}: ink colour {:?} is {off:.1} ΔE from {:?}", item.file, ink.colour, item.colour));
        }
        let chars = item.string.chars().filter(|c| !c.is_whitespace()).count();
        if ink.segments.len() > chars {
            failures.push(format!("{}: {} segments for {chars} characters", item.file, ink.segments.len()));
        }
    }
    eprintln!("worst spread {worst:.2}");
    assert!(failures.is_empty(), "{} failures\n{}", failures.len(), failures.join("\n"));
}
