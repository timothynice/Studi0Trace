//! The shortlist: on the fixture set, the true family is among the `SHORTLIST` nearest of the keyless ranking,
//! from the line's measured per-character widths (the stem picks the weight, not the family).
use serde::Deserialize;
use std::path::PathBuf;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::catalog::{weight_for_stem, Catalog};
use studi0trace_desktop::text::ink::line_ink;
use studi0trace_desktop::text::{Source as FontSource, SHORTLIST};

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

/// Removes the file it names when it goes out of scope, so a failing assertion leaves nothing for git.
struct Remove(PathBuf);

impl Drop for Remove {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn the_true_family_is_in_the_shortlist_on_every_fixture_whose_letters_are_told_apart() {
    let cat = catalog();
    assert!(cat.families() > 1500, "{}", cat.families());
    let items: Vec<Item> = serde_json::from_str(&std::fs::read_to_string(dir().join("truth.json")).unwrap()).unwrap();
    let mut misses = Vec::new();
    let (mut ranked, mut merged) = (0, 0);
    for item in items.iter().filter(|i| i.cap_px >= 18.0) {
        let source = Source::decode(&std::fs::read(dir().join(&item.file)).unwrap()).unwrap();
        let (w, h) = (source.width as f32, source.height as f32);
        let quad = [[20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, item.baseline - item.cap_px - 6.0], [w - 20.0, (item.baseline + 8.0).min(h)], [20.0, (item.baseline + 8.0).min(h)]];
        let ink = line_ink(&source, quad);
        let widths = studi0trace_desktop::text::catalog::measured_widths(&ink, &item.string);
        // letters that touch are one segment: no per-character widths, and nothing to rank by (the reader's
        // hints and the line's other families carry such a line)
        if widths.is_empty() {
            merged += 1;
            continue;
        }
        ranked += 1;
        let stem = studi0trace_desktop::text::catalog::measured_stem(&ink);
        let top = cat.shortlist(&item.string, &widths, stem, SHORTLIST);
        if !top.iter().any(|f| f.family == item.family) {
            misses.push(format!("{}: {:?}", item.file, top.iter().map(|f| f.family.as_str()).collect::<Vec<_>>()));
        }
    }
    assert!(misses.is_empty(), "{}", misses.join("\n"));
    assert!(ranked >= 60 && merged <= ranked / 5, "{ranked} ranked, {merged} with touching letters");
}

#[test]
fn a_weight_is_chosen_within_the_family_s_variants() {
    let cat = catalog();
    // a hairline stem wants the light end, and the weight is always one the family has
    let top = cat.shortlist("EST", &[('E', 0.64), ('S', 0.64), ('T', 0.66)], 0.05, 3);
    assert!(top.iter().all(|f| cat.weights_of(&f.family, FontSource::Google).contains(&f.weight)), "{top:?}");
    assert!(top.iter().all(|f| f.weight <= 400), "a hairline stem asks for the light end: {top:?}");
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
    let stand_in = cache.join(studi0trace_desktop::text::catalog::cache_name(&f));
    let _remove = Remove(stand_in.clone());
    assert!(cat.locate(&f, &cache).is_none(), "nothing is cached yet");
    std::fs::copy(cache.join("JosefinSans-Light.ttf"), &stand_in).unwrap();
    assert_eq!(cat.locate(&f, &cache).unwrap().0, cache.join("Josefin Sans-300.ttf"));
}
