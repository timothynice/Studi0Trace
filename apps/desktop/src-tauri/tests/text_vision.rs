//! Vision reads a fixture line back: one observation, its string, a quad around the text. macOS only; elsewhere
//! the finder says `text_unavailable`.
use studi0trace_desktop::redraw::Source;
use studi0trace_desktop::text::reader::Finder;
use studi0trace_desktop::text::vision::Vision;

fn fixture(name: &str) -> Source {
    let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/text/");
    Source::decode(&std::fs::read(format!("{p}{name}")).unwrap()).unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn reads_est_2026_with_a_box_around_it() {
    let source = fixture("montserrat-est-30-t0-white.png");
    let lines = Vision.find(&source).unwrap();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let line = &lines[0];
    assert_eq!(line.string.trim(), "EST. 2026");
    assert!(line.confidence > 0.5);
    let xs: Vec<f32> = line.quad.iter().map(|p| p[0]).collect();
    let ys: Vec<f32> = line.quad.iter().map(|p| p[1]).collect();
    // the text starts at x = 30, baseline at y = 50 (cap 30 + 20), so the box spans roughly that
    assert!(xs.iter().cloned().fold(f32::MAX, f32::min) < 34.0 && xs.iter().cloned().fold(f32::MIN, f32::max) > 100.0, "{xs:?}");
    assert!(ys.iter().cloned().fold(f32::MAX, f32::min) < 24.0 && ys.iter().cloned().fold(f32::MIN, f32::max) > 46.0, "{ys:?}");
    // top-left, top-right, bottom-right, bottom-left
    assert!(line.quad[0][1] < line.quad[3][1] && line.quad[0][0] < line.quad[1][0]);
}

#[cfg(target_os = "macos")]
#[test]
fn reads_two_lines_in_order_top_to_bottom() {
    // two fixture lines stacked into one image
    let a = fixture("montserrat-est-30-t0-white.png");
    let b = fixture("montserrat-life-18-t2-white.png");
    let w = a.width.max(b.width);
    let h = a.height + b.height;
    let mut rgba = vec![255u8; (w * h * 4) as usize];
    for (src, y0) in [(&a, 0), (&b, a.height)] {
        for y in 0..src.height {
            let row = &src.rgba[((y * src.width) * 4) as usize..((y * src.width + src.width) * 4) as usize];
            let at = (((y0 + y) * w) * 4) as usize;
            rgba[at..at + row.len()].copy_from_slice(row);
        }
    }
    let lines = Vision.find(&Source { rgba, width: w, height: h }).unwrap();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].string.trim(), "EST. 2026");
    assert_eq!(lines[1].string.trim(), "LIFE IS BETTER OUTSIDE");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn elsewhere_text_is_unavailable() {
    let err = Vision.find(&fixture("montserrat-est-30-t0-white.png")).unwrap_err();
    assert_eq!(err.code(), Some("text_unavailable"));
}
