//! The fit on the fixture set: with the true font it recovers size, baseline and tracking, scores a high IoU and
//! draws the ink's stroke; every wrong font scores lower. FIT_ACCEPT is a floor set on real sources, under every
//! true font here with a margin; what keeps a wrong weight off a line is the stroke (STROKE_AGREE), and a light
//! face's line is never accepted in the regular.
use serde::Deserialize;
use std::path::PathBuf;
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::fit::fit_line;
use studi0trace_desktop::text::glyphs::Font;
use studi0trace_desktop::text::ink::line_ink;
use studi0trace_desktop::text::{FIT_ACCEPT, STROKE_AGREE};

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
    let mut wrong_accepted = 0;
    let mut strokes: Vec<(String, String, f32)> = Vec::new();
    let mut true_strokes: Vec<f32> = Vec::new();
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
                let agrees = (fit.stroke - 1.0).abs() <= STROKE_AGREE;
                if fit.iou >= FIT_ACCEPT && agrees {
                    wrong_accepted += 1;
                }
                // Josefin Sans Light's all-caps lines (a 300) in Montserrat Regular (a 400 with twice its stem) are
                // the wrong weight the IoU alone accepted on real sources; the stroke must refuse them. (On the
                // lowercase line the mid-height rows cross Montserrat's tall bowls, and it reads only 1.1-1.2.)
                if item.font.starts_with("Josefin") && name.starts_with("Montserrat") && all_caps && item.cap_px >= 18.0 && agrees {
                    failures.push(format!("{}: Montserrat Regular's stroke {:.2} passes on Josefin Light", item.file, fit.stroke));
                }
                strokes.push((name.to_string(), item.font.clone(), fit.stroke));
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
            (fit.iou >= 0.83, format!("iou {}", fit.iou)),
            (fit.iou > best_wrong, format!("a wrong font scored {best_wrong} against {}", fit.iou)),
            ((fit.stroke - 1.0).abs() <= STROKE_AGREE, format!("the true font's stroke {}", fit.stroke)),
        ];
        true_strokes.push(fit.stroke);
        for (ok, why) in checks {
            if !ok {
                failures.push(format!("{}: {why}", item.file));
            }
        }
    }
    true_strokes.sort_by(f32::total_cmp);
    eprintln!("true-font IoU min {true_min:.3}; wrong-font IoU max {wrong_max:.3}; FIT_ACCEPT {FIT_ACCEPT}");
    eprintln!("true-font stroke {:.2}..{:.2}; wrong fits the acceptance passes: {wrong_accepted} (each beaten by the true font)", true_strokes[0], true_strokes[true_strokes.len() - 1]);
    for pair in [("Montserrat", "Josefin"), ("Josefin", "Montserrat"), ("Playfair", "Montserrat"), ("Montserrat", "Playfair"), ("Josefin", "Playfair"), ("Playfair", "Josefin")] {
        let mut v: Vec<f32> = strokes.iter().filter(|(f, t, _)| f.starts_with(pair.0) && t.starts_with(pair.1)).map(|s| s.2).collect();
        v.sort_by(f32::total_cmp);
        eprintln!("  {} on {}: stroke {:.2}..{:.2}", pair.0, pair.1, v[0], v[v.len() - 1]);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(FIT_ACCEPT <= true_min - 0.04, "FIT_ACCEPT must sit under every true font with a margin");
}
